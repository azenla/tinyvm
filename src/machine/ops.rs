use crate::machine::error::Result;
use crate::machine::{Machine, MachineLoopState};
use crate::op::Op;

pub mod control;
pub mod math;
pub mod stack;

pub trait MachineOp {
    fn perform(machine: &mut Machine, op: &Op) -> Result<MachineLoopState>;
}
