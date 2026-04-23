use crate::machine::error::Result;
use crate::machine::ops::OpHandler;
use crate::machine::{MachineLoopState, MachineState};
use crate::op::{Op, OpCode};

pub struct JumpOp;

impl OpHandler for JumpOp {
    fn code(&self) -> OpCode {
        OpCode::Jump
    }

    fn perform(&self, machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        machine.jmp(op)?;
        Ok(MachineLoopState::Stay)
    }
}

pub struct JumpIfEqualOp;

impl OpHandler for JumpIfEqualOp {
    fn code(&self) -> OpCode {
        OpCode::JumpIfEqual
    }

    fn perform(&self, machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
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

impl OpHandler for JumpIfZeroOp {
    fn code(&self) -> OpCode {
        OpCode::JumpIfZero
    }

    fn perform(&self, machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        if value.is_zero() {
            machine.jmp(op)?;
            return Ok(MachineLoopState::Stay);
        }
        Ok(MachineLoopState::Next)
    }
}

pub struct ExitOp;

impl OpHandler for ExitOp {
    fn code(&self) -> OpCode {
        OpCode::Exit
    }

    fn perform(&self, _machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        Ok(MachineLoopState::Break)
    }
}

pub struct CallOp;

impl OpHandler for CallOp {
    fn code(&self) -> OpCode {
        OpCode::Call
    }

    fn perform(&self, machine: &mut MachineState, op: &Op) -> Result<MachineLoopState> {
        machine.call(op)?;
        Ok(MachineLoopState::Stay)
    }
}

pub struct ReturnOp;

impl OpHandler for ReturnOp {
    fn code(&self) -> OpCode {
        OpCode::Return
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        machine.ret()?;
        Ok(MachineLoopState::Next)
    }
}
