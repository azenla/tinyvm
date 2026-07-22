use crate::machine::error::Result;
use crate::machine::ops::OpHandler;
use crate::machine::{MachineLoopState, MachineState};
use crate::op::{Op, OpCode};

#[derive(Default)]
pub struct JumpOp;

impl OpHandler for JumpOp {
    fn code() -> OpCode {
        OpCode::Jump
    }

    fn perform(machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        machine.jmp(op)?;
        Ok(MachineLoopState::Stay)
    }
}

#[derive(Default)]
pub struct JumpIfEqualOp;

impl OpHandler for JumpIfEqualOp {
    fn code() -> OpCode {
        OpCode::JumpIfEqual
    }

    fn perform(machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        if value1 == value2 {
            machine.jmp(op)?;
            return Ok(MachineLoopState::Stay);
        }
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct JumpIfZeroOp;

impl OpHandler for JumpIfZeroOp {
    fn code() -> OpCode {
        OpCode::JumpIfZero
    }

    fn perform(machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        if value.is_zero() {
            machine.jmp(op)?;
            return Ok(MachineLoopState::Stay);
        }
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct ExitOp;

impl OpHandler for ExitOp {
    fn code() -> OpCode {
        OpCode::Exit
    }

    fn perform(_machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        Ok(MachineLoopState::Break)
    }
}

#[derive(Default)]
pub struct CallOp;

impl OpHandler for CallOp {
    fn code() -> OpCode {
        OpCode::Call
    }

    fn perform(machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        machine.call(op)?;
        Ok(MachineLoopState::Stay)
    }
}

#[derive(Default)]
pub struct ReturnOp;

impl OpHandler for ReturnOp {
    fn code() -> OpCode {
        OpCode::Return
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        machine.ret()?;
        Ok(MachineLoopState::Stay)
    }
}
