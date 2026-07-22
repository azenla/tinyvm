use crate::machine::error::Result;
use crate::machine::ops::OpHandler;
use crate::machine::{MachineLoopState, MachineState};
use crate::op::{Op, OpCode};

#[derive(Default)]
pub struct AddOp;

impl OpHandler for AddOp {
    fn code() -> OpCode {
        OpCode::Add
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 + value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct SubtractOp;

impl OpHandler for SubtractOp {
    fn code() -> OpCode {
        OpCode::Subtract
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 - value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct MultiplyOp;

impl OpHandler for MultiplyOp {
    fn code() -> OpCode {
        OpCode::Multiply
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 * value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct DivideOp;

impl OpHandler for DivideOp {
    fn code() -> OpCode {
        OpCode::Divide
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 / value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct RemainderOp;

impl OpHandler for RemainderOp {
    fn code() -> OpCode {
        OpCode::Remainder
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 % value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct CountLeadingZerosOp;

impl OpHandler for CountLeadingZerosOp {
    fn code() -> OpCode {
        OpCode::CountLeadingZeros
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.leading_zeros());
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct CountLeadingOnesOp;

impl OpHandler for CountLeadingOnesOp {
    fn code() -> OpCode {
        OpCode::CountLeadingOnes
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.leading_ones());
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct CountTrailingZerosOp;

impl OpHandler for CountTrailingZerosOp {
    fn code() -> OpCode {
        OpCode::CountTrailingZeros
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.trailing_zeros());
        Ok(MachineLoopState::Next)
    }
}

#[derive(Default)]
pub struct CountTrailingOnesOp;

impl OpHandler for CountTrailingOnesOp {
    fn code() -> OpCode {
        OpCode::CountTrailingOnes
    }

    fn perform(machine: &mut MachineState, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.trailing_ones());
        Ok(MachineLoopState::Next)
    }
}
