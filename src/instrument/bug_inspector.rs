use hashbrown::{HashMap, HashSet};
use revm::{
    interpreter::{instructions::i256, opcode, CreateInputs, CreateOutcome, Interpreter, OpCode},
    primitives::{Address, U256},
    Database, EvmContext, Inspector,
};
use tracing::{debug, warn};

use crate::i256_diff;

use super::{Bug, BugData, BugType, Heuristics, InstrumentConfig};

#[derive(Default)]
pub struct BugInspector {
    /// Change the created address to another address
    pub create_address_overrides: HashMap<Address, Address>,
    pub bug_data: BugData,
    pub heuristics: Heuristics,
    // Mapping from contract address to a set of PCs seen in the execution
    pub pcs_by_address: HashMap<Address, HashSet<usize>>,
    pub instrument_config: InstrumentConfig,
    // Holding the addresses created in the current transaction,
    // must be cleared by transaction caller before or after each transaction
    pub created_addresses: Vec<Address>,
    // Managed addresses: contract -> addresses created by any transaction from the contract
    pub managed_addresses: HashMap<Address, Vec<Address>>,
    /// Stack inputs of the current opcodes. Only updated when the opcode is interesting
    inputs: Vec<U256>,
    /// Current opcode
    opcode: u8,
    // Current program counter
    pc: usize,
    /// Current index in the execution. For tracking peephole optimized if-statement
    step_index: u64,
    last_index_sub: u64,
    last_index_eq: u64,
}

impl BugInspector {
    pub fn inc_step_index(&mut self) {
        self.step_index += 1;
    }

    /// Returns true if this is possible peephole optimized code,
    /// assuming when calling this function the current opcode is
    /// JUMPI
    pub fn possibly_if_equal(&self) -> bool {
        self.step_index < self.last_index_sub + 10 && self.step_index > self.last_index_eq + 10
    }

    fn record_seen_address(&mut self, address: Address) -> isize {
        // make sure target_address is the first address added
        if self.instrument_config.record_branch_for_target_only {
            if self.heuristics.seen_addresses.is_empty() {
                self.heuristics
                    .seen_addresses
                    .push(self.instrument_config.target_address);
            }

            if self.instrument_config.target_address == address {
                return 0;
            }
        }

        let idx = self
            .heuristics
            .seen_addresses
            .iter()
            .position(|a| *a == address);
        if let Some(i) = idx {
            return i as isize;
        }

        self.heuristics.seen_addresses.push(address);
        self.heuristics.seen_addresses.len() as isize - 1
    }

    /// Record the program counter for the given contract address
    pub fn record_pc(&mut self, address: Address, pc: usize) {
        let pcs = self.pcs_by_address.entry(address).or_default();
        pcs.insert(pc);
    }

    pub fn add_bug(&mut self, bug: Bug) {
        match bug.bug_type {
            BugType::Jumpi(dest) => {
                if self.instrument_config.heuristics {
                    // March 15 bug patch: keep last 256 elements
                    self.heuristics.coverage.push_back(dest);
                    if self.heuristics.coverage.len() > 256 {
                        self.heuristics.coverage.pop_front();
                    }
                }
            }
            BugType::Sload(_key) => {
                if self.bug_data.len() > 256 {
                    // this will lead to poor performance
                    // self.bug_data.retain(|front| {
                    //     !(front.address_index == address_idx
                    //         && matches!(front.bug_type, BugType::Sload(k) if k == key))
                    // });
                    self.bug_data.pop_front();
                }
                self.bug_data.push_back(bug);
            }
            BugType::Sstore(_key, _) => {
                if self.bug_data.len() > 256 {
                    // self.bug_data.retain(|front| {
                    //     !(front.address_index == address_idx
                    //         && matches!(front.bug_type, BugType::Sstore(k, _) if k == key))
                    // });
                    self.bug_data.pop_front();
                }
                self.bug_data.push_back(bug);
            }
            _ => self.bug_data.push_back(bug),
        }
    }
}

impl<DB> Inspector<DB> for BugInspector
where
    DB: Database,
{
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        let _ = interp;
        let _ = context;
        self.opcode = interp.current_opcode();
        let opcode = OpCode::new(self.opcode);
        self.pc = interp.program_counter();

        if let Some(OpCode::EQ) = opcode {
            self.last_index_eq = self.step_index;
        }

        if let Some(OpCode::SUB) = opcode {
            self.last_index_sub = self.step_index;
        }

        // it's also possible to handle REVERT, INVALID here

        match opcode {
            Some(
                OpCode::JUMPI
                // possible overflows / underflows
                | OpCode::ADD
                | OpCode::SUB
                | OpCode::MUL
                | OpCode::DIV
                | OpCode::SDIV
                | OpCode::SMOD
                | OpCode::EXP
                // heuristic distance
                | OpCode::LT
                | OpCode::SLT
                | OpCode::GT
                | OpCode::SGT
                | OpCode::EQ
                // possible truncation
                | OpCode::AND
            ) => {
                self.inputs.clear();
                let a = interp.stack().peek(0);
                let b = interp.stack().peek(1);
                if let (Ok(a), Ok(b)) = (a, b) {
                    self.inputs.push(a);
                    self.inputs.push(b);
                }
            },
            Some( OpCode::ADDMOD | OpCode::MULMOD) => {
                self.inputs.clear();
                let a = interp.stack().peek(0);
                let b = interp.stack().peek(1);
                let n = interp.stack().peek(2);

                if let (Ok(a), Ok(b), Ok(n)) = (a, b, n) {
                    self.inputs.push(a);
                    self.inputs.push(b);
                    self.inputs.push(n);
                }
            }
            _ => {}
        }

        self.inc_step_index();
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let address = interp.contract().target_address;
        let address_index = self.record_seen_address(address);
        let opcode = self.opcode;
        let pc = self.pc;

        if self.instrument_config.pcs_by_address {
            self.record_pc(address, pc);
        }

        let success = interp.instruction_result.is_ok();

        // Check for overflow and underflow
        match (opcode, success) {
            (opcode::ADD, true) => {
                if let Ok(r) = interp.stack().peek(0) {
                    if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                        if r < *a || r < *b {
                            let bug = Bug::new(BugType::IntegerOverflow, opcode, pc, address_index);
                            self.add_bug(bug);
                        }
                    }
                }
            }
            (opcode::MUL, true) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    if mul_overflow(*a, *b) {
                        let bug = Bug::new(BugType::IntegerOverflow, opcode, pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            (opcode::SUB, true) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    if a < b {
                        let bug = Bug::new(BugType::IntegerSubUnderflow, opcode, pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            (opcode::DIV | opcode::SDIV | opcode::SMOD, true) => {
                if let Some(b) = self.inputs.get(1) {
                    if b == &U256::ZERO {
                        let bug = Bug::new(BugType::IntegerDivByZero, opcode, pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            (opcode::ADDMOD | opcode::MULMOD, true) => {
                if let Some(n) = self.inputs.get(2) {
                    if n == &U256::ZERO {
                        let bug = Bug::new(BugType::IntegerDivByZero, opcode, pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            (opcode::EXP, true) => {
                // todo_cl check for overflow
                if let (Some(a), Some(b), Ok(r)) = (
                    self.inputs.first(),
                    self.inputs.get(1),
                    interp.stack().peek(0),
                ) {
                    if exp_overflow(*a, *b, r) {
                        let bug = Bug::new(BugType::IntegerOverflow, opcode, pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            (opcode::LT, true) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    let distance = if a >= b {
                        a.overflowing_sub(*b).0.saturating_add(U256::from(1))
                    } else {
                        b.overflowing_sub(*a).0
                    };
                    self.heuristics.distance = distance;
                }
            }
            (opcode::GT, true) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    let distance = if a >= b {
                        a.overflowing_sub(*b).0
                    } else {
                        b.overflowing_sub(*a).0.saturating_add(U256::from(1))
                    };
                    self.heuristics.distance = distance;
                }
            }
            (opcode::SLT, true) => {
                if let (Some(a), Some(b), Ok(r)) = (
                    self.inputs.first(),
                    self.inputs.get(1),
                    interp.stack().peek(0),
                ) {
                    let mut distance = if a >= b {
                        a.overflowing_sub(*b).0
                    } else {
                        b.overflowing_sub(*a).0
                    };
                    if r == U256::ZERO {
                        distance = distance.saturating_add(U256::from(1));
                    }
                    self.heuristics.distance = distance;
                }
            }
            (opcode::SGT, true) => {
                if let (Some(a), Some(b), Ok(r)) = (
                    self.inputs.first(),
                    self.inputs.get(1),
                    interp.stack().peek(0),
                ) {
                    let (mut distance, _) = i256_diff(a, b);
                    if r == U256::ZERO {
                        distance = distance.saturating_add(U256::from(1));
                    }
                    self.heuristics.distance = distance;
                }
            }
            (opcode::EQ, true) => {
                if let (Some(a), Some(b), Ok(r)) = (
                    self.inputs.first(),
                    self.inputs.get(1),
                    interp.stack().peek(0),
                ) {
                    let mut distance = if a >= b {
                        a.overflowing_sub(*b).0
                    } else {
                        b.overflowing_sub(*a).0
                    };
                    if r == U256::ZERO {
                        distance = distance.saturating_add(U256::from(1));
                    }
                    self.heuristics.distance = distance;
                }
            }
            (opcode::AND, true) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    // check if there is an possible truncation

                    // For AND operator, if either side of the operands equals
                    // u8, u16, ..., and the other side is larger than this
                    // operand, generate possible integer truncation signal
                    let mut i = 1;
                    let possible_overflow = loop {
                        if i == 32 {
                            break false;
                        }

                        let r = U256::MAX >> (i * 8);

                        if r == *a && b > a {
                            break true;
                        }

                        if r == *b && a > b {
                            break true;
                        }
                        i += 1;
                    };
                    if possible_overflow {
                        let bug = Bug::new(
                            BugType::PossibleIntegerTruncation,
                            opcode,
                            pc,
                            address_index,
                        );
                        self.add_bug(bug);
                    }
                }
            }
            (opcode::JUMPI, true) => {
                // Check for missed branches
                let target_address = self.instrument_config.target_address;
                macro_rules! update_heuritics {
                    // (prev_pc, dest_pc_if_cond_is_true, cond)
                    ($prev_pc: ident, $dest_pc: expr, $cond: expr) => {
                        if !self.instrument_config.record_branch_for_target_only
                            || address == target_address
                        {
                            let heuristics = &mut self.heuristics;
                            heuristics.record_missed_branch(
                                $prev_pc,
                                $dest_pc,
                                $cond,
                                address_index,
                            );
                            let target = if $cond { $dest_pc } else { $prev_pc + 1 };
                            let bug =
                                Bug::new(BugType::Jumpi(target), opcode, $prev_pc, address_index);
                            self.add_bug(bug);
                        }
                    };
                }

                // NOTE: invalid jumps are ignored
                if let (Some(dest), Some(value)) = (self.inputs.first(), self.inputs.get(1)) {
                    // Check for distance in peephole optimized if-statement
                    if self.possibly_if_equal() {
                        let max = U256::MAX;
                        let mut half = U256::MAX;
                        half.set_bit(31, false);
                        let h = &mut self.heuristics;
                        h.distance = {
                            // smallest distance from the `value` to U256::MAX and 0
                            if *value > half {
                                max - value + U256::from(1)
                            } else {
                                *value
                            }
                        };
                    }

                    let dest = usize::try_from(dest).unwrap();
                    let cond = *value != U256::ZERO;
                    update_heuritics!(pc, dest, cond);
                }
            }
            (opcode::BLOBHASH, _) => {
                let bug = Bug::new(BugType::BlockValueDependency, opcode, pc, address_index);
                self.add_bug(bug);
            }
            (opcode::COINBASE, _) => {
                let bug = Bug::new(BugType::BlockValueDependency, opcode, pc, address_index);
                self.add_bug(bug);
            }
            (opcode::TIMESTAMP, _) => {
                let bug = Bug::new(BugType::TimestampDependency, opcode, pc, address_index);
                self.add_bug(bug);
            }
            (opcode::NUMBER, _) => {
                let bug = Bug::new(BugType::BlockNumberDependency, opcode, pc, address_index);
                self.add_bug(bug);
            }
            (opcode::DIFFICULTY, _) => {
                let bug = Bug::new(BugType::BlockValueDependency, opcode, pc, address_index);
                self.add_bug(bug);
            }

            (opcode::REVERT | opcode::INVALID, _) => {
                let bug = Bug::new(BugType::RevertOrInvalid, opcode, pc, address_index);
                self.add_bug(bug);
            }

            _ => (),
        }
    }

    #[inline]
    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        _inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        let CreateOutcome { result, address } = &outcome;
        if let Some(address) = address {
            if let Some(override_address) = self.create_address_overrides.get(address) {
                debug!(
                    "Overriding created address {:?} with {:?}",
                    address, override_address
                );
                let state = &mut context.journaled_state.state;
                if let Some(value) = state.remove(address) {
                    state.insert(*override_address, value);
                } else {
                    warn!(
                        "Contract created but no state associated with it? Contract address: {:?}",
                        address
                    );
                }
                return CreateOutcome::new(result.to_owned(), Some(*override_address));
            }
        }
        outcome
    }
}

fn mul_overflow(a: U256, b: U256) -> bool {
    let zero = U256::ZERO;
    if a == zero || b == zero {
        false
    } else {
        a > U256::MAX.wrapping_div(b)
    }
}

fn exp_overflow(a: U256, b: U256, r: U256) -> bool {
    let max_value: U256 = U256::MAX;
    let mut result: U256 = U256::from(1u64);

    if b == U256::ZERO {
        return r != U256::from(1u64);
    }

    let mut i = U256::ZERO;

    while i < b {
        if result > max_value / a {
            return true;
        }
        result *= a;
        i += U256::from(1u64);
    }

    result != r
}
