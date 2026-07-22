use crate::machine::MachineState;
use crate::machine::error::{MachineError, Result};
use crate::machine::value::MachineValue;
use crate::op::{Op, OpArg, OpCode};
use crate::program::RawProgram;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompiledOp {
    PushValue(MachineValue),
    PushRegister(usize),
    PopRegister(usize),
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    JumpIfEqual(usize),
    JumpIfZero(usize),
    Jump(usize),
    Call(usize),
    Return,
    Exit,
    CountLeadingZeros,
    CountLeadingOnes,
    CountTrailingZeros,
    CountTrailingOnes,
}

impl CompiledOp {
    pub fn compile(op: &Op) -> Result<Self> {
        Ok(match op.code {
            OpCode::Push => match op.arg.register_index() {
                Some(index) => CompiledOp::PushRegister(index),
                None => match op.arg {
                    OpArg::Instruction(_) => return Err(MachineError::ValueExpected),
                    arg => CompiledOp::PushValue(MachineValue::from(arg)),
                },
            },
            OpCode::Pop => CompiledOp::PopRegister(
                op.arg
                    .register_index()
                    .ok_or(MachineError::RegisterExpected)?,
            ),
            OpCode::Add => CompiledOp::Add,
            OpCode::Subtract => CompiledOp::Subtract,
            OpCode::Multiply => CompiledOp::Multiply,
            OpCode::Divide => CompiledOp::Divide,
            OpCode::Remainder => CompiledOp::Remainder,
            OpCode::JumpIfEqual => CompiledOp::JumpIfEqual(instruction_target(op)?),
            OpCode::JumpIfZero => CompiledOp::JumpIfZero(instruction_target(op)?),
            OpCode::Jump => CompiledOp::Jump(instruction_target(op)?),
            OpCode::Call => CompiledOp::Call(instruction_target(op)?),
            OpCode::Return => CompiledOp::Return,
            OpCode::Exit => CompiledOp::Exit,
            OpCode::CountLeadingZeros => CompiledOp::CountLeadingZeros,
            OpCode::CountLeadingOnes => CompiledOp::CountLeadingOnes,
            OpCode::CountTrailingZeros => CompiledOp::CountTrailingZeros,
            OpCode::CountTrailingOnes => CompiledOp::CountTrailingOnes,
        })
    }

    #[inline(always)]
    pub(crate) fn perform(&self, machine: &mut MachineState, pc: usize) -> Result<Option<usize>> {
        Ok(match *self {
            CompiledOp::PushValue(value) => {
                machine.push(value);
                Some(pc + 1)
            }

            CompiledOp::PushRegister(index) => {
                machine.push(machine.bank.get(index));
                Some(pc + 1)
            }

            CompiledOp::PopRegister(index) => {
                let value = machine.pop()?;
                machine.bank.set(index, value);
                Some(pc + 1)
            }

            CompiledOp::Add => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 + value1);
                Some(pc + 1)
            }

            CompiledOp::Subtract => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 - value1);
                Some(pc + 1)
            }

            CompiledOp::Multiply => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 * value1);
                Some(pc + 1)
            }

            CompiledOp::Divide => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 / value1);
                Some(pc + 1)
            }

            CompiledOp::Remainder => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 % value1);
                Some(pc + 1)
            }

            CompiledOp::JumpIfEqual(target) => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                if value1 == value2 {
                    Some(target)
                } else {
                    Some(pc + 1)
                }
            }

            CompiledOp::JumpIfZero(target) => {
                let value = machine.pop()?;
                if value.is_zero() {
                    Some(target)
                } else {
                    Some(pc + 1)
                }
            }

            CompiledOp::Jump(target) => Some(target),

            CompiledOp::Call(target) => {
                machine.calls.push(MachineValue::ReturnAddress(pc + 1));
                Some(target)
            }

            CompiledOp::Return => {
                let value = machine.calls.pop().ok_or(MachineError::CallStackEmpty)?;
                match value {
                    MachineValue::ReturnAddress(target) => Some(target),
                    _ => return Err(MachineError::InstructionExpected),
                }
            }

            CompiledOp::Exit => None,

            CompiledOp::CountLeadingZeros => {
                let value = machine.pop()?;
                machine.push(value.leading_zeros());
                Some(pc + 1)
            }

            CompiledOp::CountLeadingOnes => {
                let value = machine.pop()?;
                machine.push(value.leading_ones());
                Some(pc + 1)
            }

            CompiledOp::CountTrailingZeros => {
                let value = machine.pop()?;
                machine.push(value.trailing_zeros());
                Some(pc + 1)
            }

            CompiledOp::CountTrailingOnes => {
                let value = machine.pop()?;
                machine.push(value.trailing_ones());
                Some(pc + 1)
            }
        })
    }
}

fn instruction_target(op: &Op) -> Result<usize> {
    match op.arg {
        OpArg::Instruction(target) => Ok(target as usize),
        _ => Err(MachineError::InstructionExpected),
    }
}

#[derive(Clone, Debug)]
pub struct CompiledProgram {
    ops: Vec<CompiledOp>,
}

impl CompiledProgram {
    pub fn compile(program: &RawProgram) -> Result<Self> {
        let mut ops = Vec::with_capacity(program.ops().len());
        for op in program.ops() {
            ops.push(CompiledOp::compile(op)?);
        }
        Ok(Self { ops })
    }

    pub fn ops(&self) -> &[CompiledOp] {
        &self.ops
    }
}
