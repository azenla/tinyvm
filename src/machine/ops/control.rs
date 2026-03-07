use crate::machine::error::Result;
use crate::machine::ops::MachineOp;
use crate::machine::{Machine, MachineLoopState};
use crate::op::Op;

pub struct JumpOp;

impl MachineOp for JumpOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState> {
        machine.jmp(op)?;
        Ok(MachineLoopState::Stay)
    }
}

pub struct JumpIfEqualOp;

impl MachineOp for JumpIfEqualOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        if value1 == value2 {
            machine.jmp(op)?;
            return Ok(MachineLoopState::Stay);
        }
        Ok(MachineLoopState::Next)
    }
}

pub struct JumpIfZeroOp;

impl MachineOp for JumpIfZeroOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        if value.is_zero() {
            machine.jmp(op)?;
            return Ok(MachineLoopState::Stay);
        }
        Ok(MachineLoopState::Next)
    }
}

pub struct ExitOp;

impl MachineOp for ExitOp {
    fn perform(_machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        Ok(MachineLoopState::Break)
    }
}

pub struct CallOp;

impl MachineOp for CallOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState> {
        machine.call(op)?;
        Ok(MachineLoopState::Stay)
    }
}

pub struct ReturnOp;

impl MachineOp for ReturnOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        machine.ret()?;
        Ok(MachineLoopState::Next)
    }
}
