use crate::machine::error::MachineError;
use crate::machine::error::Result;
use crate::machine::ops::OpHandler;
use crate::machine::value::MachineValue;
use crate::machine::{MachineLoopState, MachineState};
use crate::op::{Op, OpCode};

pub struct PushOp;

impl OpHandler for PushOp {
    fn code(&self) -> OpCode {
        OpCode::Push
    }

    fn perform(&self, machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        let value = MachineValue::of(op.arg, &machine.bank).ok_or(MachineError::ValueExpected)?;
        machine.push(value);
        Ok(MachineLoopState::Next)
    }
}

pub struct PopOp;

impl OpHandler for PopOp {
    fn code(&self) -> OpCode {
        OpCode::Pop
    }

    fn perform(&self, machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.bank.store(op.arg, value)?;
        Ok(MachineLoopState::Next)
    }
}
