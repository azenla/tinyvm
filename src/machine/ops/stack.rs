use crate::machine::error::MachineError;
use crate::machine::error::Result;
use crate::machine::ops::MachineOp;
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineLoopState};
use crate::op::Op;

pub struct PushOp;

impl MachineOp for PushOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState> {
        let value = MachineValue::of(op.arg, &machine.bank).ok_or(MachineError::ValueExpected)?;
        machine.push(value);
        Ok(MachineLoopState::Next)
    }
}

pub struct PopOp;

impl MachineOp for PopOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.bank.store(op.arg, value)?;
        Ok(MachineLoopState::Next)
    }
}
