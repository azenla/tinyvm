use crate::machine::error::Result;
use crate::machine::ops::MachineOp;
use crate::machine::{Machine, MachineLoopState};
use crate::op::Op;

pub struct AddOp;

impl MachineOp for AddOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 + value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct SubtractOp;

impl MachineOp for SubtractOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 - value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct MultiplyOp;

impl MachineOp for MultiplyOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 * value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct DivideOp;

impl MachineOp for DivideOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 / value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct RemainderOp;

impl MachineOp for RemainderOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value1 = machine.pop()?;
        let value2 = machine.pop()?;
        let result = value2 % value1;
        machine.push(result);
        Ok(MachineLoopState::Next)
    }
}

pub struct CountLeadingZerosOp;

impl MachineOp for CountLeadingZerosOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.leading_zeros());
        Ok(MachineLoopState::Next)
    }
}

pub struct CountLeadingOnesOp;

impl MachineOp for CountLeadingOnesOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.leading_ones());
        Ok(MachineLoopState::Next)
    }
}

pub struct CountTrailingZerosOp;

impl MachineOp for CountTrailingZerosOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.trailing_zeros());
        Ok(MachineLoopState::Next)
    }
}

pub struct CountTrailingOnesOp;

impl MachineOp for CountTrailingOnesOp {
    fn perform(machine: &mut Machine, _op: &Op) -> Result<MachineLoopState> {
        let value = machine.pop()?;
        machine.push(value.trailing_ones());
        Ok(MachineLoopState::Next)
    }
}
