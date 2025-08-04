use crate::CALL_DEPTH;
use crate::cache::{DefaultProviderCache, ProviderCache};
use crate::fork_provider::ForkProvider;
use alloy::rpc::types::Block;
use eyre::{ContextCompat, Result};
use hashbrown::hash_map::Entry;
use hashbrown::{HashMap, HashSet};
use revm::database::{AccountState, DbAccount};
use revm::primitives::{
    Address, B256, HashMap as RevmHashMap, KECCAK_EMPTY, U256,
    keccak256,
};
use revm::state::{Account, AccountInfo};
use revm::bytecode::Bytecode;
use revm::database_interface::{Database, DatabaseCommit, DBErrorMarker};
use std::env;
use std::fmt;
use tracing::{debug, info, trace};


/// Custom database error type that wraps eyre::Error
#[derive(Debug)]
pub struct ForkDBError {
    inner: eyre::Error,
}

impl ForkDBError {
    /// Create a new ForkDBError from an eyre::Error
    pub fn new(err: eyre::Error) -> Self {
        Self { inner: err }
    }

    /// Create a new ForkDBError from any error type
    pub fn from_error<E: Into<eyre::Error>>(err: E) -> Self {
        Self { inner: err.into() }
    }

    /// Get the inner eyre::Error
    pub fn inner(&self) -> &eyre::Error {
        &self.inner
    }

    /// Convert into the inner eyre::Error
    pub fn into_inner(self) -> eyre::Error {
        self.inner
    }

    /// Convert a Result<T, ForkDBError> to Result<T, eyre::Error>
    pub fn into_eyre_result<T>(result: Result<T, Self>) -> Result<T> {
        result.map_err(|e| e.into_inner())
    }
}

impl fmt::Display for ForkDBError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ForkDB Error: {}", self.inner)
    }
}

impl std::error::Error for ForkDBError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source()
    }
}

impl DBErrorMarker for ForkDBError {}

impl From<eyre::Error> for ForkDBError {
    fn from(err: eyre::Error) -> Self {
        Self::new(err)
    }
}

// Note: We don't implement From<ForkDBError> for eyre::Error to avoid conflicts
// Use .into_inner() method instead to get the inner eyre::Error

impl From<std::io::Error> for ForkDBError {
    fn from(err: std::io::Error) -> Self {
        Self::from_error(err)
    }
}

impl From<String> for ForkDBError {
    fn from(err: String) -> Self {
        Self::from_error(eyre::eyre!(err))
    }
}

impl From<&str> for ForkDBError {
    fn from(err: &str) -> Self {
        Self::from_error(eyre::eyre!(err.to_string()))
    }
}

#[derive(Debug, Default)]
pub struct ForkDB<T: ProviderCache> {
    /// Account info where None means it is not existing. Not existing state is needed for Pre TANGERINE forks.
    /// `code` is always `None`, and bytecode can be found in `contracts`.
    pub accounts: HashMap<Address, DbAccount>,
    /// Tracks all contracts by their code hash.
    pub contracts: HashMap<B256, Bytecode>,
    /// All cached block hashes
    pub block_hashes: HashMap<u64, B256>,

    pub fork_enabled: bool,
    /// Web3 provider
    provider: Option<ForkProvider<T>>,
    /// Optional block ID to fetch data from, if not the latest
    block_id: Option<u64>,
    /// Address loaded remotely
    pub remote_addresses: HashMap<Address, HashSet<U256>>,
    /// Addresses ignored by depth limit
    pub ignored_addresses: HashSet<Address>,
    /// Block caches
    block_cache: HashMap<u64, Block>,
    /// Max depth to consider when forking address
    max_fork_depth: usize,
}

impl Clone for ForkDB<DefaultProviderCache> {
    fn clone(&self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            contracts: self.contracts.clone(),
            block_hashes: self.block_hashes.clone(),
            provider: self.provider.clone(),
            block_id: self.block_id,
            remote_addresses: self.remote_addresses.clone(),
            fork_enabled: self.fork_enabled,
            block_cache: self.block_cache.clone(),
            ignored_addresses: self.ignored_addresses.clone(),
            max_fork_depth: self.max_fork_depth,
        }
    }
}

impl<T: ProviderCache> ForkDB<T> {
    pub fn create() -> Self {
        ForkDB::create_with_provider(None, None)
    }

    /// Returns the forked block id
    fn get_fork_block_id(&mut self) -> Result<u64> {
        if let Some(block_id) = self.block_id {
            return Ok(block_id);
        }

        if let Some(provider) = &self.provider {
            info!("Load current block number from provider");
            let block_number = provider.get_block_number()?;
            Ok(block_number)
        } else {
            Err(eyre::eyre!("No block ID provided"))
        }
    }

    fn get_fork_block_by_number(&mut self, number: u64) -> Result<Block> {
        if let Some(block) = self.block_cache.get(&number) {
            return Ok(block.clone());
        }

        if let Some(provider) = &mut self.provider {
            let block = provider
                .get_block(number)?
                .context("Block does not exist")?;
            self.block_cache.insert(number, block.clone());
            Ok(block)
        } else {
            Err(eyre::eyre!("No provider to retrieve from remote endpoint"))
        }
    }

    /// Get forked block
    pub fn get_fork_block(&mut self) -> Result<Block> {
        let number = self.get_fork_block_id()?;
        self.get_fork_block_by_number(number)
    }

    pub fn create_with_provider(
        provider: Option<ForkProvider<T>>,
        mut block_id: Option<u64>,
    ) -> Self {
        let fork_enabled = provider.is_some();

        if fork_enabled && block_id.is_none() {
            let number = &provider
                .as_ref()
                .unwrap()
                .get_block_number()
                .expect("Getting the latest block number failed");
            block_id = Some(*number);
        }

        let max_fork_depth = env::var("TINYEVM_MAX_FORK_DEPTH")
            .map(|x| x.parse::<usize>())
            .unwrap_or(Ok(usize::MAX))
            .unwrap_or_default();

        Self {
            accounts: HashMap::new(),
            contracts: HashMap::new(),
            block_hashes: HashMap::new(),
            provider,
            block_id,
            remote_addresses: Default::default(),
            fork_enabled,
            block_cache: HashMap::new(),
            ignored_addresses: Default::default(),
            max_fork_depth,
        }
    }

    /// insert account storage without overriding account info
    pub fn insert_account_storage(
        &mut self,
        address: Address,
        slot: U256,
        value: U256,
    ) -> Result<()> {
        trace!("insert_account_storage {}", address);
        let _ = self.basic(address)?;
        self.accounts
            .entry(address)
            .or_default()
            .storage
            .insert(slot, value);
        Ok(())
    }

    /// replace account storage without overriding account info
    pub fn replace_account_storage(
        &mut self,
        address: Address,
        storage: HashMap<U256, U256>,
    ) -> Result<()> {
        let _ = self.basic(address)?;
        let account = self.accounts.entry(address).or_default();
        account.storage = storage.into_iter().collect();
        account.account_state = AccountState::StorageCleared;

        Ok(())
    }

    /// Insert account info but not override storage
    pub fn insert_account_info(&mut self, address: Address, mut info: AccountInfo) {
        self.insert_contract(&mut info);
        self.accounts.entry(address).or_default().info = info;
    }

    pub fn insert_contract(&mut self, account: &mut AccountInfo) {
        let mut changed = false;
        if let Some(code) = &account.code {
            if !code.is_empty() {
                if account.code_hash == KECCAK_EMPTY {
                    account.code_hash = code.hash_slow();
                }
                self.contracts
                    .entry(account.code_hash)
                    .or_insert_with(|| code.clone());
                changed = true;
            }
        }
        if !changed {
            account.code_hash = KECCAK_EMPTY;
        }
    }
}

// The database methods reload from remote endpoint if the data is missing
impl<T: ProviderCache> Database for ForkDB<T> {
    type Error = ForkDBError;
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let add = Address::from(address.0);

        // Use cached account if available
        if let Some(account) = self.accounts.get(&address) {
            return Ok(Some(account.info.clone()));
        }

        if !self.fork_enabled {
            return Ok(None);
        }

        if CALL_DEPTH.get_or_default().get() > self.max_fork_depth {
            self.ignored_addresses.insert(address);
            return Ok(None);
        }

        // Load from ethereum node
        let provider = self.provider.as_mut().unwrap();
        let nonce = provider.get_transaction_count(&add, self.block_id)?;
        let balance = provider.get_balance(&add, self.block_id)?;
        let code = provider.get_code(&add, self.block_id)?;

        info!(
            "Loading account from ethereum node: address {:?} nonce {:?} balance {:?} ",
            address, nonce, balance
        );

        // An exist remotely if there is something in the remote address
        // Assuming an account can't have storage without code
        let is_remote = !code.0.is_empty() || !balance.is_zero() || nonce != 0;

        let info = AccountInfo::new(
            balance,
            nonce,
            keccak256(&code),
            Bytecode::new_raw(code.0.into()),
        );

        // Write to in memory db
        self.insert_account_info(address, info.clone());
        if is_remote {
            self.remote_addresses.entry(address).or_default();
        }

        Ok(Some(info))
    }

    fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        panic!("Not expected, code should be loaded by account");
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let add = Address::from(address.0);
        let uindex = index;
        trace!("retrieve storage {} {}", address, index);

        let _ = self.basic(address)?;

        if let Entry::Occupied(mut acc_entry) = self.accounts.entry(address) {
            let acc_entry = acc_entry.get_mut();
            if let Entry::Occupied(entry) = acc_entry.storage.entry(uindex) {
                return Ok(*entry.get());
            }
        }

        if !self.remote_addresses.contains_key(&address) || !self.fork_enabled {
            return Ok(U256::ZERO);
        }

        let provider = self.provider.as_mut().unwrap();
        let value = provider.get_storage_at(&add, &index, self.block_id)?;

        debug!(
            "Using storage: {:?} index {:?} value {:?} ",
            address, index, value
        );

        self.remote_addresses
            .entry(address)
            .or_default()
            .insert(uindex);

        self.accounts
            .entry(address)
            .or_default()
            .storage
            .insert(uindex, value);
        Ok(value)
    }

    /// Get block hash by block number. Note if fork is not enabled, the block hash
    /// is calculated from the block number
    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        if let Entry::Occupied(entry) = self.block_hashes.entry(number) {
            return Ok(*entry.get());
        }

        if !self.fork_enabled {
            let bytes: [u8; 32] = U256::from(number).to_be_bytes();
            return Ok(keccak256(bytes));
        }

        let block = self.get_fork_block_by_number(number)?;

        let hash = block.header.hash;
        self.block_hashes.insert(number, hash);
        Ok(hash)
    }
}

impl<T: ProviderCache> DatabaseCommit for ForkDB<T> {
    fn commit(&mut self, changes: RevmHashMap<Address, Account>) {
        for (address, mut account) in changes {
            if !account.is_touched() {
                continue;
            }

            if account.is_selfdestructed() {
                let db_account = self.accounts.entry(address).or_default();
                db_account.storage.clear();
                db_account.account_state = AccountState::NotExisting;
                db_account.info = AccountInfo::default();
                continue;
            }
            let is_newly_created = account.is_created();
            self.insert_contract(&mut account.info);

            let db_account = self.accounts.entry(address).or_default();
            db_account.info = account.info;

            db_account.account_state = if is_newly_created {
                db_account.storage.clear();
                AccountState::StorageCleared
            } else if db_account.account_state.is_storage_cleared() {
                // Preserve old account state if it already exists
                AccountState::StorageCleared
            } else {
                AccountState::Touched
            };

            trace!(
                "Replacing storage for address {:?} <== {:?}",
                address, account.storage
            );

            db_account.storage.extend(
                account
                    .storage
                    .into_iter()
                    .map(|(key, value)| (key, value.present_value())),
            );
        }
    }
}
