mod encoded;
pub mod textual;

#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug)]
#[repr(u8)]
pub enum OpCode {
    Push = 0,
    Pop = 1,
    Add = 2,
    Subtract = 3,
    Multiply = 4,
    Divide = 5,
    JumpIfEqual = 6,
    Exit = 7,
    JumpIfZero = 8,
    Call = 9,
    Return = 10,
    Jump = 11,
    Remainder = 12,
    CountLeadingZeros = 13,
    CountLeadingOnes = 14,
    CountTrailingZeros = 15,
    CountTrailingOnes = 16,
}

impl OpCode {
    pub const ALL: &'static [OpCode] = &[
        Self::Push,
        Self::Pop,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::JumpIfEqual,
        Self::Exit,
        Self::JumpIfZero,
        Self::Call,
        Self::Return,
        Self::Jump,
        Self::Remainder,
        Self::CountLeadingZeros,
        Self::CountLeadingOnes,
        Self::CountTrailingZeros,
        Self::CountTrailingOnes,
    ];
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug)]
#[repr(u8)]
pub enum OpArg {
    Register1 = 0,
    Register2 = 1,
    Register3 = 2,
    Register4 = 3,
    Register5 = 4,
    Register6 = 5,
    Register7 = 6,
    Register8 = 7,
    Register9 = 8,
    None = 9,
    Uint8(u8) = 10,
    Uint16(u16) = 11,
    Uint32(u32) = 12,
    Uint64(u64) = 13,
    Int8(i8) = 14,
    Int16(i16) = 15,
    Int32(i32) = 16,
    Int64(i64) = 17,
    Instruction(u64) = 18,
}

impl OpArg {
    pub fn register_index(&self) -> Option<usize> {
        match self {
            OpArg::Register1 => Some(0),
            OpArg::Register2 => Some(1),
            OpArg::Register3 => Some(2),
            OpArg::Register4 => Some(3),
            OpArg::Register5 => Some(4),
            OpArg::Register6 => Some(5),
            OpArg::Register7 => Some(6),
            OpArg::Register8 => Some(7),
            OpArg::Register9 => Some(8),
            _ => None,
        }
    }

    pub fn is_register(&self) -> bool {
        matches!(
            self,
            OpArg::Register1
                | OpArg::Register2
                | OpArg::Register3
                | OpArg::Register4
                | OpArg::Register5
                | OpArg::Register6
                | OpArg::Register7
                | OpArg::Register8
                | OpArg::Register9
        )
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug)]
pub struct Op {
    pub code: OpCode,
    pub arg: OpArg,
}

impl Op {
    pub const fn new(code: OpCode, arg: OpArg) -> Self {
        Self { code, arg }
    }
}

#[macro_export]
macro_rules! op {
    ($code:expr) => {
        $crate::op::Op::new($code, $crate::op::OpArg::None)
    };

    ($code:expr, $arg:expr) => {
        $crate::op::Op::new($code, $arg)
    };
}
