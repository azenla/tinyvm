use crate::op::{Op, OpArg, OpCode};
use std::error::Error;
use std::fmt::Display;
use std::str::FromStr;

impl OpCode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            OpCode::Push => "push",
            OpCode::Pop => "pop",
            OpCode::Add => "add",
            OpCode::Subtract => "sub",
            OpCode::Multiply => "mul",
            OpCode::Divide => "div",
            OpCode::JumpIfEqual => "jie",
            OpCode::Exit => "exit",
            OpCode::JumpIfZero => "jiz",
            OpCode::Call => "call",
            OpCode::Return => "ret",
            OpCode::Jump => "jmp",
            OpCode::Remainder => "rem",
            OpCode::CountLeadingZeros => "clz",
            OpCode::CountLeadingOnes => "clo",
            OpCode::CountTrailingZeros => "ctz",
            OpCode::CountTrailingOnes => "cto",
        }
    }
}

impl OpArg {
    pub const fn as_id(&self) -> &'static str {
        match self {
            OpArg::Register1 => "r1",
            OpArg::Register2 => "r2",
            OpArg::Register3 => "r3",
            OpArg::Register4 => "r4",
            OpArg::Register5 => "r5",
            OpArg::Register6 => "r6",
            OpArg::Register7 => "r7",
            OpArg::Register8 => "r8",
            OpArg::Register9 => "r9",
            OpArg::None => "none",
            OpArg::Uint8(_) => "u8",
            OpArg::Uint16(_) => "u16",
            OpArg::Uint32(_) => "u32",
            OpArg::Uint64(_) => "u64",
            OpArg::Int8(_) => "i8",
            OpArg::Int16(_) => "i16",
            OpArg::Int32(_) => "i32",
            OpArg::Int64(_) => "i64",
            OpArg::Instruction(_) => "p",
        }
    }
}

impl Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.arg == OpArg::None {
            write!(f, "{}", self.code.as_str())
        } else {
            write!(f, "{} {}", self.code.as_str(), self.arg)
        }
    }
}

impl Display for OpArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpArg::Uint8(value) => write!(f, "{}", value)?,
            OpArg::Uint16(value) => write!(f, "{}", value)?,
            OpArg::Uint32(value) => write!(f, "{}", value)?,
            OpArg::Uint64(value) => write!(f, "{}", value)?,
            OpArg::Int8(value) => write!(f, "{}", value)?,
            OpArg::Int16(value) => write!(f, "{}", value)?,
            OpArg::Int32(value) => write!(f, "{}", value)?,
            OpArg::Int64(value) => write!(f, "{}", value)?,
            OpArg::Instruction(value) => write!(f, "{}", value)?,
            _ => {}
        };

        write!(f, "{}", self.as_id())
    }
}

#[derive(Debug)]
pub enum TextualParseError {
    InvalidToken(String),
    InvalidOpCode(String),
}

impl Display for TextualParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextualParseError::InvalidToken(token) => write!(f, "invalid token: {}", token),
            TextualParseError::InvalidOpCode(opcode) => write!(f, "invalid opcode: {}", opcode),
        }
    }
}

impl Error for TextualParseError {}

impl FromStr for Op {
    type Err = TextualParseError;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let Some((code, value)) = string.split_once(" ") else {
            return Ok(Op::new(OpCode::from_str(string)?, OpArg::None));
        };
        let code = OpCode::from_str(code)?;
        let arg = OpArg::from_str(value)?;
        Ok(Op::new(code, arg))
    }
}

impl FromStr for OpCode {
    type Err = TextualParseError;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let lower = string.to_lowercase();
        let code = OpCode::ALL
            .iter()
            .find(|code| code.as_str() == lower)
            .ok_or(TextualParseError::InvalidOpCode(lower))?;
        Ok(*code)
    }
}

impl FromStr for OpArg {
    type Err = TextualParseError;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let lower = string.trim().to_lowercase();
        if lower.is_empty() || lower == "none" {
            return Ok(OpArg::None);
        }

        if let Some(suffix) = lower.strip_prefix("r") {
            let reg = suffix
                .parse::<usize>()
                .map_err(|_| TextualParseError::InvalidToken(string.to_string()))?;

            return match reg {
                1 => Ok(OpArg::Register1),
                2 => Ok(OpArg::Register2),
                3 => Ok(OpArg::Register3),
                4 => Ok(OpArg::Register4),
                5 => Ok(OpArg::Register5),
                6 => Ok(OpArg::Register6),
                7 => Ok(OpArg::Register7),
                8 => Ok(OpArg::Register8),
                9 => Ok(OpArg::Register9),
                _ => Err(TextualParseError::InvalidToken(string.to_string())),
            };
        }

        let Some(sign_index) = string.find(['u', 'i', 'p']) else {
            return Err(TextualParseError::InvalidToken(string.to_string()));
        };

        let value = &string[..sign_index];
        let sign = &lower[sign_index..sign_index + 1];

        if sign == "p" {
            return value
                .parse::<u64>()
                .map_err(|_| TextualParseError::InvalidToken(value.to_string()))
                .map(OpArg::Instruction);
        }

        let size = lower[sign_index + 1..]
            .parse::<usize>()
            .map_err(|_| TextualParseError::InvalidToken(string.to_string()))?;

        let result = match (sign, size) {
            ("u", 8) => value.parse::<u8>().map(OpArg::Uint8),
            ("i", 8) => value.parse::<i8>().map(OpArg::Int8),
            ("u", 16) => value.parse::<u16>().map(OpArg::Uint16),
            ("i", 16) => value.parse::<i16>().map(OpArg::Int16),
            ("u", 32) => value.parse::<u32>().map(OpArg::Uint32),
            ("i", 32) => value.parse::<i32>().map(OpArg::Int32),
            ("u", 64) => value.parse::<u64>().map(OpArg::Uint64),
            ("i", 64) => value.parse::<i64>().map(OpArg::Int64),
            _ => return Err(TextualParseError::InvalidToken(string.to_string())),
        };

        result.map_err(|_| TextualParseError::InvalidToken(string.to_string()))
    }
}
