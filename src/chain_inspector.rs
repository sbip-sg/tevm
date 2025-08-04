use revm::interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter};
use revm::primitives::Log;
use revm::inspector::Inspector;

use crate::instrument::bug_inspector::BugInspector;
use crate::instrument::log_inspector::LogInspector;

/// A chain of inspectors, ecch inspector will be executed in order.
pub struct ChainInspector {
    pub log_inspector: Option<LogInspector>,
    pub bug_inspector: Option<BugInspector>,
}

impl<CTX> Inspector<CTX> for ChainInspector 
where
    CTX: revm::context_interface::ContextTr,
    CTX::Journal: revm::inspector::JournalExt,
{
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.step(interp, context);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.step(interp, context);
        }
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut CTX) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.step_end(interp, context);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.step_end(interp, context);
        }
    }

    #[inline]
    fn log(&mut self, interp: &mut Interpreter, context: &mut CTX, log: Log) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.log(interp, context, log.clone());
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.log(interp, context, log);
        }
    }

    /// Call the inspectors in order, if any of them returns a `Some`, return that value.
    /// If all of them return `None`, the execution will continue normally.
    #[inline]
    fn call(
        &mut self,
        context: &mut CTX,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.call(context, inputs);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.call(context, inputs)
        } else {
            None
        }
    }

    #[inline]
    fn call_end(
        &mut self,
        context: &mut CTX,
        inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.call_end(context, inputs, outcome);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.call_end(context, inputs, outcome);
        }
    }

    /// Call the inspectors in order, if any of them returns a `Some`, return that value.
    #[inline]
    fn create(
        &mut self,
        context: &mut CTX,
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
        context: &mut CTX,
        inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        if let Some(ins) = self.log_inspector.as_mut() {
            ins.create_end(context, inputs, outcome);
        }
        if let Some(ins) = self.bug_inspector.as_mut() {
            ins.create_end(context, inputs, outcome);
        }
    }
}
