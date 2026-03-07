use std::error::Error;
use std::fmt::{Display, Formatter, Result as FmtResult};

pub type Result<T> = std::result::Result<T, MachineError>;

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum MachineError {
    StackEmpty,
    InstructionExpected,
    InstructionOverflow,
    ValueExpected,
    RegisterExpected,
    CallStackEmpty,
}

impl Display for MachineError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::StackEmpty => write!(f, "stack empty"),
            MachineError::InstructionExpected => write!(f, "instruction expected"),
            MachineError::InstructionOverflow => write!(f, "instruction overflow"),
            MachineError::ValueExpected => write!(f, "value expected"),
            MachineError::RegisterExpected => write!(f, "register expected"),
            MachineError::CallStackEmpty => write!(f, "call stack empty"),
        }
    }
}

impl Error for MachineError {}
