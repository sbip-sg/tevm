use revm::interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs};
use revm::primitives::{Address, Log, U256};
use revm::{interpreter::Interpreter, Database, EvmContext, Inspector};

/// A chain of inspectors, ecch inspector will be executed in order.
pub struct ChainInspector<DB: Database> {
    /// The inspector which modifies the execution should be placed at the end of the chain.
    pub inspectors: Vec<Box<dyn Inspector<DB>>>,
}

impl<DB: Database> Default for ChainInspector<DB> {
    fn default() -> Self {
        Self {
            inspectors: Vec::new(),
        }
    }
}

impl<DB: Database> ChainInspector<DB> {
    pub fn add<I: Inspector<DB> + 'static>(&mut self, inspector: I) {
        self.inspectors.push(Box::new(inspector));
    }
}

impl<DB: Database> Inspector<DB> for ChainInspector<DB> {
    #[inline]
    fn initialize_interp(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        for inspector in self.inspectors.iter_mut() {
            inspector.initialize_interp(interp, context);
        }
    }

    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        for inspector in self.inspectors.iter_mut() {
            inspector.step(interp, context);
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        for inspector in self.inspectors.iter_mut() {
            inspector.step_end(interp, context);
        }
    }

    #[inline]
    fn log(&mut self, context: &mut EvmContext<DB>, log: &Log) {
        for inspector in self.inspectors.iter_mut() {
            inspector.log(context, log);
        }
    }

    /// Call the inspectors in order, if any of them returns a `Some`, return that value.
    /// If all of them return `None`, the execution will continue normally.
    #[inline]
    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        for inspector in self.inspectors.iter_mut() {
            if let Some(outcome) = inspector.call(context, inputs) {
                return Some(outcome);
            }
        }
        None
    }

    #[inline]
    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        let mut outcome = outcome;
        for inspector in self.inspectors.iter_mut() {
            outcome = inspector.call_end(context, inputs, outcome);
        }
        outcome
    }

    /// Call the inspectors in order, if any of them returns a `Some`, return that value.
    #[inline]
    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        for inspector in self.inspectors.iter_mut() {
            if let Some(outcome) = inspector.create(context, inputs) {
                return Some(outcome);
            }
        }
        None
    }

    #[inline]
    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        let mut outcome = outcome;
        for inspector in self.inspectors.iter_mut() {
            outcome = inspector.create_end(context, inputs, outcome);
        }
        outcome
    }

    fn eofcreate(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        for inspector in self.inspectors.iter_mut() {
            if let Some(outcome) = inspector.eofcreate(context, inputs) {
                return Some(outcome);
            }
        }
        None
    }

    fn eofcreate_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &EOFCreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        let mut outcome = outcome;
        for inspector in self.inspectors.iter_mut() {
            outcome = inspector.eofcreate_end(context, inputs, outcome);
        }
        outcome
    }

    #[inline]
    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        for inspector in self.inspectors.iter_mut() {
            inspector.selfdestruct(contract, target, value);
        }
    }
}
