pub mod dataflow;
mod impls;

#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug)]
#[repr(u8)]
pub enum OpCode {
    Push = 0,
    Pop = 1,
    Add = 2,
    Subtract = 3,
    Multiply = 4,
    Divide = 5,
    Remainder = 6,
    CountLeadingZeros = 7,
    CountLeadingOnes = 8,
    CountTrailingZeros = 9,
    CountTrailingOnes = 10,
    Jump = 11,
    JumpIfZero = 12,
    JumpIfEqual = 13,
    Call = 14,
    Return = 15,
    Exit = 16,
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
    None = 10,
    Uint8(u8) = 11,
    Uint16(u16) = 12,
    Uint32(u32) = 13,
    Uint64(u64) = 14,
    Int8(i8) = 15,
    Int16(i16) = 16,
    Int32(i32) = 17,
    Int64(i64) = 18,
    Instruction(u64) = 19,
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug)]
pub struct Op {
    pub code: OpCode,
    pub arg: OpArg,
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
