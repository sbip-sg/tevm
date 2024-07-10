use hashbrown::{HashMap, HashSet};
use revm::{
    interpreter::{
        opcode::{self},
        CreateInputs, CreateOutcome, InstructionResult, Interpreter, OpCode,
    },
    primitives::{Address, U256},
    Database, EvmContext, Inspector,
};

use super::{Bug, BugData, BugType, Heuristics, InstrumentConfig};

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
    pub opcode_index: usize,
    /// Stack inputs of the current opcodes. Only updated when the opcode is interesting
    inputs: Vec<U256>,
}

impl BugInspector {
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
        let pcs = self.pcs_by_address.entry(address).or_insert(HashSet::new());
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
        let opcode = OpCode::new(interp.current_opcode());
        match opcode {
            Some(OpCode::ADD | OpCode::SUB | OpCode::MUL | OpCode::DIV | OpCode::SDIV) => {
                let a = interp.stack().peek(0).unwrap();
                let b = interp.stack().peek(1).unwrap();
                self.inputs.push(a);
                self.inputs.push(b);
            }
            _ => {}
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        let address = interp.contract().target_address;
        let address_index = self.record_seen_address(address);
        let pc = interp.program_counter();
        let opcode = interp.current_opcode();

        if self.instrument_config.pcs_by_address {
            self.record_pc(address.into(), pc);
        }

        // Check for revert or invalid opcode
        match &interp.instruction_result {
            InstructionResult::Revert | InstructionResult::InvalidEFOpcode => {
                let bug = Bug::new(BugType::RevertOrInvalid, opcode, pc, address_index);
                self.add_bug(bug);
            }
            _ => {}
        }

        // Check for overflow and underflow
        match opcode {
            opcode::ADD => {
                // todo_cl
            }
            _ => {}
        }
    }

    // #[inline]
    // fn call_end(
    //     &mut self,
    //     context: &mut EvmContext<DB>,
    //     inputs: &CallInputs,
    //     outcome: CallOutcome,
    // ) -> CallOutcome {

    //     outcome
    // }

    #[inline]
    fn create_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        let CreateOutcome { result, address } = outcome;
        if let Some(address) = address {
            if let Some(override_address) = self.create_address_overrides.get(&address) {
                return CreateOutcome::new(result, Some(*override_address));
            }
        }
        CreateOutcome::new(result, address)
    }
}
