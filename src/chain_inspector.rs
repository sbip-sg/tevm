use revm::interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome};
use revm::primitives::Log;
use revm::{Database, context::Context as EvmContext, Inspector, interpreter::Interpreter};

use crate::instrument::bug_inspector::BugInspector;
use crate::instrument::log_inspector::LogInspector;

/// A chain of inspectors, ecch inspector will be executed in order.
pub struct ChainInspector {
    pub log_inspector: Option<LogInspector>,
    pub bug_inspector: Option<BugInspector>,
}

impl<DB: Database> Inspector<DB> for ChainInspector {
    #[inline] 
    fn step(&mut self, interp: &mut Interpreter, db: &mut DB) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.step(interp, db);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.step(interp, db);
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, db: &mut DB) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.step_end(interp, db);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.step_end(interp, db);
        }
    }

    #[inline]
    fn log(&mut self, interp: &mut Interpreter, db: &mut DB, log: Log) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.log(interp, db, log.clone());
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.log(interp, db, log);
        }
    }

    /// Call the inspectors in order, if any of them returns a `Some`, return that value.
    /// If all of them return `None`, the execution will continue normally.
    #[inline]
    fn call(
        &mut self,
        db: &mut DB,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.call(db, inputs);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.call(db, inputs)
        } else {
            None
        }
    }

    #[inline]
    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        let mut outcome = outcome;
        if let Some(ins) = self.log_inspector.as_mut() {
            outcome = ins.call_end(context, inputs, outcome);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            outcome = ins.call_end(context, inputs, outcome);
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
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.create(context, inputs);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.create(context, inputs)
        } else {
            None
        }
    }

    #[inline]
    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        let mut outcome = outcome;
        if let Some(ins) = self.log_inspector.as_mut() {
            outcome = ins.create_end(context, inputs, outcome);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            outcome = ins.create_end(context, inputs, outcome);
        }
        outcome
    }
}
