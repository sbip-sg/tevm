use crate::{fork_provider::ForkProvider, response::RevmResult};
use ::revm::{
    db::DbAccount,
    primitives::{
        keccak256, AccountInfo, Address, Bytecode, CfgEnv, Env, ExecutionResult, HaltReason,
        TransactTo,
    },
    Evm,
};
use cache::DefaultProviderCache;
use chain_inspector::ChainInspector;
use dotenv::dotenv;
use ethers_providers::{Http, Provider};
use eyre::{eyre, ContextCompat, Result};
use fork_db::ForkDB;
use hashbrown::{HashMap, HashSet};
use lazy_static::lazy_static;
use num_bigint::BigInt;
use pyo3::prelude::*;
use response::{Response, SeenPcsMap, WrappedBug, WrappedHeuristics, WrappedMissedBranch};
use revm::inspector_handle_register;
use thread_local::ThreadLocal;
use tokio::runtime::Runtime;
use uuid::Uuid;

/// Caching for Web3 provider
mod cache;
mod chain_inspector;
/// Common functions shared by both EVMs
mod common;

// /// Create inspector for overriding address creation
// mod create_inspector;
/// Database for REVM
pub mod fork_db;
/// Cache for the fork requests
pub mod fork_provider;
pub mod instrument;
/// Provide response data structure from EVM
pub mod response;
pub use common::*;
use hex::ToHex;
use instrument::{
    bug_inspector::BugInspector, log_inspector::LogInspector, BugData, Heuristics, InstrumentConfig,
};
use ruint::aliases::U256;
use std::{cell::Cell, mem::replace, str::FromStr};
use tracing::{debug, info, trace};

lazy_static! {
    pub static ref CALL_DEPTH: ThreadLocal<Cell<usize>> = ThreadLocal::new();
}

/// Macro to define const string(s)
macro_rules! define_static_string {
    ($(($name:ident, $value: tt)),*) => {
        $(
            const $name: &str = $value;
        )*
    };
}

// Define some const strings used locally
define_static_string![
    (GAS_PRICE, "gas_price"),
    (ORIGIN, "origin"),
    (CHAIN_ID, "chain_id"),
    (BLOCK_NUMBER, "block_number"),
    (BLOCK_COINBASE, "block_coinbase"),
    (BLOCK_DIFFICULTY, "block_difficulty"),
    (BLOCK_TIMESTAMP, "block_timestamp"),
    (BLOCK_GAS_LIMIT, "block_gas_limit"),
    (BLOCK_BASE_FEE_PER_GAS, "block_base_fee_per_gas")
];

pub const DEFAULT_BALANCE: U256 =
    U256::from_limbs([0x0, 0xffffffffffffffff, 0xffffffffffffffff, 0x0]);

pub type TinyEvmDb = ForkDB<DefaultProviderCache>;

pub struct TinyEvmContext {}

/// TinyEVM is a Python wrapper for REVM
#[pyclass(unsendable)]
pub struct TinyEVM {
    /// REVM instance
    pub exe: Option<Evm<'static, ChainInspector, TinyEvmDb>>,
    pub owner: Address,
    /// Default gas limit for each transaction
    #[pyo3(get, set)]
    tx_gas_limit: u64,
    /// Snapshots of account state
    pub snapshots: HashMap<Address, DbAccount>,
    /// Optional fork url
    pub fork_url: Option<String>,
    /// Snapshot of global states
    global_snapshot: HashMap<Uuid, ForkDB<DefaultProviderCache>>,
}

static mut TRACE_ENABLED: bool = false;

/// Enable printing of trace logs for debugging
#[pyfunction]
pub fn enable_tracing() -> Result<()> {
    use tracing_subscriber::{fmt, EnvFilter};

    if unsafe { !TRACE_ENABLED } {
        let subscriber = fmt::Subscriber::builder()
            .with_env_filter(EnvFilter::from("tinyevm=trace,revm=trace"))
            .finish();

        // Set the subscriber as the global default.
        tracing::subscriber::set_global_default(subscriber)
            .expect("Setting default subscriber failed");

        unsafe {
            TRACE_ENABLED = true;
        }
    }

    Ok(())
}

// Implementations for use in Rust
impl TinyEVM {
    fn db(&self) -> &ForkDB<DefaultProviderCache> {
        &self.exe.as_ref().unwrap().context.evm.db
    }

    fn db_mut(&mut self) -> &mut ForkDB<DefaultProviderCache> {
        &mut self.exe.as_mut().unwrap().context.evm.db
    }

    pub fn instrument_config_mut(&mut self) -> &mut InstrumentConfig {
        &mut self.bug_inspector_mut().instrument_config
    }
    fn log_inspector(&self) -> &LogInspector {
        self.exe
            .as_ref()
            .unwrap()
            .context
            .external
            .log_inspector
            .as_ref()
            .unwrap()
    }

    fn log_inspector_mut(&mut self) -> &mut LogInspector {
        self.exe
            .as_mut()
            .unwrap()
            .context
            .external
            .log_inspector
            .as_mut()
            .unwrap()
    }

    fn bug_inspector(&self) -> &BugInspector {
        self.exe
            .as_ref()
            .unwrap()
            .context
            .external
            .bug_inspector
            .as_ref()
            .unwrap()
    }

    fn bug_inspector_mut(&mut self) -> &mut BugInspector {
        self.exe
            .as_mut()
            .unwrap()
            .context
            .external
            .bug_inspector
            .as_mut()
            .unwrap()
    }

    pub fn bug_data(&self) -> &BugData {
        &self.bug_inspector().bug_data
    }

    pub fn heuristics(&self) -> &Heuristics {
        &self.bug_inspector().heuristics
    }

    pub fn pcs_by_address(&self) -> &HashMap<Address, HashSet<usize>> {
        &self.bug_inspector().pcs_by_address
    }

    pub fn created_addresses(&self) -> &Vec<Address> {
        &self.bug_inspector().created_addresses
    }

    /// Create a new TinyEVM instance without fork
    pub fn new_offline() -> Result<Self> {
        Self::new_instance(None, None, false)
    }

    /// Set account balance, if the account does not exist, will create one
    pub fn set_account_balance(&mut self, address: Address, balance: U256) -> Result<()> {
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        if let Some(account) = db.accounts.get_mut(&address) {
            account.info.balance = balance;
        } else {
            let account = AccountInfo::from_balance(balance);
            db.insert_account_info(address, account);
        }
        Ok(())
    }

    /// Reset the account info
    pub fn reset_account(&mut self, addr: Address) -> Result<()> {
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;

        if db.accounts.get(&addr).is_some() {
            let account = AccountInfo {
                balance: DEFAULT_BALANCE,
                ..AccountInfo::default()
            };
            db.insert_account_info(addr, account);
        }

        Ok(())
    }

    /// Reset an account storage keeping the account info
    pub fn reset_storage(&mut self, addr: Address) -> Result<()> {
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        db.replace_account_storage(addr, Default::default())?;
        Ok(())
    }

    /// Reset both the accoun info and storage by address
    pub fn nuke_account(&mut self, addr: Address) -> Result<()> {
        info!("Nuke account: {:?}", addr);
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        db.accounts.remove(&addr);

        let managed_addresses = &mut self.bug_inspector_mut().managed_addresses;
        managed_addresses.remove(&addr);

        Ok(())
    }

    /// Deploy the contract for the `owner`.
    pub fn deploy_helper(
        &mut self,
        owner: Address,
        contract_bytecode: Vec<u8>,
        value: U256,
        tx_gas_limit: Option<u64>,
        force_address: Option<Address>, // not supported yet
    ) -> Result<Response> {
        trace!(
            "deploy_helper: {:?}, {:?}, {:?}",
            contract_bytecode.encode_hex::<String>(),
            owner,
            value,
        );

        CALL_DEPTH.get_or_default().set(0);

        // Reset instrumentation,
        self.clear_instrumentation();

        self.bug_inspector_mut().pcs_by_address.clear(); // If don't want to trace the deploy PCs

        {
            let tx = self.exe.as_mut().unwrap().tx_mut();
            tx.caller = owner;
            tx.transact_to = TransactTo::Create;
            tx.data = contract_bytecode.clone().into();
            tx.value = value;
            tx.gas_limit = tx_gas_limit.unwrap_or(self.tx_gas_limit);
        }

        // todo_cl this is read from global state, might be wrong
        let nonce = self
            .exe
            .as_ref()
            .unwrap()
            .context
            .evm
            .db
            .accounts
            .get(&owner)
            .map_or(0, |a| a.info.nonce);
        let address = owner.create(nonce);

        debug!("Calculated addresss: {:?}", address);

        if let Some(force_address) = force_address {
            self.bug_inspector_mut()
                .create_address_overrides
                .insert(address, force_address);
        }
        let result = self.exe.as_mut().unwrap().transact_commit();

        // debug!("db {:?}", self.exe.as_ref().unwrap().db());
        // debug!("sender {:?}", owner.encode_hex::<String>(),);

        // // todo_cl temp check
        // self.db = self.exe.as_ref().unwrap().db().clone();
        // self.env = self.exe.as_ref().unwrap().context.evm.env.as_ref().clone();

        trace!("deploy result: {:?}", result);

        let collision = {
            if let Ok(ref result) = result {
                matches!(
                    result,
                    ExecutionResult::Halt {
                        reason: HaltReason::CreateCollision,
                        ..
                    }
                )
            } else {
                false
            }
        };

        if collision {
            return Err(eyre!(
                "Address collision for {}",
                address.encode_hex::<String>()
            ))?;
        }

        let bug_data = self.bug_data().clone();
        let heuristics = self.heuristics().clone();
        let seen_pcs = self.pcs_by_address().clone();
        let addresses = self.created_addresses().clone();
        info!(
            "created addresses from deployment: {:?} for calculated address {:?}",
            addresses, address
        );
        if !addresses.is_empty() {
            self.bug_inspector_mut()
                .managed_addresses
                .insert(address, addresses);
        }

        let logs = self.log_inspector().logs.clone();
        let traces = self.log_inspector().traces.clone();

        trace!("deploy result: {:?}", result);

        let revm_result = RevmResult {
            result: result.map_err(|e| eyre!(e)),
            bug_data,
            heuristics,
            seen_pcs,
            traces,
            transient_logs: logs,
            ignored_addresses: Default::default(),
        };

        Ok(revm_result.into())
    }

    /// Send a `transact_call` to a `contract` from the `sender` with raw
    /// `data` and some ETH `value`.
    pub fn contract_call_helper(
        &mut self,
        contract: Address,
        sender: Address,
        data: Vec<u8>,
        value: U256,
        tx_gas_limit: Option<u64>,
    ) -> Response {
        // Reset instrumentation,
        self.clear_instrumentation();
        CALL_DEPTH.get_or_default().set(0);

        {
            let tx = self.exe.as_mut().unwrap().tx_mut();
            tx.caller = sender;
            tx.transact_to = TransactTo::Call(contract);
            tx.data = data.into();
            tx.value = value;
            tx.gas_limit = tx_gas_limit.unwrap_or(TX_GAS_LIMIT);
        }

        let result = {
            let exe = self.exe.as_mut().unwrap();
            exe.transact_commit()
        };

        let addresses = self.created_addresses().clone();
        info!(
            "created addresses from contract call: {:?} for {:?}",
            addresses, contract
        );

        debug!("contract_call result: {:?}", result);

        if !addresses.is_empty() {
            self.bug_inspector_mut()
                .managed_addresses
                .insert(contract, addresses);
        }

        let bug_data = self.bug_data().clone();
        let heuristics = self.heuristics().clone();
        let seen_pcs = self.pcs_by_address().clone();

        let db = &self.exe.as_ref().unwrap().context.evm.db;
        let ignored_addresses = db.ignored_addresses.clone();
        let ignored_addresses = ignored_addresses.into_iter().map(Into::into).collect();

        let log_inspector = self.log_inspector();
        let logs = log_inspector.logs.clone();
        let traces = log_inspector.traces.clone();

        let revm_result = RevmResult {
            result: result.map_err(|e| eyre!(e)),
            bug_data,
            heuristics,
            seen_pcs,
            traces,
            transient_logs: logs,
            ignored_addresses,
        };
        revm_result.into()
    }

    /// Set code of an account
    pub fn set_code_by_address(&mut self, addr: Address, code: Vec<u8>) -> Result<()> {
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        let code = Bytecode::new_raw(code.into());
        let accounts = &db.accounts;

        if let Some(account) = accounts.get(&addr) {
            debug!("Set code for existing account");
            let code_hash = keccak256(code.bytecode());
            let code = Some(code);

            let account = AccountInfo {
                code,
                code_hash,
                ..account.info
            };

            db.insert_account_info(addr, account);
        } else {
            debug!("Set code for new account");
            let code_hash = keccak256(code.bytecode());
            let code = Some(code);

            let account = AccountInfo {
                balance: U256::from(1_000_000_000_000u64),
                code,
                code_hash,
                ..Default::default()
            };

            db.insert_account_info(addr, account);
        }

        Ok(())
    }

    /// Get code from an address
    pub fn get_code_by_address(&self, addr: Address) -> Result<Vec<u8>> {
        let db = &self.exe.as_ref().unwrap().context.evm.db;
        let accounts = &db.accounts;
        let account = accounts.get(&addr);
        if let Some(account) = account {
            let code = &account.info.code;
            if let Some(code) = code {
                return Ok(code.bytecode().to_vec());
            }
        }

        Ok(vec![])
    }

    /// Get Eth balance for an account
    pub fn get_eth_balance(&self, addr: Address) -> Result<U256> {
        let db = &self.exe.as_ref().unwrap().context.evm.db;
        let accounts = &db.accounts;
        if let Some(account) = accounts.get(&addr) {
            Ok(account.info.balance)
        } else {
            Ok(U256::ZERO)
        }
    }

    /// Get storage by address and index
    pub fn get_storage_by_address(&self, addr: Address, index: U256) -> Result<U256> {
        let db = &self.exe.as_ref().unwrap().context.evm.db;
        let accounts = &db.accounts;
        let account = accounts
            .get(&addr)
            .context(format!("Failed to get account for address: {:?}", addr))?;
        account
            .storage
            .get(&index)
            .map_or_else(|| Ok(U256::default()), |v| Ok(*v))
    }

    /// Set storage by address and index
    pub fn set_storage_by_address(
        &mut self,
        addr: Address,
        index: U256,
        value: U256,
    ) -> Result<()> {
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        db.insert_account_storage(addr, index, value)?;
        Ok(())
    }

    /// Clone account from one address to another. If `delete` is true, the original account will be deleted.
    pub fn clone_account(&mut self, from: Address, to: Address, delete: bool) -> Result<()> {
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        let accounts = &db.accounts;
        let account = accounts.get(&from).cloned();

        if let Some(account) = account {
            db.accounts.insert(to, account);
            if delete {
                db.accounts.remove(&from);
            }
        }

        Ok(())
    }

    pub fn new_instance(
        fork_url: Option<String>,
        block_id: Option<u64>,
        enable_call_trace: bool, // Whether to show call and event traces
    ) -> Result<Self> {
        dotenv().ok();
        let owner = Address::default();

        // Create a new REVM instance with default configurations

        let mut cfg_env = CfgEnv::default();
        cfg_env.disable_eip3607 = true;
        cfg_env.disable_block_gas_limit = true;

        let fork_enabled = fork_url.is_some();

        // let mut db = InMemoryDB::default();
        let mut db = match fork_url {
            Some(ref url) => {
                info!("Starting EVM from fork {} and block: {:?}", url, block_id);
                let runtime = Runtime::new().expect("Create runtime failed");
                let provider = Provider::<Http>::try_from(url)?;
                let provider = ForkProvider::new(provider, runtime);
                ForkDB::create_with_provider(Some(provider), block_id)
            }
            None => ForkDB::create(),
        };

        let mut env = Env {
            cfg: cfg_env,
            ..Default::default()
        };

        if fork_enabled {
            let block = db.get_fork_block().unwrap();
            let block_number = block.number.expect("Failed to get block number").as_u64();
            info!("Using block number: {:?}", block_number);

            env.block.number = U256::from(block_number);
            env.block.timestamp = U256::from_limbs(block.timestamp.0);
            env.block.difficulty = U256::from_limbs(block.difficulty.0);
            env.block.gas_limit = U256::from_limbs(block.gas_limit.0);
            env.cfg.disable_base_fee = true;
            if let Some(base_fee) = block.base_fee_per_gas {
                env.block.basefee = U256::from_limbs(base_fee.0);
            }
            if let Some(coinbase) = block.author {
                env.block.coinbase = Address::from(coinbase.0);
            }
        }

        // NOTE: Possibly load other necessary configuration from remote

        // Add owner account
        let account = AccountInfo {
            balance: DEFAULT_BALANCE,
            ..Default::default()
        };

        db.insert_account_info(owner, account);
        // let mut builder = Evm::builder();
        let log_inspector = LogInspector {
            trace_enabled: enable_call_trace,
            ..LogInspector::default()
        };

        let bug_inspector = BugInspector::default();

        let inspector = ChainInspector {
            log_inspector: Some(log_inspector),
            bug_inspector: Some(bug_inspector),
        };

        // builder = builder.with_external_context(inspector);

        let exe = Evm::builder()
            .modify_env(|e| *e = Box::new(env.clone()))
            .with_db(db.clone())
            .with_external_context(inspector)
            .append_handler_register(inspector_handle_register)
            .build();
        let tinyevm = Self {
            exe: Some(exe),
            owner,
            fork_url,
            tx_gas_limit: TX_GAS_LIMIT,
            snapshots: HashMap::with_capacity(32),
            global_snapshot: Default::default(),
        };

        Ok(tinyevm)
    }
}

impl Default for TinyEVM {
    fn default() -> Self {
        Self::new_instance(None, None, false).unwrap()
    }
}

// Implementations for use in Python and Rust
#[pymethods]
impl TinyEVM {
    /// Create a new TinyEVM instance
    #[new]
    #[pyo3(signature = (fork_url = None, block_id = None))]
    pub fn new(fork_url: Option<String>, block_id: Option<u64>) -> Result<Self> {
        Self::new_instance(fork_url, block_id, false)
    }

    /// Get addresses loaded remotely as string
    pub fn get_forked_addresses(&self) -> Result<Vec<String>> {
        let db = &self.exe.as_ref().unwrap().context.evm.db;
        let addresses = &db.remote_addresses;
        addresses.keys().map(|a| Ok(format!("0x{:x}", a))).collect()
    }

    /// Get remotely loaded slot indices by address
    pub fn get_forked_slots(&self, address: String) -> Result<Vec<BigInt>> {
        let address = Address::from_str(&address)?;
        let db = &self.exe.as_ref().unwrap().context.evm.db;
        db.remote_addresses.get(&address).map_or_else(
            || Ok(vec![]),
            |slots| Ok(slots.iter().map(ruint_u256_to_bigint).collect::<Vec<_>>()),
        )
    }

    /// Toggle for enable mode, only makes sense when fork_url is set
    pub fn toggle_enable_fork(&mut self, enabled: bool) {
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        db.fork_enabled = enabled;
    }

    /// Set whether to log the traces of the EVM execution
    pub fn set_evm_tracing(&mut self, enabled: bool) {
        let log_inspector = self.log_inspector_mut();
        log_inspector.trace_enabled = enabled;
    }

    /// Get the current fork toggle status
    pub fn is_fork_enabled(&self) -> bool {
        let db = &self.exe.as_ref().unwrap().context.evm.db;
        db.fork_enabled
    }

    /// Deploy a contract using contract deploy binary
    ///
    /// - `contract_deploy_code`: contract deploy binary array encoded as hex string
    /// - `owner`: owner address as a 20-byte array encoded as hex string
    #[pyo3(signature = (contract_deploy_code, owner=None))]
    pub fn deploy(
        &mut self,
        contract_deploy_code: String,
        owner: Option<String>,
    ) -> Result<Response> {
        let owner = owner
            .map(|address| Address::from_str(&address))
            .unwrap_or(Ok(self.owner))?;
        self.deploy_helper(
            // Address::from_str(&owner.unwrap_or_default())?,
            owner,
            hex::decode(contract_deploy_code)?,
            U256::default(),
            None,
            None,
        )
    }

    /// Deploy a contract using contract deploy binary If the account already
    /// exists in the executor, the nonce and code of the account will be
    /// **overwritten**.
    ///
    /// For optional arguments, you can use the empty string as inputs to use the default values.
    ///
    /// - `contract_deploy_code`: contract deploy binary array encoded as hex string
    /// - `owner`: Owner address as a 20-byte array encoded as hex string
    /// - `data`: (Optional, default empty) Constructor arguments encoded as hex string.
    /// - `value`: (Optional, default 0) a U256. Set the value to be included in the contract creation transaction.
    /// - `deploy_to_address`: when provided, change the address of the deployed contract to this address

    ///   - This requires the constructor to be payable.
    ///   - The transaction sender (owner) must have enough balance
    /// - `init_value`: (Optional) BigInt. Override the initial balance of the contract to this value.
    ///
    /// Returns a list consisting of 4 items `[reason, address-as-byte-array, bug_data, heuristics]`
    #[pyo3(signature = (contract_deploy_code, owner=None, data=None, value=None, init_value=None, deploy_to_address=None))]
    pub fn deterministic_deploy(
        &mut self,
        contract_deploy_code: String, // variable length
        owner: Option<String>,        // h160 as hex string
        data: Option<String>,         // variable length
        value: Option<BigInt>,
        init_value: Option<BigInt>,
        deploy_to_address: Option<String>,
    ) -> Result<Response> {
        let owner = {
            if let Some(owner) = owner {
                let owner = &owner;
                Address::from_str(trim_prefix(owner, "0x"))?
            } else {
                self.owner
            }
        };

        let contract_deploy_code = hex::decode(contract_deploy_code)?;
        let data = {
            if let Some(data) = data {
                hex::decode(data)?
            } else {
                vec![]
            }
        };
        let value = value.unwrap_or_default();
        let mut contract_bytecode = contract_deploy_code.to_vec();
        contract_bytecode.extend(data);
        let force_address: Option<Address> = deploy_to_address
            .map(|s| Address::from_str(&s))
            .transpose()?;

        let resp = {
            let resp = self.deploy_helper(
                owner,
                contract_bytecode,
                bigint_to_ruint_u256(&value)?,
                None,
                force_address,
            )?;

            if let Some(balance) = init_value {
                let address = Address::from_slice(&resp.data);
                self.set_account_balance(address, bigint_to_ruint_u256(&balance)?)?;
            }

            resp
        };

        Ok(resp)
    }

    /// - `contract` null ended c string of contract address encoded as hex
    /// - `sender` null ended c string of sender address (20 bytes) encoded as hex
    /// - `data` null ended c string of encoded contract method plus parameters
    /// - `value` value send in the transaction, U256 as hex
    ///
    /// Returns c string of Json encoded response consists of a list of four elements:
    /// `[reason, data, bug_data, heuristics]`
    #[pyo3(signature = (contract, sender=None, data=None, value=None))]
    pub fn contract_call(
        &mut self,
        contract: String,
        sender: Option<String>,
        data: Option<String>,
        value: Option<BigInt>,
    ) -> Result<Response> {
        let sender = {
            if let Some(sender) = sender {
                let sender = &sender;
                Address::from_str(trim_prefix(sender, "0x"))?
            } else {
                self.owner
            }
        };

        let contract = {
            let contract = &contract;
            Address::from_str(trim_prefix(contract, "0x"))?
        };

        let data = {
            if let Some(data) = data {
                hex::decode(data)?
            } else {
                vec![]
            }
        };
        let value = value.unwrap_or_default();
        let value = bigint_to_ruint_u256(&value)?;
        debug!(
            "contract_call: contract {} sender {} data {} value {}",
            contract,
            sender,
            data.encode_hex::<String>(),
            value
        );

        let resp = self.contract_call_helper(contract, sender, data, value, None);

        Ok(resp)
    }

    /// Reset EVM state
    pub fn reset(&mut self) -> Result<()> {
        self.owner = Address::ZERO;
        // TODO reset db and env

        // let fork_enabled = self.exe.context.evm.db.fork_enabled;
        // TODO clear all data
        // let mut exe = revm::make_executor_with_fork(
        //     Some(self.owner.into()),
        //     self.fork_url.clone(),
        //     self.block_id,
        // )?;
        // self.exe.context.evm.db.fork_enabled = fork_enabled;
        // self.exe = exe;
        Ok(())
    }

    /// Return account's balance in wei
    pub fn get_balance(&self, addr: String) -> Result<BigInt> {
        let addr = Address::from_str(trim_prefix(&addr, "0x"))?;

        let balance = self.get_eth_balance(addr)?;
        let balance = ruint_u256_to_bigint(&balance);

        Ok(balance)
    }

    /// Set account's balance
    pub fn set_balance(&mut self, addr: String, balance: BigInt) -> Result<()> {
        let addr = Address::from_str(trim_prefix(&addr, "0x"))?;

        let balance = bigint_to_ruint_u256(&balance)?;

        self.set_account_balance(addr, balance)
    }

    /// Get account's code
    pub fn get_code(&self, addr: String) -> Result<String> {
        let addr = Address::from_str(&addr)?;

        let code: String = self.get_code_by_address(addr)?.encode_hex();
        Ok(code)
    }

    /// Set account's code (runtime-binary). Will create the account
    /// if it does not exist
    pub fn set_code(&mut self, addr: String, data: String) -> Result<()> {
        let addr = Address::from_str(&addr)?;

        let data = hex::decode(data)?;
        self.set_code_by_address(addr, data)?;
        Ok(())
    }

    /// Set a vicinity value by field name and reset the EVM executor. You
    /// may call this function multiple times to set multiple fields.
    ///
    /// Supported fields:
    ///
    /// - `gas_price`: U256 as hex string
    /// - `origin`: H160 as hex string
    /// - `chain_id`: U256 as hex string
    /// - `block_number`: U256 as hex string
    /// - `block_coinbase`: H160 as hex string
    /// - `block_timestamp`: U256 as hex string
    /// - `block_difficulty`: U256 as hex string
    /// - `block_gas_limit`: U256 as hex string
    /// - `block_base_fee_per_gas`: U256 as hex string
    /// - `block_hashes`: not supported
    pub fn get_env_value_by_field(&self, field: String) -> Result<String> {
        let exe = &self.exe.as_ref().unwrap();
        macro_rules! hex2str {
            ($val:expr) => {
                serde_json::to_string(&$val).unwrap()
            };
        }

        let r = match field.as_str() {
            // NOTE returning BigInt instead of hex string might be a better idea
            GAS_PRICE => hex2str!(exe.tx().gas_price),
            CHAIN_ID => hex2str!(exe.cfg().chain_id),
            BLOCK_NUMBER => hex2str!(exe.block().number),
            BLOCK_TIMESTAMP => hex2str!(exe.block().timestamp),
            BLOCK_DIFFICULTY => hex2str!(exe.block().difficulty),
            BLOCK_GAS_LIMIT => hex2str!(exe.block().gas_limit),
            BLOCK_BASE_FEE_PER_GAS => hex2str!(exe.block().basefee),
            ORIGIN => format!("0x{}", hex::encode(exe.tx().caller)),
            BLOCK_COINBASE => format!("0x{}", hex::encode(exe.block().coinbase)),
            _ => return Err(eyre!("Unknown field: {}", &field)),
        };
        Ok(r)
    }

    /// Set a vicinity value by field name and reset the EVM executor. You
    /// may call this function multiple times to set multiple fields.
    ///
    /// Supported fields:
    ///
    /// - `gas_price`: U256 as hex string
    /// - `origin`: H160 as hex string
    /// - `chain_id`: U256 as hex string
    /// - `block_number`: U256 as hex string
    /// - `block_coinbase`: H160 as hex string
    /// - `block_timestamp`: U256 as hex string
    /// - `block_difficulty`: U256 as hex string
    /// - `block_gas_limit`: U256 as hex string
    /// - `block_base_fee_per_gas`: U256 as hex string
    /// - `block_hashes`: not supported
    pub fn set_env_field_value(&mut self, field: String, value: String) -> Result<()> {
        self.set_env_field_value_inner(&field, &value)
    }

    /// Configure runtime instrumentation options
    /// Supported fields:
    ///
    /// - `config`: A json string serialized for [`InstrumentConfig`](https://github.com/sbip-sg/revm/blob/6f7ac687a22f67462999ca132ede8d116bd7feb9/crates/revm/src/bug.rs#L153)
    pub fn configure(&mut self, config: &REVMConfig) -> Result<()> {
        let config = config.to_iconfig()?;
        self.bug_inspector_mut().instrument_config = config;
        Ok(())
    }

    /// Get current runtime instrumentation configuration
    pub fn get_instrument_config(&self) -> Result<REVMConfig> {
        let r = &self.bug_inspector().instrument_config;
        Ok(REVMConfig::from(r))
    }

    /// Set EVM env field value. Value is hex encoded string
    pub fn set_env_field_value_inner(&mut self, field: &str, value: &str) -> Result<()> {
        debug!("set_env_field_value_inner: {} {}", field, value);

        let value = trim_prefix(value, "0x");

        let to_u256 = |v: &str| U256::from_str_radix(v, 16);
        let to_address = |v: &str| Address::from_str(v);

        macro_rules! set_env_field {
            ($field:ident, $value:expr, $env:ident, $method:ident) => {{
                let env = &mut self.exe.as_mut().unwrap().$env();
                env.$field = $method($value)?;
            }};
        }
        match field {
            CHAIN_ID => {
                let cfg = &mut self.exe.as_mut().unwrap().cfg_mut();
                cfg.chain_id = u64::from_str_radix(value, 16)?;
            }
            GAS_PRICE => set_env_field!(gas_price, value, tx_mut, to_u256),
            ORIGIN => set_env_field!(caller, value, tx_mut, to_address),
            BLOCK_NUMBER => set_env_field!(number, value, block_mut, to_u256),
            BLOCK_TIMESTAMP => set_env_field!(timestamp, value, block_mut, to_u256),
            BLOCK_DIFFICULTY => set_env_field!(difficulty, value, block_mut, to_u256),
            BLOCK_GAS_LIMIT => set_env_field!(gas_limit, value, block_mut, to_u256),
            BLOCK_BASE_FEE_PER_GAS => set_env_field!(basefee, value, block_mut, to_u256),
            BLOCK_COINBASE => set_env_field!(coinbase, value, block_mut, to_address),
            _ => return Err(eyre!("Unknown field: {}", &field))?,
        }

        Ok(())
    }

    /// API to set tx origin, after this method call, tx.origin will always return the set address.
    /// This function should be called after EVM executor is created.
    pub fn set_tx_origin(&mut self, address: String) -> Result<()> {
        let address = &address;
        self.set_env_field_value_inner(ORIGIN, address)
    }

    /// API to get the owner (default sender) address
    pub fn get_owner(&self) -> Result<String> {
        Ok(format!("{:#066x}", self.owner))
    }

    /// Set the owner (default sender) address
    pub fn set_owner(&mut self, owner: String) -> Result<()> {
        let owner = &owner;
        let owner = Address::from_str(trim_prefix(owner, "0x"))?;
        self.owner = owner;
        Ok(())
    }

    // /// Get current env from EVM executor as JSON string
    // pub fn get_env(&mut self) -> Result<String> {
    //     let env = self.exe.env.clone();
    // serde json has been removed
    //     let s = serde_json::to_string(&env)?;
    //     Ok(s)
    // }

    /// Set account's storage by index
    ///
    /// - `addr`: H160 address as hex string
    /// - `index`: H256 as hex string
    /// - `value`: H256 as hex string
    pub fn set_storage(
        &mut self,
        addr: String,  // address as H160, encoded as hex
        index: String, // index as H256, encoded as hex
        value: String, // value as H256, encoded as hex
    ) -> Result<()> {
        let addr = &addr;
        let index = &index;
        let value = &value;
        let addr = Address::from_str(trim_prefix(addr, "0x"))?;
        let value = U256::from_str_radix(trim_prefix(value, "0x"), 16)?;
        let index = U256::from_str_radix(trim_prefix(index, "0x"), 16)?;

        self.set_storage_by_address(addr, index, value)
    }

    /// Get account's storage by index
    ///
    /// - `addr`: H160 address as hex string
    /// - `index`: H256 as hex string
    ///
    /// Returns H256 as hex string
    pub fn get_storage(
        &self,
        addr: String,  // address as H160, encoded as hex
        index: String, // index as H256, encoded as hex
    ) -> Result<BigInt> {
        let addr = Address::from_str(trim_prefix(&addr, "0x"))?;

        let index = &index;
        let index = U256::from_str_radix(trim_prefix(index, "0x"), 16)?;

        let s = self.get_storage_by_address(addr, index)?;

        Ok(ruint_u256_to_bigint(&s))
    }

    /// Reset storage by account
    pub fn reset_storage_by_account(&mut self, addr: String) -> Result<()> {
        let addr = Address::from_str(&addr)?;
        self.reset_storage(addr)
    }

    /// Remove account
    pub fn remove_account(
        &mut self,
        addr: String, // address as H160, encoded as hex
    ) -> Result<()> {
        let addr = Address::from_str(&addr)?;
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        db.accounts.remove(&addr);
        Ok(())
    }

    /// Take a snapshot of an account, raise error if account does not exist in db
    pub fn take_snapshot(&mut self, address: String) -> Result<()> {
        let addr = Address::from_str(&address)?;
        let db = &self.exe.as_ref().unwrap().context.evm.db;
        if let Some(account) = db.accounts.get(&addr) {
            self.snapshots.insert(addr, account.clone());
            Ok(())
        } else {
            Err(eyre!("Account not found"))
        }
    }

    /// Copy an account from snapshot to another address, the target address will
    /// be overridden. Raise error if account to be copied from does not exist in
    /// db
    pub fn copy_snapshot(&mut self, from: String, to: String) -> Result<()> {
        let from = Address::from_str(&from)?;
        let to = Address::from_str(&to)?;

        let db = &mut self.exe.as_mut().unwrap().context.evm.db;

        let account = self
            .snapshots
            .get(&from)
            .context("No snapshot found")?
            .clone();
        db.accounts.insert(to, account);
        Ok(())
    }

    pub fn clear_instrumentation(&mut self) {
        let bug_inspector = self.bug_inspector_mut();
        bug_inspector.bug_data.clear();
        bug_inspector.created_addresses.clear();
        bug_inspector.heuristics = Default::default();
    }

    /// Restore a snapshot for an account, raise error if there is no snapshot for the account
    pub fn restore_snapshot(&mut self, address: String) -> Result<()> {
        let addr = Address::from_str(&address)?;
        let db = &mut self.exe.as_mut().unwrap().context.evm.db;
        let account = self.snapshots.get(&addr).context("No snapshot found")?;

        db.accounts.insert(addr, account.clone());
        Ok(())
    }

    /// Take global snapshot of all accounts
    pub fn take_global_snapshot(&mut self) -> Result<String> {
        let db = self.db();
        let snapshot = db.clone();
        let id = Uuid::new_v4();
        self.global_snapshot.insert(id, snapshot);
        Ok(id.to_string())
    }

    pub fn restore_global_snapshot(
        &mut self,
        snapshot_id: String,
        keep_snapshot: bool,
    ) -> Result<()> {
        let id = Uuid::parse_str(&snapshot_id)?;

        if keep_snapshot {
            let snapshot = self.global_snapshot.get(&id).context("No snapshot found")?;
            *self.db_mut() = snapshot.clone();
        } else {
            let snapshot = self
                .global_snapshot
                .remove(&id)
                .context("No snapshot found")?;
            let _ = replace(self.db_mut(), snapshot);
        }

        Ok(())
    }
}

/// Configuration class for instrumentation, this is a wrapper for
/// REVM::InstrumentConfig
#[pyclass(set_all, get_all)]
pub struct REVMConfig {
    /// Enable recording seen PCs by current contract address
    pub pcs_by_address: bool,
    /// Enable heuristics which will record list of jumpi destinations
    pub heuristics: bool,
    /// Recored missed branches for target contract address only. If
    /// this option is true, `env.heuristics.coverage` and
    /// `env.heuristics.missed_branchs` will be recorded only when the
    /// current contract address equals the `target_address`
    pub record_branch_for_target_only: bool,
    /// Only when `record_branch_for_target_only` is `true`: the
    /// target contract address set by the API caller
    pub target_address: Option<String>,
    /// Whether to record SHA3 mappings
    pub record_sha3_mapping: bool,
    /// The block id to fork
    pub fork_block_id: Option<String>,
    /// The endpoints to use
    pub fork_endpoints: Vec<String>,
    /// The network id to fork
    pub fork_network_id: Option<String>,
}

#[pymethods]
impl REVMConfig {
    /// Create a new REVMConfig instance with same default settings as REVM::InstrumentConfig
    #[new]
    pub fn new() -> Self {
        let config = InstrumentConfig::default();
        Self::from(&config)
    }
}

impl REVMConfig {
    /// Convert from `InstrumentConfig`
    fn to_iconfig(&self) -> Result<InstrumentConfig> {
        let target_address = if let Some(addr) = &self.target_address {
            let addr = trim_prefix(addr, "0x");
            Address::from_str(addr)?
        } else {
            Address::default()
        };

        Ok(InstrumentConfig {
            target_address,
            pcs_by_address: self.pcs_by_address,
            heuristics: self.heuristics,
            record_branch_for_target_only: self.record_branch_for_target_only,
            record_sha3_mapping: self.record_sha3_mapping,
        })
    }

    /// Convert to `REVMConfig` from internal Rust struct
    fn from(config: &InstrumentConfig) -> Self {
        Self {
            pcs_by_address: config.pcs_by_address,
            heuristics: config.heuristics,
            record_branch_for_target_only: config.record_branch_for_target_only,
            target_address: Some(format!("{:#066x}", config.target_address)),
            record_sha3_mapping: config.record_sha3_mapping,
            fork_block_id: None,
            fork_endpoints: vec![],
            fork_network_id: None,
        }
    }
}

impl Default for REVMConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The Python module we provide
#[pymodule]
fn tinyevm(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(enable_tracing, m)?)?;
    m.add_class::<TinyEVM>()?;
    m.add_class::<Response>()?;
    m.add_class::<WrappedBug>()?;
    m.add_class::<WrappedMissedBranch>()?;
    m.add_class::<WrappedHeuristics>()?;
    m.add_class::<SeenPcsMap>()?;
    m.add_class::<REVMConfig>()?;
    Ok(())
}
