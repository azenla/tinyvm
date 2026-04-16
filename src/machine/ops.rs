use crate::machine::error::Result;
use crate::machine::ops::control::{CallOp, ExitOp, JumpIfEqualOp, JumpIfZeroOp, JumpOp, ReturnOp};
use crate::machine::ops::math::{
    AddOp, CountLeadingOnesOp, CountLeadingZerosOp, CountTrailingOnesOp, CountTrailingZerosOp,
    DivideOp, MultiplyOp, RemainderOp, SubtractOp,
};
use crate::machine::ops::stack::{PopOp, PushOp};
use crate::machine::{Machine, MachineLoopState};
use crate::op::{Op, OpCode};

pub mod control;
pub mod math;
pub mod stack;

pub trait MachineOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState>;
}

pub fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState> {
    match op.code {
        OpCode::Push => PushOp::perform(machine, op),

        OpCode::Pop => PopOp::perform(machine, op),

        OpCode::Add => AddOp::perform(machine, op),

        OpCode::Subtract => SubtractOp::perform(machine, op),

        OpCode::Multiply => MultiplyOp::perform(machine, op),

        OpCode::Divide => DivideOp::perform(machine, op),

        OpCode::Remainder => RemainderOp::perform(machine, op),

        OpCode::JumpIfEqual => JumpIfEqualOp::perform(machine, op),

        OpCode::Jump => JumpOp::perform(machine, op),

        OpCode::JumpIfZero => JumpIfZeroOp::perform(machine, op),

        OpCode::Exit => ExitOp::perform(machine, op),

        OpCode::Call => CallOp::perform(machine, op),

        OpCode::Return => ReturnOp::perform(machine, op),

        OpCode::CountLeadingZeros => CountLeadingZerosOp::perform(machine, op),

        OpCode::CountLeadingOnes => CountLeadingOnesOp::perform(machine, op),

        OpCode::CountTrailingZeros => CountTrailingZerosOp::perform(machine, op),

        OpCode::CountTrailingOnes => CountTrailingOnesOp::perform(machine, op),
    }
}
