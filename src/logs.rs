use crate::CALL_DEPTH;
use hashbrown::HashMap;
use lazy_static::lazy_static;
use revm::{
    interpreter::{
        CallInputs, CallOutcome, CallScheme, CallValue, CreateInputs, CreateOutcome,
        InstructionResult,
    },
    primitives::{Address, Bytes, Log as EvmLog, B256, U256},
    Database, EvmContext, Inspector,
};
use std::cell::Cell;
use thread_local::ThreadLocal;

lazy_static! {
    static ref COUNTER: ThreadLocal<Cell<usize>> = ThreadLocal::new();
}

#[derive(Debug, Clone)]
pub struct CallTrace {
    pub from: Address,
    pub to: Address,
    pub value: U256,
    pub input: Bytes,
    pub depth: usize,
    pub return_data: Option<Bytes>,
    pub is_static: bool,
    pub status: Option<InstructionResult>,
    pub id: usize,
}

#[derive(Debug, Clone)]
pub struct Log {
    pub id: usize,
    pub depth: usize,
    pub address: Address,
    pub topics: Vec<B256>,
    pub data: Bytes,
}

/// An inspector that collects call traces.
#[derive(Debug, Default)]
pub struct LogInspector {
    /// Traced enabled?
    pub trace_enabled: bool,
    /// The collected traces
    pub traces: Vec<CallTrace>,
    /// EVM events/logs collected during execution
    pub logs: Vec<Log>,
}

impl<DB> Inspector<DB> for LogInspector
where
    DB: Database,
{
    #[inline]
    fn log(&mut self, _context: &mut EvmContext<DB>, evm_log: &EvmLog) {
        if !self.trace_enabled {
            return;
        }
        let cell = COUNTER.get_or_default();
        let id = cell.get();
        cell.set(cell.get() + 1);
        let depth = CALL_DEPTH.get_or_default().get();
        self.logs.push(Log {
            id,
            depth,
            address: evm_log.address,
            topics: evm_log.topics().to_vec(),
            data: evm_log.data.data.clone(),
        });
    }

    #[inline]
    fn call(
        &mut self,
        _context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        if self.trace_enabled {
            let is_static = !matches!(inputs.scheme, CallScheme::Call | CallScheme::CallCode);
            let (from, to) = match inputs.scheme {
                CallScheme::DelegateCall | CallScheme::CallCode => {
                    (inputs.target_address, inputs.bytecode_address)
                }
                _ => (inputs.caller, inputs.target_address),
            };

            let cell = COUNTER.get_or_default();
            let id = cell.get();
            cell.set(id + 1);

            let cell = CALL_DEPTH.get_or_default();
            let depth = cell.get();
            cell.set(depth + 1);

            let value = match inputs.value {
                CallValue::Transfer(value) => value,
                _ => U256::ZERO, // double check this
            };

            let trace = CallTrace {
                id,
                from,
                to,
                value,
                input: inputs.input.clone(),
                depth,
                return_data: None,
                is_static,
                status: None,
            };

            self.traces.push(trace);
        }
        None
    }

    #[inline]
    fn call_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CallInputs,
        result: CallOutcome,
    ) -> CallOutcome {
        if self.trace_enabled {
            let cell = CALL_DEPTH.get_or_default();
            cell.set(cell.get() - 1);
            let depth = cell.get();
            let call_trace = self
                .traces
                .iter_mut()
                .find(|c| c.depth == depth)
                .expect("Bad state: Call end without start?");
            call_trace.return_data = Some(result.output().clone());
            call_trace.status = Some(result.result.result);
        }

        result
    }
}
