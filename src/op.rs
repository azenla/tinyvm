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
    JumpIfEqual = 6,
    Exit = 7,
    JumpIfZero = 8,
    Call = 9,
    Return = 10,
    Jump = 11,
}

#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug)]
#[repr(u8)]
pub enum OpArg {
    Register1,
    Register2,
    Register3,
    Register4,
    Register5,
    Register6,
    Register7,
    Register8,
    Register9,
    None,
    Uint8(u8),
    Uint16(u16),
    Uint32(u32),
    Uint64(u64),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Instruction(u64),
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
