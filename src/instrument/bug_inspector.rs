use hashbrown::HashMap;
use revm::{
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter},
    primitives::{Address, Log as EvmLog},
    Database, EvmContext, Inspector,
};

pub struct BugInspector {
    /// Change the created address to another address
    pub create_address_overrides: HashMap<Address, Address>,
}

impl<DB> Inspector<DB> for BugInspector
where
    DB: Database,
{
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        let _ = interp;
        let _ = context;
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        let _ = interp;
        let _ = context;
    }

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
