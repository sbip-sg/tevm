use hashbrown::hash_map::Entry;
use hashbrown::{HashMap, HashSet};
use std::env;

use crate::cache::filesystem_cache::FileSystemProviderCache;
use crate::cache::ProviderCache;
use crate::fork_provider::ForkProvider;
use crate::instrument::bug::{BugData, Heuristics, InstrumentConfig};
use crate::CALL_DEPTH;
use ethers::types::{Block, TxHash};
use eyre::{ContextCompat, Result};
use primitive_types::H256;
use revm::db::{AccountState, DbAccount};
use revm::primitives::{
    keccak256, Account, AccountInfo, Address, Bytecode, HashMap as RevmHashMap, B256, KECCAK_EMPTY,
    U256,
};
use revm::{Database, DatabaseCommit};
use tracing::{debug, info, trace};

#[derive(Debug, Clone, Default)]
pub struct InstrumentData {
    pub bug_data: BugData,
    pub heuristics: Heuristics,
    // Mapping from contract address to a set of PCs seen in the execution
    pub pcs_by_address: HashMap<Address, HashSet<usize>>,
    // Holding the addresses created in the current transaction,
    // must be cleared by transaction caller before or after each transaction
    pub created_addresses: Vec<Address>,
    // Managed addresses: contract -> addresses created by any transaction from the contract
    pub managed_addresses: HashMap<Address, Vec<Address>>,
    pub opcode_index: usize,
    pub last_index_sub: usize,
    pub last_index_eq: usize,
}

#[derive(Debug)]
pub struct ForkDB<T: ProviderCache> {
    /// Account info where None means it is not existing. Not existing state is needed for Pre TANGERINE forks.
    /// `code` is always `None`, and bytecode can be found in `contracts`.
    pub accounts: HashMap<Address, DbAccount>,
    /// Tracks all contracts by their code hash.
    pub contracts: HashMap<B256, Bytecode>,
    /// All cached block hashes from the [DatabaseRef].
    pub block_hashes: HashMap<U256, B256>,

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
    block_cache: HashMap<u64, Block<TxHash>>,
    /// Max depth to consider when forking address
    max_fork_depth: usize,

    /// Optional instrument config
    pub instrument_config: Option<InstrumentConfig>,
    /// Instrument data collected
    pub instrument_data: InstrumentData,
}

impl Clone for ForkDB<FileSystemProviderCache> {
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
            instrument_config: self.instrument_config.clone(),
            instrument_data: self.instrument_data.clone(),
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

    fn get_fork_block_by_number(&mut self, number: u64) -> Result<Block<TxHash>> {
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
    pub fn get_fork_block(&mut self) -> Result<Block<TxHash>> {
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
            instrument_config: Some(InstrumentConfig::default()),
            instrument_data: InstrumentData::default(),
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
                account.code_hash = keccak256(code.bytecode());
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
    type Error = eyre::Error;
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
        let is_remote = !code.0.is_empty() || !balance.is_zero() || !nonce.is_zero();

        let info = AccountInfo::new(
            U256::from_limbs(balance.0),
            nonce.as_u64(),
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
        let index = H256::from(index.to_be_bytes());
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

        let value = U256::from_be_bytes(value.to_fixed_bytes());

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
    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        let unumber = number;
        if let Entry::Occupied(entry) = self.block_hashes.entry(number) {
            return Ok(*entry.get());
        }

        if !self.fork_enabled {
            return Ok(keccak256(number.to_be_bytes::<{ U256::BYTES }>()));
        }

        // saturate usize
        if number > U256::from(u64::MAX) {
            return Ok(KECCAK_EMPTY);
        }
        let number = u64::try_from(number).unwrap();

        let block = self.get_fork_block_by_number(number)?;

        let hash = block.hash.unwrap().0;
        let hash = B256::from_slice(&hash);
        self.block_hashes.insert(unumber, hash);
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
                address,
                account.storage
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
