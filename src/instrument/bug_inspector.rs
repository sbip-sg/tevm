use hashbrown::{HashMap, HashSet};
use primitive_types::{H160, H256};
use revm::{
    interpreter::{CreateInputs, CreateOutcome, Interpreter, OpCode},
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
    opcode: Option<OpCode>,
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
        let opcode = interp.current_opcode();
        let opcode = OpCode::new(opcode);
        self.opcode = opcode;
        self.pc = interp.program_counter();

        if let Some(OpCode::EQ) = opcode {
            self.last_index_eq = self.step_index;
        }

        if let Some(OpCode::SUB) = opcode {
            self.last_index_sub = self.step_index;
        }

        self.inputs.clear();
        if let Some(
            op @ (OpCode::JUMPI
            | OpCode::CALL
            | OpCode::CALLCODE
            | OpCode::DELEGATECALL
            | OpCode::STATICCALL
            | OpCode::SSTORE
            | OpCode::SLOAD
            | OpCode::ADD
            | OpCode::SUB
            | OpCode::MUL
            | OpCode::DIV
            | OpCode::SDIV
            | OpCode::MOD
            | OpCode::SMOD
            | OpCode::EXP
            | OpCode::LT
            | OpCode::SLT
            | OpCode::GT
            | OpCode::SGT
            | OpCode::EQ
            | OpCode::AND
            | OpCode::ADDMOD
            | OpCode::MULMOD
            | OpCode::KECCAK256),
        ) = opcode
        {
            let num_inputs = op.inputs();
            for i in 0..num_inputs {
                if let Ok(v) = interp.stack().peek(i as usize) {
                    self.inputs.push(v);
                } else {
                    break;
                }
            }
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

        match opcode {
            Some(op @ OpCode::ADD) => {
                if let Ok(r) = interp.stack().peek(0) {
                    if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                        if r < *a || r < *b {
                            let bug =
                                Bug::new(BugType::IntegerOverflow, op.get(), pc, address_index);
                            self.add_bug(bug);
                        }
                    }
                }
            }
            Some(op @ OpCode::MUL) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    if mul_overflow(*a, *b) {
                        let bug = Bug::new(BugType::IntegerOverflow, op.get(), pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            Some(op @ OpCode::SUB) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    if a < b {
                        let bug =
                            Bug::new(BugType::IntegerSubUnderflow, op.get(), pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            Some(op @ (OpCode::SMOD | OpCode::MOD)) => {
                if let Some(b) = self.inputs.get(1) {
                    if *b == U256::ZERO {
                        let bug = Bug::new(BugType::IntegerModByZero, op.get(), pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            Some(op @ (OpCode::DIV | OpCode::SDIV)) => {
                if let Some(b) = self.inputs.get(1) {
                    if *b == U256::ZERO {
                        let bug = Bug::new(BugType::IntegerDivByZero, op.get(), pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            Some(op @ (OpCode::ADDMOD | OpCode::MULMOD)) => {
                if let Some(n) = self.inputs.get(2) {
                    if n == &U256::ZERO {
                        let bug = Bug::new(BugType::IntegerModByZero, op.get(), pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            Some(op @ OpCode::EXP) => {
                println!("checking-exp-overflow");
                // todo_cl check for overflow
                if let (Some(a), Some(b), Ok(r)) = (
                    self.inputs.first(),
                    self.inputs.get(1),
                    interp.stack().peek(0),
                ) {
                    if exp_overflow(*a, *b, r) {
                        let bug = Bug::new(BugType::IntegerOverflow, op.get(), pc, address_index);
                        self.add_bug(bug);
                    }
                }
            }
            Some(OpCode::LT) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    let distance = if a >= b {
                        a.overflowing_sub(*b).0.saturating_add(U256::from(1))
                    } else {
                        b.overflowing_sub(*a).0
                    };
                    self.heuristics.distance = distance;
                }
            }
            Some(OpCode::GT) => {
                if let (Some(a), Some(b)) = (self.inputs.first(), self.inputs.get(1)) {
                    let distance = if a >= b {
                        a.overflowing_sub(*b).0
                    } else {
                        b.overflowing_sub(*a).0.saturating_add(U256::from(1))
                    };
                    self.heuristics.distance = distance;
                }
            }
            Some(OpCode::SLT) => {
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
            Some(OpCode::SGT) => {
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
            Some(OpCode::EQ) => {
                if let (Some(a), Some(b), Ok(r)) = (
                    self.inputs.first(),
                    self.inputs.get(1),
                    interp.stack().peek(0),
                ) {
                    let mut distance = if a > b {
                        a.overflowing_sub(*b).0
                    } else {
                        b.overflowing_sub(*a).0
                    };
                    if r != U256::ZERO {
                        distance = U256::from(1);
                    }
                    self.heuristics.distance = distance;
                }
            }
            Some(op @ OpCode::AND) => {
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
                            op.get(),
                            pc,
                            address_index,
                        );
                        self.add_bug(bug);
                    }
                }
            }
            Some(op @ OpCode::SSTORE) => {
                if let (Some(key), Some(value)) = (self.inputs.first(), self.inputs.get(1)) {
                    let bug = Bug::new(
                        BugType::Sstore(*key, *value),
                        op.get(),
                        self.pc,
                        address_index,
                    );
                    self.add_bug(bug);
                }
            }
            Some(op @ OpCode::SLOAD) => {
                if let Some(key) = self.inputs.first() {
                    let bug = Bug::new(BugType::Sload(*key), op.get(), self.pc, address_index);
                    self.add_bug(bug);
                }
            }
            Some(op @ OpCode::ORIGIN) => {
                let bug = Bug::new(
                    BugType::TxOriginDependency,
                    op.get(),
                    self.pc,
                    address_index,
                );
                self.add_bug(bug);
            }

            Some(
                op @ (OpCode::CALL | OpCode::CALLCODE | OpCode::DELEGATECALL | OpCode::STATICCALL),
            ) => {
                let in_len = {
                    if matches!(op, OpCode::CALL | OpCode::CALLCODE) {
                        self.inputs.get(4)
                    } else {
                        self.inputs.get(3)
                    }
                };
                let address = self.inputs.get(1);

                if let (Some(in_len), Some(callee)) = (in_len, address) {
                    let callee_bytes: [u8; 32] = callee.to_be_bytes();
                    let callee = H160::from_slice(&callee_bytes[12..]);
                    let in_len = usize::try_from(in_len).unwrap();
                    let bug = Bug::new(
                        BugType::Call(in_len, callee),
                        op.get(),
                        self.pc,
                        address_index,
                    );
                    self.add_bug(bug);
                }
            }
            Some(op @ OpCode::JUMPI) => {
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
                                Bug::new(BugType::Jumpi(target), op.get(), $prev_pc, address_index);
                            self.add_bug(bug);
                        }
                    };
                }

                // NOTE: invalid jumps are ignored
                if let (Some(counter), Some(cond)) = (self.inputs.first(), self.inputs.get(1)) {
                    // Check for distance in peephole optimized if-statement
                    if self.possibly_if_equal() {
                        debug!(
                            "Possible peephole optimized if-statement found, inputs: {:?} pc {}",
                            self.inputs, self.pc
                        );
                        let max = U256::MAX;
                        let mut half = U256::MAX;
                        half.set_bit(31, false);
                        let h = &mut self.heuristics;
                        h.distance = {
                            // smallest distance from the `value` to U256::MAX and 0
                            if *cond > half {
                                max - cond + U256::from(1)
                            } else {
                                *cond
                            }
                        };
                    }

                    let dest = usize::try_from(counter).unwrap();
                    let cond = *cond != U256::ZERO;
                    update_heuritics!(pc, dest, cond);
                }
            }
            Some(op @ OpCode::BLOBHASH) => {
                let bug = Bug::new(BugType::BlockValueDependency, op.get(), pc, address_index);
                self.add_bug(bug);
            }
            Some(op @ OpCode::COINBASE) => {
                let bug = Bug::new(BugType::BlockValueDependency, op.get(), pc, address_index);
                self.add_bug(bug);
            }
            Some(op @ OpCode::TIMESTAMP) => {
                let bug = Bug::new(BugType::TimestampDependency, op.get(), pc, address_index);
                self.add_bug(bug);
            }
            Some(op @ OpCode::NUMBER) => {
                let bug = Bug::new(BugType::BlockNumberDependency, op.get(), pc, address_index);
                self.add_bug(bug);
            }
            Some(op @ OpCode::DIFFICULTY) => {
                let bug = Bug::new(BugType::BlockValueDependency, op.get(), pc, address_index);
                self.add_bug(bug);
            }
            Some(op @ (OpCode::REVERT | OpCode::INVALID)) => {
                let bug = Bug::new(BugType::RevertOrInvalid, op.get(), pc, address_index);
                self.add_bug(bug);
            }
            Some(op @ (OpCode::SELFDESTRUCT | OpCode::CREATE | OpCode::CREATE2)) => {
                let bug = Bug::new(BugType::Unclassified, op.get(), pc, address_index);
                self.add_bug(bug);
                if matches!(op, OpCode::CREATE | OpCode::CREATE2) {
                    if let Ok(created_address) = interp.stack.peek(0) {
                        let bytes: [u8; 32] = created_address.to_be_bytes();
                        let created_address = Address::from_slice(&bytes[12..]);
                        self.record_seen_address(created_address);
                    }
                }
            }
            Some(OpCode::KECCAK256) => {
                if self.instrument_config.record_sha3_mapping {
                    if let (Some(offset), Some(size), Ok(output)) = (
                        self.inputs.first(),
                        self.inputs.get(1),
                        interp.stack().peek(0),
                    ) {
                        let offset = offset.as_limbs()[0] as usize;
                        let size = size.as_limbs()[0] as usize;
                        let input = &interp.shared_memory.context_memory()[offset..offset + size];
                        // get only last 32 bytes
                        let last_32 = {
                            if input.len() > 32 {
                                &input[input.len() - 32..]
                            } else {
                                input
                            }
                        };
                        let output = H256::from_slice(&output.to_be_bytes::<32>());
                        self.heuristics.record_sha3_mapping(last_32, output);
                    }
                }
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
