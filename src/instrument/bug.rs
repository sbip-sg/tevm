use primitive_types::{H160, H256};
use revm::primitives::Address;
use ruint::aliases::U256;
use std::collections::{HashMap, VecDeque};
use strum_macros::Display;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Display)]
pub enum BugType {
    IntegerOverflow,
    IntegerSubUnderflow,
    /// op2 for DIV, SDIV is zero
    IntegerDivByZero,
    /// op2 for MOD, SMOD, ADDMOD, MULMOD,  is zero
    IntegerModByZero,
    PossibleIntegerTruncation,
    TimestampDependency,
    BlockNumberDependency,
    BlockValueDependency,
    TxOriginDependency,
    /// Call(input_parameter_size, destination_address)
    Call(usize, H160),
    RevertOrInvalid,
    /// Jumpi(dest)
    Jumpi(usize),
    Sload(U256),
    /// storage key, value
    Sstore(U256, U256),
    Unclassified,
}

/// Bug
#[derive(Clone, Debug, PartialEq)]
pub struct Bug {
    pub bug_type: BugType,
    pub opcode: u8,
    /// program counter
    pub position: usize,
    /// Direct contract address in which this operation is executed
    pub address_index: isize,
}

pub type BugData = VecDeque<Bug>;

impl Bug {
    /// Create a bug
    pub fn new(bug_type: BugType, opcode: u8, position: usize, address_index: isize) -> Self {
        Self {
            bug_type,
            opcode,
            position,
            address_index,
        }
    }
}

impl std::fmt::Display for Bug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BUG {} opcode: 0x{:02x} position: {}",
            self.bug_type, self.opcode, self.position
        )
    }
}

/// A MissedBranch represents a branch in a `if/else` statement not visited by the program.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MissedBranch {
    // The pc imediately before the conditional jumpi
    pub prev_pc: usize,
    /// Condition of the jumpi, true jump to `dest_pc`, false jump to `prev_pc + 1`
    pub cond: bool,
    /// Destination pc if condition is true
    pub dest_pc: usize,
    /// Distiance required to reach the missed branch
    pub distance: U256,
    /// Address of the contract in which this operation is executed
    pub address_index: isize,
}

impl MissedBranch {
    pub fn new(
        prev_pc: usize,
        dest_pc: usize,
        cond: bool,
        distance: U256,
        address_index: isize,
    ) -> Self {
        Self {
            prev_pc,
            dest_pc,
            cond,
            distance,
            address_index,
        }
    }
}

impl From<(usize, usize, bool, U256, isize)> for MissedBranch {
    fn from(
        (prev_pc, dest_pc, cond, distance, address_index): (usize, usize, bool, U256, isize),
    ) -> Self {
        Self {
            prev_pc,
            dest_pc,
            cond,
            distance,
            address_index,
        }
    }
}

/// Storing heuristics code coverage data
#[derive(Clone, Debug)]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Heuristics {
    /// Whether to skip `record_missed_branch` when jumpi occurs
    #[cfg_attr(feature = "with-serde", serde(skip_serializing))]
    pub skip: bool,
    /// List of jumpi destinations
    pub coverage: VecDeque<usize>,
    /// Current distance
    #[cfg_attr(feature = "with-serde", serde(skip_serializing))]
    pub distance: U256,
    /// Missed branches
    pub missed_branches: Vec<MissedBranch>,
    /// Mapping from SHA3 output to input. This is for reverse lookup of slot mapping
    pub sha3_mapping: HashMap<H256, Vec<u8>>,
    // Addresses the transaction was executed on
    pub seen_addresses: Vec<Address>,
}

impl Default for Heuristics {
    fn default() -> Heuristics {
        Heuristics {
            skip: true,
            coverage: VecDeque::with_capacity(32), // Set some initial capacity to avoid some data copying
            distance: U256::MAX,
            missed_branches: Vec::with_capacity(32),
            sha3_mapping: HashMap::with_capacity(32),
            seen_addresses: Vec::with_capacity(32),
        }
    }
}

impl Heuristics {
    /// Create new Heuristics data
    pub fn new() -> Self {
        Heuristics::default()
    }

    /// Reset Heuristics data
    pub fn reset(&mut self) {
        self.skip = true;
        self.coverage = VecDeque::with_capacity(32);
        self.distance = U256::MAX;
        self.missed_branches = Vec::with_capacity(32);
    }

    /// Record Sha3 mapping
    pub fn record_sha3_mapping(&mut self, input: &[u8], output: H256) {
        self.sha3_mapping.insert(output, input.to_vec());
    }

    /// Record missing branch data
    pub fn record_missed_branch(
        &mut self,
        prev_pc: usize,
        dest_pc: usize,
        cond: bool,
        address_index: isize,
    ) {
        let distance = self.distance;

        if self.missed_branches.iter_mut().any(|x| {
            matches!(x, MissedBranch { prev_pc: p, dest_pc: d, distance: dist, .. } if *p == prev_pc && *d == dest_pc && *dist == distance)
        }) {
            return;
        }

        self.missed_branches.push(MissedBranch::new(
            prev_pc,
            dest_pc,
            cond,
            distance,
            address_index,
        ));
        // if self.missed_branchs.len() > 2 {
        //     self.missed_branchs.drain(0..self.missed_branchs.len() - 2);
        // }
    }
}

/// Instrumentation runtime configuration
#[derive(Clone, Debug)]
#[cfg_attr(feature = "with-serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InstrumentConfig {
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
    pub target_address: Address,
    /// Whether to record SHA3 mappings
    pub record_sha3_mapping: bool,
}

impl Default for InstrumentConfig {
    fn default() -> InstrumentConfig {
        InstrumentConfig {
            pcs_by_address: true,
            heuristics: true,
            record_branch_for_target_only: false,
            target_address: Default::default(),
            record_sha3_mapping: true,
        }
    }
}
