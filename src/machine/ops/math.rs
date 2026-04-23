use crate::machine::error::Result;
use crate::machine::ops::OpHandler;
use crate::machine::{MachineLoopState, MachineState};
use crate::op::{Op, OpCode};

pub struct AddOp;

impl OpHandler for AddOp {
    fn code(&self) -> OpCode {
        OpCode::Add
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 + value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct SubtractOp;

impl OpHandler for SubtractOp {
    fn code(&self) -> OpCode {
        OpCode::Subtract
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 - value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct MultiplyOp;

impl OpHandler for MultiplyOp {
    fn code(&self) -> OpCode {
        OpCode::Multiply
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 * value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct DivideOp;

impl OpHandler for DivideOp {
    fn code(&self) -> OpCode {
        OpCode::Divide
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 / value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct RemainderOp;

impl OpHandler for RemainderOp {
    fn code(&self) -> OpCode {
        OpCode::Remainder
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 % value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct CountLeadingZerosOp;

impl OpHandler for CountLeadingZerosOp {
    fn code(&self) -> OpCode {
        OpCode::CountLeadingZeros
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.leading_zeros());
        Ok(MachineLoopState::Next)
    }
}

pub struct CountLeadingOnesOp;

impl OpHandler for CountLeadingOnesOp {
    fn code(&self) -> OpCode {
        OpCode::CountLeadingOnes
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.leading_ones());
        Ok(MachineLoopState::Next)
    }
}

pub struct CountTrailingZerosOp;

impl OpHandler for CountTrailingZerosOp {
    fn code(&self) -> OpCode {
        OpCode::CountTrailingZeros
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.trailing_zeros());
        Ok(MachineLoopState::Next)
    }
}

pub struct CountTrailingOnesOp;

impl OpHandler for CountTrailingOnesOp {
    fn code(&self) -> OpCode {
        OpCode::CountTrailingOnes
    }

    fn perform(&self, machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.trailing_ones());
        Ok(MachineLoopState::Next)
    }
}
