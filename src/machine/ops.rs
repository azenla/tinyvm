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

pub mod control;
pub mod math;
pub mod stack;

pub type OpHandlerFunction = fn(&mut MachineState, op: &Op) -> Result<MachineLoopState>;

pub trait OpHandler: Default {
    fn code() -> OpCode;
    fn perform(machine: &mut MachineState, op: &Op) -> Result<MachineLoopState>;
}

#[derive(Clone)]
pub struct OpHandlerSet {
    handlers: Vec<Option<OpHandlerFunction>>,
}

impl Default for OpHandlerSet {
    fn default() -> Self {
        Self::new()
    }
}

impl OpHandlerSet {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn add<H: OpHandler>(&mut self) {
        let code = H::code() as u8;
        if self.handlers.len() <= code as usize {
            self.handlers.resize(code as usize + 1, None);
        }
        self.handlers[code as usize] = Some(H::perform);
    }

    pub fn get(&self, code: &OpCode) -> Option<&OpHandlerFunction> {
        self.handlers
            .get(*code as u8 as usize)
            .and_then(|item| if item.is_some() { item.as_ref() } else { None })
    }

    pub fn inline<'ops>(&self, program: &'ops RawProgram<'ops>) -> Result<InlinedOpHandlers<'ops>> {
        let mut output = Vec::with_capacity(program.ops().len());
        for op in program.ops() {
            let Some(handler) = self.get(&op.code) else {
                return Err(MachineError::InvalidOpCode);
            };
            output.push((op, *handler));
        }
        Ok(InlinedOpHandlers::new(output))
    }
}

#[derive(Clone)]
pub struct InlinedOpHandlers<'op> {
    ops: Vec<(&'op Op, OpHandlerFunction)>,
}

impl<'op> InlinedOpHandlers<'op> {
    pub fn new(ops: Vec<(&'op Op, OpHandlerFunction)>) -> Self {
        Self { ops }
    }

    pub fn ops(&self) -> &[(&'op Op, OpHandlerFunction)] {
        &self.ops
    }
}

pub fn all() -> OpHandlerSet {
    let mut handlers = OpHandlerSet::new();
    handlers.add::<PushOp>();
    handlers.add::<PopOp>();
    handlers.add::<AddOp>();
    handlers.add::<SubtractOp>();
    handlers.add::<MultiplyOp>();
    handlers.add::<DivideOp>();
    handlers.add::<RemainderOp>();
    handlers.add::<JumpIfEqualOp>();
    handlers.add::<JumpIfZeroOp>();
    handlers.add::<JumpOp>();
    handlers.add::<ExitOp>();
    handlers.add::<CallOp>();
    handlers.add::<ReturnOp>();
    handlers.add::<CountLeadingZerosOp>();
    handlers.add::<CountLeadingOnesOp>();
    handlers.add::<CountTrailingZerosOp>();
    handlers.add::<CountTrailingOnesOp>();
    handlers
}
