use crate::machine::RegisterBank;
use crate::machine::value::MachineValue;
use crate::op::OpArg;
use std::ops::{Add, Div, Mul, Sub};

impl MachineValue {
    pub fn of(arg: OpArg, bank: &RegisterBank) -> Option<Self> {
        match arg {
            OpArg::Register1
            | OpArg::Register2
            | OpArg::Register3
            | OpArg::Register4
            | OpArg::Register5
            | OpArg::Register6
            | OpArg::Register7
            | OpArg::Register8
            | OpArg::Register9 => bank.load(arg),

            OpArg::Uint32(value) => Some(MachineValue::Uint32(value)),
            OpArg::Uint64(value) => Some(MachineValue::Uint64(value)),
            OpArg::Int32(value) => Some(MachineValue::Int32(value)),
            OpArg::Int64(value) => Some(MachineValue::Int64(value)),

            OpArg::None => Some(MachineValue::None),
            OpArg::Uint8(value) => Some(MachineValue::Uint8(value)),
            OpArg::Uint16(value) => Some(MachineValue::Uint16(value)),
            OpArg::Int8(value) => Some(MachineValue::Int8(value)),
            OpArg::Int16(value) => Some(MachineValue::Int16(value)),
            OpArg::Instruction(_) => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            MachineValue::Uint32(value) => value as u8,
            MachineValue::Uint64(value) => value as u8,
            MachineValue::Int32(value) => value as u8,
            MachineValue::Int64(value) => value as u8,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value,
            MachineValue::Uint16(value) => value as u8,
            MachineValue::Int8(value) => value as u8,
            MachineValue::Int16(value) => value as u8,
            MachineValue::ReturnAddress(value) => value as u8,
        }
    }

    pub fn as_u16(self) -> u16 {
        match self {
            MachineValue::Uint32(value) => value as u16,
            MachineValue::Uint64(value) => value as u16,
            MachineValue::Int32(value) => value as u16,
            MachineValue::Int64(value) => value as u16,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value as u16,
            MachineValue::Uint16(value) => value,
            MachineValue::Int8(value) => value as u16,
            MachineValue::Int16(value) => value as u16,
            MachineValue::ReturnAddress(value) => value as u16,
        }
    }

    pub fn as_u32(self) -> u32 {
        match self {
            MachineValue::Uint32(value) => value,
            MachineValue::Uint64(value) => value as u32,
            MachineValue::Int32(value) => value as u32,
            MachineValue::Int64(value) => value as u32,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value as u32,
            MachineValue::Uint16(value) => value as u32,
            MachineValue::Int8(value) => value as u32,
            MachineValue::Int16(value) => value as u32,
            MachineValue::ReturnAddress(value) => value as u32,
        }
    }

    pub fn as_u64(self) -> u64 {
        match self {
            MachineValue::Uint32(value) => value as u64,
            MachineValue::Int32(value) => value as u64,
            MachineValue::Int64(value) => value as u64,
            MachineValue::Uint64(value) => value,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value as u64,
            MachineValue::Uint16(value) => value as u64,
            MachineValue::Int8(value) => value as u64,
            MachineValue::Int16(value) => value as u64,
            MachineValue::ReturnAddress(value) => value as u64,
        }
    }

    pub fn as_i8(self) -> i8 {
        match self {
            MachineValue::Uint32(value) => value as i8,
            MachineValue::Uint64(value) => value as i8,
            MachineValue::Int32(value) => value as i8,
            MachineValue::Int64(value) => value as i8,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value as i8,
            MachineValue::Uint16(value) => value as i8,
            MachineValue::Int8(value) => value,
            MachineValue::Int16(value) => value as i8,
            MachineValue::ReturnAddress(value) => value as i8,
        }
    }

    pub fn as_i16(self) -> i16 {
        match self {
            MachineValue::Uint32(value) => value as i16,
            MachineValue::Uint64(value) => value as i16,
            MachineValue::Int32(value) => value as i16,
            MachineValue::Int64(value) => value as i16,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value as i16,
            MachineValue::Uint16(value) => value as i16,
            MachineValue::Int8(value) => value as i16,
            MachineValue::Int16(value) => value,
            MachineValue::ReturnAddress(value) => value as i16,
        }
    }

    pub fn as_i32(self) -> i32 {
        match self {
            MachineValue::Uint32(value) => value as i32,
            MachineValue::Int32(value) => value,
            MachineValue::Int64(value) => value as i32,
            MachineValue::Uint64(value) => value as i32,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value as i32,
            MachineValue::Uint16(value) => value as i32,
            MachineValue::Int8(value) => value as i32,
            MachineValue::Int16(value) => value as i32,
            MachineValue::ReturnAddress(value) => value as i32,
        }
    }

    pub fn as_i64(self) -> i64 {
        match self {
            MachineValue::Uint32(value) => value as i64,
            MachineValue::Int32(value) => value as i64,
            MachineValue::Int64(value) => value,

            MachineValue::None => 0,
            MachineValue::Uint8(value) => value as i64,
            MachineValue::Uint16(value) => value as i64,
            MachineValue::Uint64(value) => value as i64,
            MachineValue::Int8(value) => value as i64,
            MachineValue::Int16(value) => value as i64,
            MachineValue::ReturnAddress(value) => value as i64,
        }
    }
}

macro_rules! perform_value_op {
    ($left:expr, $right:expr, $op:tt) => {
        match ($left, $right) {
            (MachineValue::Uint32(lhs), MachineValue::Uint32(rhs)) => {
                MachineValue::Uint32(lhs $op rhs)
            }
            (MachineValue::Uint64(lhs), MachineValue::Uint64(rhs)) => {
                MachineValue::Uint64(lhs $op rhs)
            }
            (MachineValue::Int32(lhs), MachineValue::Int32(rhs)) => MachineValue::Int32(lhs $op rhs),
            (MachineValue::Int64(lhs), MachineValue::Int64(rhs)) => MachineValue::Int64(lhs $op rhs),

            (MachineValue::Uint8(lhs), MachineValue::Uint8(rhs)) => MachineValue::Uint8(lhs $op rhs),
            (MachineValue::Uint16(lhs), MachineValue::Uint16(rhs)) => {
                MachineValue::Uint16(lhs $op rhs)
            }

            (MachineValue::Int8(lhs), MachineValue::Int8(rhs)) => MachineValue::Int8(lhs $op rhs),
            (MachineValue::Int16(lhs), MachineValue::Int16(rhs)) => MachineValue::Int16(lhs $op rhs),
            _ => match $left {
                MachineValue::Uint32(lhs) => MachineValue::Uint32(lhs $op $right.as_u32()),
                MachineValue::Uint64(lhs) => MachineValue::Uint64(lhs $op $right.as_u64()),
                MachineValue::Int32(lhs) => MachineValue::Int32(lhs $op $right.as_i32()),
                MachineValue::Int64(lhs) => MachineValue::Int64(lhs $op $right.as_i64()),

                MachineValue::None => MachineValue::None,
                MachineValue::Uint8(lhs) => MachineValue::Uint8(lhs $op $right.as_u8()),
                MachineValue::Uint16(lhs) => MachineValue::Uint16(lhs $op $right.as_u16()),
                MachineValue::Int8(lhs) => MachineValue::Int8(lhs $op $right.as_i8()),
                MachineValue::Int16(lhs) => MachineValue::Int16(lhs $op $right.as_i16()),
                MachineValue::ReturnAddress(lhs) => {
                    MachineValue::ReturnAddress(lhs $op $right.as_u64() as usize)
                }
            },
        }
    };
}

impl Add for MachineValue {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        perform_value_op!(self, rhs, +)
    }
}

impl Sub for MachineValue {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        perform_value_op!(self, rhs, -)
    }
}

impl Mul for MachineValue {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        perform_value_op!(self, rhs, *)
    }
}

impl Div for MachineValue {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        perform_value_op!(self, rhs, /)
    }
}

impl PartialEq for MachineValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (MachineValue::Uint32(lhs), MachineValue::Uint32(rhs)) => lhs == rhs,
            (MachineValue::Uint64(lhs), MachineValue::Uint64(rhs)) => lhs == rhs,
            (MachineValue::Int32(lhs), MachineValue::Int32(rhs)) => lhs == rhs,
            (MachineValue::Int64(lhs), MachineValue::Int64(rhs)) => lhs == rhs,

            (MachineValue::Uint8(lhs), MachineValue::Uint8(rhs)) => lhs == rhs,
            (MachineValue::Uint16(lhs), MachineValue::Uint16(rhs)) => lhs == rhs,
            (MachineValue::Int8(lhs), MachineValue::Int8(rhs)) => lhs == rhs,
            (MachineValue::Int16(lhs), MachineValue::Int16(rhs)) => lhs == rhs,
            (MachineValue::ReturnAddress(lhs), MachineValue::ReturnAddress(rhs)) => lhs == rhs,
            _ => match self {
                MachineValue::Uint32(value) => *value == other.as_u32(),
                MachineValue::Uint64(value) => *value == other.as_u64(),
                MachineValue::Int32(value) => *value == other.as_i32(),
                MachineValue::Int64(value) => *value == other.as_i64(),

                MachineValue::None => matches!(other, MachineValue::None),
                MachineValue::Uint8(value) => *value == other.as_u8(),
                MachineValue::Uint16(value) => *value == other.as_u16(),
                MachineValue::Int8(value) => *value == other.as_i8(),
                MachineValue::Int16(value) => *value == other.as_i16(),
                MachineValue::ReturnAddress(value) => *value == other.as_u64() as usize,
            },
        }
    }
}

impl Eq for MachineValue {}
