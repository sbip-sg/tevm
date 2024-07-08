use crate::{fork_provider::ForkProvider, logs::LogsInspector, response::RevmResult};
use ::revm::{
    db::DbAccount,
    primitives::{
        keccak256, AccountInfo, Address, Bytecode, CfgEnv, Env, ExecutionResult, HaltReason,
        SpecId, TransactTo, B256,
    },
    Evm,
};
use hashbrown::{HashMap, HashSet};
// TODO use feature to load only one of them
use cache::{filesystem_cache::FileSystemProviderCache, redis_cache::RedisProviderCache};
use dotenv::dotenv;
use ethers_providers::{Http, Provider};
use eyre::{eyre, ContextCompat, Result};
use fork_db::{ForkDB, InstrumentData};
use lazy_static::lazy_static;
use num_bigint::BigInt;
use pyo3::prelude::*;
use response::{Response, SeenPcsMap, WrappedBug, WrappedHeuristics, WrappedMissedBranch};
use thread_local::ThreadLocal;
use tokio::runtime::Runtime;
/// Caching for Web3 provider
mod cache;
/// Common functions shared by both EVMs
mod common;

// /// Create inspector for overriding address creation
// mod create_inspector;
/// Database for REVM
pub mod fork_db;
/// Cache for the fork requests
pub mod fork_provider;
pub mod instrument;
/// Logging
mod logs;
/// Provide response data structure from EVM
pub mod response;
pub use common::*;
use hex::ToHex;
use instrument::{BugData, Heuristics, InstrumentConfig};
use ruint::aliases::U256;
use std::{cell::Cell, env, str::FromStr};
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

/// Name of field in env for configuring `SpecId`
const SPEC: &str = "spec_name";

pub const DEFAULT_BALANCE: U256 =
    U256::from_limbs([0x0, 0xffffffffffffffff, 0xffffffffffffffff, 0x0]);

pub type FileSystemTinyEvmDb = ForkDB<FileSystemProviderCache>;

pub type RedisTinyEvmDb = ForkDB<RedisProviderCache>;

pub struct TinyEvmContext {}

/// TinyEVM is a Python wrapper for REVM
#[pyclass(unsendable)]
pub struct TinyEVM {
    /// REVM instance
    pub exe: Option<Evm<'static, (), ForkDB<FileSystemProviderCache>>>,
    /// Default sender address
    pub db: ForkDB<FileSystemProviderCache>,
    /// EVM env which contains the config for the EVM, the block as well as the transaction
    pub env: Env,
    pub owner: Address,
    /// Default gas limit for each transaction
    pub tx_gas_limit: u64,
    // Default gas limit for each block
    // default_block_gas_limit: u64,
    /// Snapshots of account state
    pub snapshots: HashMap<Address, DbAccount>,
    /// Optional fork url
    pub fork_url: Option<String>,
    /// Optional block id
    pub block_id: Option<u64>,
    /// Optional specId used, otherwise use the REVM default
    pub spec_id: Option<SpecId>,
}

static mut TRACE_ENABLED: bool = false;

/// Enable global tracing
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
    pub fn instrument_data(&self) -> &InstrumentData {
        &self.db.instrument_data
    }

    pub fn bug_data(&self) -> &BugData {
        &self.instrument_data().bug_data
    }

    pub fn heuristics(&self) -> &Heuristics {
        &self.instrument_data().heuristics
    }

    pub fn pcs_by_address(&self) -> &HashMap<Address, HashSet<usize>> {
        &self.instrument_data().pcs_by_address
    }

    pub fn created_addresses(&self) -> &Vec<Address> {
        &self.instrument_data().created_addresses
    }

    /// Create a new TinyEVM instance without fork
    pub fn new_offline() -> Result<Self> {
        Self::new(None, None)
    }

    /// Set account balance, if the account does not exist, will create one
    pub fn set_account_balance(&mut self, address: Address, balance: U256) -> Result<()> {
        let db = &mut self.db;
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
        let db = &mut self.db;

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
        let db = &mut self.db;
        db.replace_account_storage(addr, Default::default())?;
        Ok(())
    }

    /// Reset both the accoun info and storage by address
    pub fn nuke_account(&mut self, addr: Address) -> Result<()> {
        info!("Nuke account: {:?}", addr);
        self.reset_account(addr)?;
        self.reset_storage(addr)?;

        let managed_addresses = &self.db.instrument_data.managed_addresses;

        let addrs = managed_addresses.get(&addr);

        if let Some(created_addrs) = addrs {
            for addr in created_addrs.clone() {
                self.nuke_account(addr)?;
            }
        }

        self.db.instrument_data.managed_addresses.remove(&addr);

        Ok(())
    }

    /// Deploy the contract for the `owner`.
    pub fn deploy_helper(
        &mut self,
        salt: U256,
        owner: Address,
        contract_bytecode: Vec<u8>,
        value: U256,
        overwrite: bool,
        tx_gas_limit: Option<u64>,
        force_address: Option<Address>,
    ) -> Result<Response> {
        trace!(
            "deploy_helper: {:?}, {:?}, {:?}, {:?}",
            contract_bytecode.encode_hex::<String>(),
            salt,
            owner,
            value,
        );

        CALL_DEPTH.get_or_default().set(0);

        // Reset instrumentation,
        self.clear_instrumentation();

        self.db.instrument_data.pcs_by_address.clear(); // If don't want to trace the deploy PCs

        if let Some(exe) = self.exe.take() {
            let exe = exe
                .modify()
                .modify_tx_env(|tx| {
                    tx.caller = owner;
                    tx.transact_to = TransactTo::Create;
                    tx.data = contract_bytecode.clone().into();
                    tx.value = value;
                    tx.gas_limit = tx_gas_limit.unwrap_or(TX_GAS_LIMIT);
                })
                .modify_cfg_env(|env| env.limit_contract_code_size = Some(0x24000))
                .build();
            self.exe = Some(exe);
        }

        let code_hash = keccak256(&contract_bytecode);
        let address = owner.create2(B256::from(salt), code_hash);

        // TODO Could check if the address is already created beforing
        // running a transaction, this could potentially reduce time

        debug!("Calculated addresss: {:?}", address);

        let mut traces = vec![];
        let mut logs = vec![];
        let mut override_addresses = HashMap::with_capacity(1);
        if let Some(force_address) = force_address {
            override_addresses.insert(address, force_address);
        }
        let trace_enabled = matches!(env::var("TINYEVM_CALL_TRACE_ENABLED"), Ok(val) if val == "1");
        let inspector = LogsInspector {
            trace_enabled,
            traces: &mut traces,
            logs: &mut logs,
            override_addresses: &override_addresses,
        };

        // todo add the inspector to the exe

        let result = self.exe.as_mut().unwrap().transact_commit();

        // debug!("db {:?}", self.exe.as_ref().unwrap().db());
        // debug!("sender {:?}", owner.encode_hex::<String>(),);

        // todo_cl temp check
        self.db = self.exe.as_ref().unwrap().db().clone();
        self.env = self.exe.as_ref().unwrap().context.evm.env.as_ref().clone();

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
            info!(
                "Found address collision, reset the existing account: {}",
                address.encode_hex::<String>()
            );

            if overwrite {
                self.nuke_account(address)?;
                return self.deploy_helper(
                    salt,
                    owner,
                    contract_bytecode,
                    value,
                    false,
                    tx_gas_limit,
                    force_address,
                );
            } else {
                Err(eyre!(
                    "Address collision for {}",
                    address.encode_hex::<String>()
                ))?;
            }
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
            self.exe
                .as_mut()
                .unwrap()
                .context
                .evm
                .db
                .instrument_data
                .managed_addresses
                .insert(address, addresses);
        }

        trace!("deploy result: {:?}", result);

        let revm_result = RevmResult {
            result: result.map_err(|e| eyre!(e)),
            bug_data,
            heuristics,
            seen_pcs,
            transient_logs: vec![],
            traces: vec![],
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
        self.db.instrument_data.bug_data.clear();
        self.db.instrument_data.created_addresses.clear();
        self.db.instrument_data.heuristics = Heuristics::default();

        CALL_DEPTH.get_or_default().set(0);

        debug!("db in contract_call: {:?}", self.exe.as_ref().unwrap().db());
        debug!("sender {:?}", sender.encode_hex::<String>(),);

        if let Some(exe) = self.exe.take() {
            let exe = exe
                .modify()
                .modify_tx_env(|tx| {
                    tx.caller = sender;
                    tx.transact_to = TransactTo::Call(contract);
                    tx.data = data.into();
                    tx.value = value;
                    tx.gas_limit = tx_gas_limit.unwrap_or(TX_GAS_LIMIT);
                })
                .build();
            self.exe = Some(exe);
        }

        let mut traces = vec![];
        let mut logs = vec![];
        let trace_enabled = matches!(env::var("TINYEVM_CALL_TRACE_ENABLED"), Ok(val) if val == "1");

        let inspector = LogsInspector {
            trace_enabled,
            traces: &mut traces,
            logs: &mut logs,
            override_addresses: &mut Default::default(),
        };

        let result = {
            let exe = self.exe.as_mut().unwrap();
            exe.transact_commit()
        };

        let bug_data = self.bug_data().clone();
        let heuristics = self.heuristics().clone();
        let seen_pcs = self.pcs_by_address().clone();
        let addresses = self.created_addresses().clone();
        info!(
            "created addresses from contract call: {:?} for {:?}",
            addresses, contract
        );

        debug!("contract_call result: {:?}", result);

        if !addresses.is_empty() {
            let exe = self.exe.as_mut().unwrap();
            exe.context
                .evm
                .db
                .instrument_data
                .managed_addresses
                .insert(contract, addresses);
        }

        let ignored_addresses = self.db.ignored_addresses.clone();
        let ignored_addresses = ignored_addresses.into_iter().map(Into::into).collect();

        let revm_result = RevmResult {
            result: result.map_err(|e| eyre!(e)),
            bug_data,
            heuristics,
            seen_pcs,
            transient_logs: logs,
            traces,
            ignored_addresses,
        };
        revm_result.into()
    }

    /// Set code of an account
    pub fn set_code_by_address(&mut self, addr: Address, code: Vec<u8>) -> Result<()> {
        let db = &mut self.db;
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
        let accounts = &self.db.accounts;
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
        let accounts = &self.db.accounts;
        if let Some(account) = accounts.get(&addr) {
            Ok(account.info.balance)
        } else {
            Ok(U256::ZERO)
        }
    }

    /// Get storage by address and index
    pub fn get_storage_by_address(&self, addr: Address, index: U256) -> Result<U256> {
        let account = self
            .db
            .accounts
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
        self.db.insert_account_storage(addr, index, value)?;
        Ok(())
    }
}

impl Default for TinyEVM {
    fn default() -> Self {
        Self::new(None, None).unwrap()
    }
}

// Implementations for use in Python and Rust
#[pymethods]
impl TinyEVM {
    /// Create a new TinyEVM instance
    #[new]
    #[pyo3(signature = (fork_url = None, block_id = None))]
    pub fn new(fork_url: Option<String>, block_id: Option<u64>) -> Result<Self> {
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

        let exe = Evm::builder()
            .modify_env(|e| *e = Box::new(env.clone()))
            .with_db(db.clone())
            .build();
        let tinyevm = Self {
            exe: Some(exe),
            owner,
            fork_url,
            block_id,
            db,
            env,
            tx_gas_limit: TX_GAS_LIMIT,
            snapshots: HashMap::with_capacity(32),
            spec_id: None,
        };

        Ok(tinyevm)
    }

    /// Get addresses loaded remotely as string
    pub fn get_forked_addresses(&self) -> Result<Vec<String>> {
        let addresses = &self.db.remote_addresses;
        addresses.keys().map(|a| Ok(format!("0x{:x}", a))).collect()
    }

    /// Get remotely loaded slot indices by address
    pub fn get_forked_slots(&self, address: String) -> Result<Vec<BigInt>> {
        let address = Address::from_str(&address)?;
        self.db.remote_addresses.get(&address).map_or_else(
            || Ok(vec![]),
            |slots| Ok(slots.iter().map(ruint_u256_to_bigint).collect::<Vec<_>>()),
        )
    }

    /// Toggle for enable mode, only makes sense when fork_url is set
    pub fn toggle_enable_fork(&mut self, enable: bool) {
        self.db.fork_enabled = enable;
    }

    /// Get the current fork toggle status
    pub fn is_fork_enabled(&self) -> bool {
        self.db.fork_enabled
    }

    /// Deploy a contract using contract deploy binary and default salt of zero.
    /// The contract will be deployed with CREATE2.
    ///
    /// - `contract_deploy_code`: contract deploy binary array encoded as hex string
    /// - `owner`: owner address as a 20-byte array encoded as hex string
    #[pyo3(signature = (contract_deploy_code, owner=None))]
    pub fn deploy(
        &mut self,
        contract_deploy_code: String,
        owner: Option<String>,
    ) -> Result<Response> {
        self.deterministic_deploy(contract_deploy_code, None, owner, None, None, None, None)
    }

    /// Deploy a contract using contract deploy binary and the provided
    /// salt, the contract address is calculated by `hash(0xFF, sender,
    /// salt, bytecode)`. If the account already exists in the executor,
    /// the nonce and code of the account will be **overwritten**.
    ///
    /// For optional arguments, you can use the empty string as inputs to use the default values.
    ///
    /// [Source: <https://docs.openzeppelin.com/cli/2.8/deploying-with-create2#create2>]
    ///
    /// - `contract_deploy_code`: contract deploy binary array encoded as hex string
    /// - `salt`: (Optional, default to random) A 32-byte array encoded as hex string. Default to zero. This value has no effect if `deploy_to_address` is provided.
    /// - `owner`: Owner address as a 20-byte array encoded as hex string
    /// - `data`: (Optional, default empty) Constructor arguments encoded as hex string.
    /// - `value`: (Optional, default 0) a U256. Set the value to be included in the contract creation transaction.
    ///   - This requires the constructor to be payable.
    ///   - The transaction sender (owner) must have enough balance
    /// - `init_value`: (Optional) BigInt. Override the initial balance of the contract to this value.
    /// - `deploy_to_address`: (Optional) hex string. The address to deploy to, will be overridden if it's not empty.
    ///
    /// Returns a list consisting of 4 items `[reason, address-as-byte-array, bug_data, heuristics]`
    #[pyo3(signature = (contract_deploy_code, salt=None, owner=None, data=None, value=None, init_value=None, deploy_to_address=None))]
    pub fn deterministic_deploy(
        &mut self,
        contract_deploy_code: String, // variable length
        salt: Option<String>,         // h256 as hex string
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

        let salt = {
            if let Some(salt) = salt {
                let salt = &salt;
                U256::from_str_radix(trim_prefix(salt, "0x"), 16)?
            } else {
                U256::default()
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
        // contract_bytecode.extend(&salt.to_be_bytes::<32>().to_vec());

        let resp = {
            let resp = self.deploy_helper(
                salt,
                owner,
                contract_bytecode,
                bigint_to_ruint_u256(&value)?,
                true,
                Some(self.tx_gas_limit),
                deploy_to_address.and_then(|a| Address::from_str(&a).ok()),
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

        let resp =
            self.contract_call_helper(contract, sender, data, value, Some(self.tx_gas_limit));

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
        let env = &self.env;
        macro_rules! hex2str {
            ($val:expr) => {
                serde_json::to_string(&$val).unwrap()
            };
        }

        let r = match field.as_str() {
            // NOTE returning BigInt instead of hex string might be a better idea
            GAS_PRICE => hex2str!(env.tx.gas_price),
            CHAIN_ID => hex2str!(env.cfg.chain_id),
            BLOCK_NUMBER => hex2str!(env.block.number),
            BLOCK_TIMESTAMP => hex2str!(env.block.timestamp),
            BLOCK_DIFFICULTY => hex2str!(env.block.difficulty),
            BLOCK_GAS_LIMIT => hex2str!(env.block.gas_limit),
            BLOCK_BASE_FEE_PER_GAS => hex2str!(env.block.basefee),
            ORIGIN => format!("0x{}", hex::encode(env.tx.caller)),
            BLOCK_COINBASE => format!("0x{}", hex::encode(env.block.coinbase)),
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
        self.db.instrument_config = Some(config);
        Ok(())
    }

    /// Get current runtime instrumentation configuration
    pub fn get_instrument_config(&self) -> Result<REVMConfig> {
        let r = &self
            .db
            .instrument_config
            .as_ref()
            .ok_or_else(|| eyre!("Instrumentation config not set"))?;
        Ok(REVMConfig::from(r))
    }

    /// Set EVM env field value. Value is hex encoded string
    pub fn set_env_field_value_inner(&mut self, field: &str, value: &str) -> Result<()> {
        debug!("set_env_field_value_inner: {} {}", field, value);

        let value = trim_prefix(value, "0x");

        let to_u256 = |v: &str| U256::from_str_radix(v, 16);
        let to_address = |v: &str| Address::from_str(v);

        let env = &mut self.env;

        match field {
            GAS_PRICE => env.tx.gas_price = to_u256(value)?,
            CHAIN_ID => env.cfg.chain_id = u64::from_str_radix(value, 16)?,
            BLOCK_NUMBER => env.block.number = to_u256(value)?,
            BLOCK_TIMESTAMP => env.block.timestamp = to_u256(value)?,
            BLOCK_DIFFICULTY => env.block.difficulty = to_u256(value)?,
            BLOCK_GAS_LIMIT => env.block.gas_limit = to_u256(value)?,
            BLOCK_BASE_FEE_PER_GAS => env.block.basefee = to_u256(value)?,
            ORIGIN => env.tx.caller = to_address(value)?,
            BLOCK_COINBASE => env.block.coinbase = to_address(value)?,
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
        let db = &mut self.db;
        db.accounts.remove(&addr);
        Ok(())
    }

    /// Take a snapshot of an account, raise error if account does not exist in db
    pub fn take_snapshot(&mut self, address: String) -> Result<()> {
        let addr = Address::from_str(&address)?;
        let db = &self.db;
        if let Some(account) = db.accounts.get(&addr) {
            self.snapshots.insert(addr, account.clone());
            Ok(())
        } else {
            Err(eyre!("Account not found"))
        }
    }

    /// Copy a snapshot from one account to another, the target
    /// account storage and code will be overridden.  Raise error if
    /// account to be copied from does not exist in db
    pub fn copy_snapshot(&mut self, from: String, to: String) -> Result<()> {
        let from = Address::from_str(&from)?;
        let to = Address::from_str(&to)?;

        let db = &mut self.db;

        let account = self
            .snapshots
            .get(&from)
            .context("No snapshot found")?
            .clone();
        db.accounts.insert(to, account);
        Ok(())
    }

    pub fn clear_instrumentation(&mut self) {
        // TODO Check if we've cleared all
        self.db.instrument_data.bug_data.clear();
        self.db.instrument_data.created_addresses.clear();
        self.db.instrument_data.heuristics = Default::default();
    }

    /// Restore a snapshot for an account, raise error if there is no snapshot for the account
    pub fn restore_snapshot(&mut self, address: String) -> Result<()> {
        let addr = Address::from_str(&address)?;
        let db = &mut self.db;
        let account = self.snapshots.get(&addr).context("No snapshot found")?;

        db.accounts.insert(addr, account.clone());
        Ok(())
    }
}

/// Configuration class for instrumentation, this is a wrapper for
/// REVM::InstrumentConfig
#[pyclass(set_all, get_all)]
pub struct REVMConfig {
    /// Master switch to toggle instrumentation
    pub enabled: bool,
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
            enabled: self.enabled,
            pcs_by_address: self.pcs_by_address,
            heuristics: self.heuristics,
            record_branch_for_target_only: self.record_branch_for_target_only,
            record_sha3_mapping: self.record_sha3_mapping,
        })
    }

    /// Convert to `REVMConfig` from internal Rust struct
    fn from(config: &InstrumentConfig) -> Self {
        Self {
            enabled: config.enabled,
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
