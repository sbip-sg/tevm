use eyre::Result;
use hashbrown::{HashMap, HashSet};
use hex::ToHex;
use num_bigint::BigInt;
use pyo3::{exceptions::PyValueError, prelude::*};
use revm::primitives::{Address, ExecutionResult, Output};
use ruint::aliases::U256;
use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use std::collections::HashMap as StdHashMap;
use std::collections::HashSet as StdHashSet;

use crate::{
    instrument::bug::*,
    logs::{CallTrace, Log},
    ruint_u256_to_bigint, trim_prefix,
};
use primitive_types::H160;

/// Response from REVM executor
pub struct RevmResult {
    /// Tx result
    pub result: Result<ExecutionResult, eyre::Error>,
    /// Bug data
    pub bug_data: BugData,
    /// Heuristics data
    pub heuristics: Heuristics,
    /// Map of seen pcs: from address to a set of PCs
    pub seen_pcs: HashMap<Address, HashSet<usize>>,
    /// Call traces
    pub traces: Vec<CallTrace>,
    /// Transient logs (including logs for reverted calls)
    pub transient_logs: Vec<Log>,
    /// Ignored addresses from ForkDb
    pub ignored_addresses: HashSet<Address>,
}

/// WrappedBug is a wrapper around Bug for use by Python
#[pyclass(get_all)]
#[derive(Debug)]
pub struct WrappedBug {
    /// BugType as a map from string to string. Numerical values are hex encoded
    pub bug_type: StdHashMap<String, String>,
    /// opcode
    pub opcode: u8,
    /// program counter
    pub position: usize,
    /// Index of the contract address in seen_addresses
    pub address_index: isize,
}

/// Wrapper around Missed Branch
#[pyclass(get_all)]
#[derive(Clone, Debug)]
pub struct WrappedMissedBranch {
    /// Previous program counter
    pub prev_pc: usize,
    /// Destination pc if condition is true
    pub dest_pc: usize,
    pub cond: bool,
    /// Distiance required to reach the missed branch
    pub distance: BigInt,
    pub address_index: isize,
}

/// Wrapper around Heuristics
#[pyclass(get_all)]
#[derive(Clone, Debug)]
pub struct WrappedHeuristics {
    /// List of jumpi destinations
    pub coverage: Vec<usize>,
    /// Missed branches
    pub missed_branches: Vec<WrappedMissedBranch>,
    /// Mapping from SHA3 output to input. This is for reverse lookup of slot mapping
    pub sha3_mapping: StdHashMap<String, Vec<u8>>,
    /// Addresses the transaction was executed on
    pub seen_addresses: Vec<String>,
    /// extra data from constructor (the distance of missed branch)
    pub extra_data: BigInt,
}

impl Display for WrappedHeuristics {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Coverage: {:?}", &self)
    }
}

#[pymethods]
impl WrappedHeuristics {
    /// Get the string respresentation
    fn __str__(&self) -> String {
        format!("{:?}", self)
    }
}

impl From<Heuristics> for WrappedHeuristics {
    fn from(heuristics: Heuristics) -> Self {
        let coverage = heuristics.coverage.iter().copied().collect();
        let missed_branches = heuristics
            .missed_branches
            .iter()
            .map(|x| WrappedMissedBranch {
                prev_pc: x.prev_pc,
                dest_pc: x.dest_pc,
                cond: x.cond,
                distance: ruint_u256_to_bigint(&x.distance),
                address_index: x.address_index,
            })
            .collect();
        let mut sha3_mapping = StdHashMap::new();
        for (k, v) in heuristics.sha3_mapping {
            sha3_mapping.insert(format!("0x{:x}", k), v);
        }
        let mut seen_addresses = Vec::new();
        for addr in heuristics.seen_addresses {
            seen_addresses.push(format!("0x{}", addr.encode_hex::<String>()));
        }
        let extra_data = ruint_u256_to_bigint(&heuristics.distance);
        Self {
            coverage,
            missed_branches,
            sha3_mapping,
            seen_addresses,
            extra_data,
        }
    }
}

/// Convert a `BugType` to a map from string to string, numerical values are encoded as hex string
fn hash_map_from_bug_type(bug_type: &BugType) -> StdHashMap<String, String> {
    let mut map = StdHashMap::new();
    match bug_type {
        BugType::Jumpi(dest) => {
            map.insert("type".into(), "Jumpi".into());
            map.insert("dest".into(), dest.to_string());
        }
        BugType::Sload(index) => {
            map.insert("type".into(), "Sload".into());
            map.insert(
                "index".into(),
                format!(
                    "0x{}",
                    index
                        .to_be_bytes::<{ U256::BYTES }>()
                        .encode_hex::<String>()
                ),
            );
        }
        BugType::Sstore(index, value) => {
            map.insert("type".into(), "Sstore".into());
            map.insert(
                "index".into(),
                format!(
                    "0x{}",
                    index
                        .to_be_bytes::<{ U256::BYTES }>()
                        .encode_hex::<String>()
                ),
            );
            map.insert(
                "value".into(),
                format!(
                    "0x{}",
                    value
                        .to_be_bytes::<{ U256::BYTES }>()
                        .encode_hex::<String>()
                ),
            );
        }
        BugType::Call(input_parameter_size, destination_address) => {
            map.insert("type".into(), "Call".into());
            map.insert("size".into(), input_parameter_size.to_string());
            map.insert(
                "dest".to_string(),
                format!("0x{}", destination_address.encode_hex::<String>()),
            );
        }
        BugType::IntegerOverflow => {
            map.insert("type".into(), "IntegerOverflow".into());
        }
        BugType::IntegerSubUnderflow => {
            map.insert("type".to_string(), "IntegerSubUnderflow".to_string());
        }
        BugType::IntegerDivByZero => {
            map.insert("type".to_string(), "IntegerDivByZero".to_string());
        }
        BugType::IntegerModByZero => {
            map.insert("type".to_string(), "IntegerModByZero".to_string());
        }
        BugType::PossibleIntegerTruncation => {
            map.insert("type".to_string(), "PossibleIntegerTruncation".to_string());
        }
        BugType::TimestampDependency => {
            map.insert("type".to_string(), "TimestampDependency".to_string());
        }
        BugType::BlockValueDependency => {
            map.insert("type".to_string(), "BlockValueDependency".to_string());
        }
        BugType::BlockNumberDependency => {
            map.insert("type".to_string(), "BlockNumberDependency".to_string());
        }
        BugType::TxOriginDependency => {
            map.insert("type".to_string(), "TxOriginDependency".to_string());
        }
        BugType::RevertOrInvalid => {
            map.insert("type".to_string(), "RevertOrInvalid".to_string());
        }
        BugType::Unclassified => {
            map.insert("type".to_string(), "Unclassified".to_string());
        }
    }
    map
}

impl From<Bug> for WrappedBug {
    fn from(bug: Bug) -> Self {
        Self {
            bug_type: hash_map_from_bug_type(&bug.bug_type),
            opcode: bug.opcode,
            position: bug.position,
            address_index: bug.address_index,
        }
    }
}

#[pymethods]
impl WrappedBug {
    /// Get the string representation bug type
    fn __str__(&self) -> String {
        format!("{:?}", &self)
    }
}

/// A wrapper around `Log` for use by Python
/// All fields are hex encoded
#[derive(Clone, Debug)]
#[pyclass]
pub struct PyLog {
    #[pyo3(get)]
    pub id: usize,
    #[pyo3(get)]
    pub depth: usize,
    #[pyo3(get)]
    pub address: String,
    #[pyo3(get)]
    pub topics: Vec<String>,
    #[pyo3(get)]
    pub data: String,
}

/// A wrapper around `CallTrace` for use by Python
/// All fields are hex encoded
#[derive(Clone, Debug)]
#[pyclass]
pub struct PyCallTrace {
    #[pyo3(get)]
    pub id: usize,
    #[pyo3(get)]
    pub caller: String,
    #[pyo3(get)]
    pub to: String,
    #[pyo3(get)]
    pub value: BigInt,
    #[pyo3(get)]
    pub input: String,
    #[pyo3(get)]
    pub depth: usize,
    #[pyo3(get)]
    pub return_data: String,
    #[pyo3(get)]
    pub is_static: bool,
    #[pyo3(get)]
    pub status: String,
}

impl From<Log> for PyLog {
    fn from(log: Log) -> Self {
        Self {
            id: log.id,
            depth: log.depth,
            address: format!("0x{}", log.address.encode_hex::<String>()),
            topics: log
                .topics
                .iter()
                .map(|x| format!("0x{}", x.encode_hex::<String>()))
                .collect(),
            data: format!("0x{}", log.data.encode_hex::<String>()),
        }
    }
}

impl From<CallTrace> for PyCallTrace {
    fn from(trace: CallTrace) -> Self {
        let input = if trace.input.is_empty() {
            "".into()
        } else {
            format!("0x{}", trace.input.encode_hex::<String>())
        };
        Self {
            id: trace.id,
            caller: format!("0x{}", trace.from.encode_hex::<String>()),
            to: format!("0x{}", trace.to.encode_hex::<String>()),
            value: ruint_u256_to_bigint(&trace.value),
            input,
            depth: trace.depth,
            return_data: trace
                .return_data
                .map(|x| format!("0x{}", x.encode_hex::<String>()))
                .unwrap_or_default(),
            is_static: trace.is_static,
            status: trace.status.map(|x| format!("{:?}", x)).unwrap_or_default(),
        }
    }
}

/// Response from EVM executor
#[pyclass]
#[derive(Clone, Debug)]
pub struct Response {
    /// True if the execution is exitted normally
    #[pyo3(get)]
    pub success: bool,
    /// A ExitReason code
    #[pyo3(get)]
    pub exit_reason: String,
    /// Address for deploy, or return data for contract call
    #[pyo3(get)]
    pub data: Vec<u8>,
    /// Emitted events
    #[pyo3(get)]
    pub events: Vec<PyLog>,
    #[pyo3(get)]
    pub traces: Vec<PyCallTrace>,
    /// Bug signal data
    pub bug_data: BugData,
    /// Heuristics data
    pub heuristics: Heuristics,
    /// Gas usage
    #[pyo3(get)]
    pub gas_usage: u64,
    /// Ignored addresses
    #[pyo3(get)]
    pub ignored_addresses: Vec<String>,
    /// Seen PCs by address
    pub seen_pcs: HashMap<Address, HashSet<usize>>,
}

impl From<RevmResult> for Response {
    fn from(
        RevmResult {
            result,
            bug_data,
            heuristics,
            seen_pcs,
            traces,
            transient_logs,
            ignored_addresses,
        }: RevmResult,
    ) -> Self {
        let events = transient_logs
            .into_iter()
            .map(|x| x.into())
            .collect::<Vec<_>>();
        let traces = traces.into_iter().map(|x| x.into()).collect();
        let ignored_addresses = ignored_addresses
            .iter()
            .map(|x| format!("0x{}", x.encode_hex::<String>()))
            .collect();
        if result.is_err() {
            return Self {
                success: false,
                exit_reason: format!("EVM InfallibleError: {:?}", result.err()),
                data: Vec::new(),
                bug_data,
                heuristics,
                gas_usage: 0,
                seen_pcs,
                events,
                traces,
                ignored_addresses,
            };
        }

        let result = result.unwrap();
        let success = result.is_success();

        let gas_usage = result.gas_used();

        let exit_reason = match result {
            ExecutionResult::Success { .. } => "Success".into(),
            ExecutionResult::Revert { .. } => "Revert".into(),
            ExecutionResult::Halt { reason, .. } => format!("{:?}", reason),
        };

        let data = match result {
            ExecutionResult::Success { output, .. } => match output {
                Output::Call(data) => data.to_vec(),
                Output::Create(_data, Some(address)) => address.to_vec(),
                _ => Vec::new(), // WARN: assuming no such case that creation succeeds but no address is returned
            },
            ExecutionResult::Revert { output, .. } => output.to_vec(),
            _ => Vec::new(),
        };

        Self {
            success,
            exit_reason,
            data,
            bug_data,
            heuristics,
            gas_usage,
            seen_pcs,
            events,
            traces,
            ignored_addresses,
        }
    }
}

impl Display for Response {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "success: {}, exit_reason: {}, data: {:?}, gas_usage: {}, bugs: {:?}, heuristics: {:?}, seen_pcs: {:?}",
            self.success, self.exit_reason, self.data, self.gas_usage, self.bug_data, self.heuristics, self.seen_pcs
        )
    }
}

/// A map from address as hex strign to a list of PCs visited by the adddress
#[pyclass]
pub struct SeenPcsMap(HashMap<String, HashSet<usize>>);

#[pymethods]
impl SeenPcsMap {
    /// Return all keys (addresses) in the map
    fn keys(&self) -> Vec<String> {
        self.0.keys().map(|x| x.to_string()).collect()
    }
    /// Return seen PCs for the given address
    fn get(&self, key: &str) -> Option<StdHashSet<usize>> {
        self.0.get(key).map(|x| x.into_iter().copied().collect())
    }
}

impl From<HashMap<H160, HashSet<usize>>> for SeenPcsMap {
    fn from(seen_pcs: HashMap<H160, HashSet<usize>>) -> Self {
        let mut map = HashMap::new();
        for (addr, pcs) in seen_pcs {
            map.insert(format!("0x{}", addr.encode_hex::<String>()), pcs);
        }
        Self(map)
    }
}

#[pymethods]
impl Response {
    /// Response to string for Python
    fn __str__(&self) -> String {
        self.to_string()
    }

    /// List of bugs signals
    #[getter]
    fn bug_data(&self) -> Vec<WrappedBug> {
        self.bug_data.iter().map(|b| b.clone().into()).collect()
    }

    /// Heuristics data
    #[getter]
    fn heuristics(&self) -> WrappedHeuristics {
        self.heuristics.clone().into()
    }

    /// Return a set of unique PCs visited by the address
    fn pcs_by_address(&self, address: String) -> Result<StdHashSet<usize>> {
        let mut pc_set = StdHashSet::new();
        let address = Address::from_str(trim_prefix(&address, "0x"))
            .or(Err(PyValueError::new_err("Invalid address format")))?;
        let pcs = self.seen_pcs.get(&address);

        if let Some(pcs) = pcs {
            for pc in pcs {
                pc_set.insert(*pc);
            }
            Ok(pc_set)
        } else {
            Ok(pc_set)
        }
    }
}
