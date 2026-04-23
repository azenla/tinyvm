use crate::machine::error::{MachineError, Result};
use crate::machine::ops::control::{CallOp, ExitOp, JumpIfEqualOp, JumpIfZeroOp, JumpOp, ReturnOp};
use crate::machine::ops::math::{
    AddOp, CountLeadingOnesOp, CountLeadingZerosOp, CountTrailingOnesOp, CountTrailingZerosOp,
    DivideOp, MultiplyOp, RemainderOp, SubtractOp,
};
use crate::machine::ops::stack::{PopOp, PushOp};
use crate::machine::{MachineLoopState, MachineState};
use crate::op::{Op, OpCode};
use crate::program::RawProgram;
use std::collections::HashMap;
use std::sync::Arc;

pub mod control;
pub mod math;
pub mod stack;

pub type GlobalOpHandler = Arc<Box<dyn OpHandler>>;

pub trait OpHandler {
    fn code(&self) -> OpCode;
    fn perform(&self, machine: &mut MachineState, op: &Op) -> Result<MachineLoopState>;
}

#[derive(Clone)]
pub struct OpHandlerSet {
    handlers: HashMap<OpCode, GlobalOpHandler>,
}

impl Default for OpHandlerSet {
    fn default() -> Self {
        Self::new()
    }
}

impl OpHandlerSet {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn add(&mut self, handler: impl OpHandler + 'static) {
        self.handlers
            .insert(handler.code(), Arc::new(Box::new(handler)));
    }

    pub fn get(&self, code: &OpCode) -> Option<&GlobalOpHandler> {
        self.handlers.get(code)
    }

    pub fn inline<'ops>(&self, program: &'ops RawProgram<'ops>) -> Result<InlinedOpHandlers<'ops>> {
        let mut output = Vec::with_capacity(program.ops().len());
        for op in program.ops() {
            let Some(handler) = self.get(&op.code) else {
                return Err(MachineError::InvalidOpCode);
            };
            output.push((op, handler.clone()));
        }
        Ok(InlinedOpHandlers::new(output))
    }
}

#[derive(Clone)]
pub struct InlinedOpHandlers<'op> {
    ops: Vec<(&'op Op, GlobalOpHandler)>,
}

impl<'op> InlinedOpHandlers<'op> {
    pub fn new(ops: Vec<(&'op Op, GlobalOpHandler)>) -> Self {
        Self { ops }
    }

    pub fn ops(&self) -> &[(&'op Op, GlobalOpHandler)] {
        &self.ops
    }
}

pub fn all() -> OpHandlerSet {
    let mut handlers = OpHandlerSet::new();
    handlers.add(PushOp);
    handlers.add(PopOp);
    handlers.add(AddOp);
    handlers.add(SubtractOp);
    handlers.add(MultiplyOp);
    handlers.add(DivideOp);
    handlers.add(RemainderOp);
    handlers.add(JumpIfEqualOp);
    handlers.add(JumpIfZeroOp);
    handlers.add(JumpOp);
    handlers.add(ExitOp);
    handlers.add(CallOp);
    handlers.add(ReturnOp);
    handlers.add(CountLeadingZerosOp);
    handlers.add(CountLeadingOnesOp);
    handlers.add(CountTrailingZerosOp);
    handlers.add(CountTrailingOnesOp);
    handlers
}
