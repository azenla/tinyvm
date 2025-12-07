use crate::op::{Op, OpArg, OpCode};

impl OpCode {
    pub const fn encoded_len() -> usize {
        size_of::<u8>()
    }

    pub const fn decode(id: u8) -> Option<OpCode> {
        match id {
            0 => Some(OpCode::Push),
            1 => Some(OpCode::Pop),
            2 => Some(OpCode::Add),
            3 => Some(OpCode::Subtract),
            4 => Some(OpCode::Multiply),
            5 => Some(OpCode::Divide),
            6 => Some(OpCode::JumpIfEqual),
            7 => Some(OpCode::Exit),
            8 => Some(OpCode::JumpIfZero),
            9 => Some(OpCode::Call),
            10 => Some(OpCode::Return),
            _ => None,
        }
    }

    pub const fn encode(&self) -> u8 {
        *self as u8
    }
}

impl OpArg {
    pub const fn encoded_len() -> usize {
        size_of::<u8>() + size_of::<u64>()
    }

    pub fn id(&self) -> u8 {
        match self {
            OpArg::Register1 => 0,
            OpArg::Register2 => 1,
            OpArg::Register3 => 2,
            OpArg::Register4 => 3,
            OpArg::Register5 => 4,
            OpArg::Register6 => 5,
            OpArg::Register7 => 6,
            OpArg::Register8 => 7,
            OpArg::Register9 => 8,
            OpArg::None => 9,
            OpArg::Uint8(_) => 10,
            OpArg::Uint16(_) => 11,
            OpArg::Uint32(_) => 12,
            OpArg::Uint64(_) => 13,
            OpArg::Int8(_) => 14,
            OpArg::Int16(_) => 15,
            OpArg::Int32(_) => 16,
            OpArg::Int64(_) => 17,
            OpArg::Instruction(_) => 18,
        }
    }

    pub const fn decode(buffer: &[u8]) -> Option<OpArg> {
        let id = buffer[0];
        let v1 = buffer[1];
        let v2 = buffer[2];
        let v3 = buffer[3];
        let v4 = buffer[4];
        let v5 = buffer[5];
        let v6 = buffer[6];
        let v7 = buffer[7];
        let v8 = buffer[8];
        Some(match id {
            0 => OpArg::Register1,
            1 => OpArg::Register2,
            2 => OpArg::Register3,
            3 => OpArg::Register4,
            4 => OpArg::Register5,
            5 => OpArg::Register6,
            6 => OpArg::Register7,
            7 => OpArg::Register8,
            8 => OpArg::Register9,
            9 => OpArg::None,
            10 => OpArg::Uint8(u8::from_le_bytes([v1])),
            11 => OpArg::Uint16(u16::from_le_bytes([v1, v2])),
            12 => OpArg::Uint32(u32::from_le_bytes([v1, v2, v3, v4])),
            13 => OpArg::Uint64(u64::from_le_bytes([v1, v2, v3, v4, v5, v6, v7, v8])),
            14 => OpArg::Int8(i8::from_le_bytes([v1])),
            15 => OpArg::Int16(i16::from_le_bytes([v1, v2])),
            16 => OpArg::Int32(i32::from_le_bytes([v1, v2, v3, v4])),
            17 => OpArg::Int64(i64::from_le_bytes([v1, v2, v3, v4, v5, v6, v7, v8])),
            18 => OpArg::Instruction(u64::from_le_bytes([v1, v2, v3, v4, v5, v6, v7, v8])),
            _ => return None,
        })
    }

    pub fn encode_value(&self, buffer: &mut [u8]) {
        match self {
            OpArg::None
            | OpArg::Register1
            | OpArg::Register2
            | OpArg::Register3
            | OpArg::Register4
            | OpArg::Register5
            | OpArg::Register6
            | OpArg::Register7
            | OpArg::Register8
            | OpArg::Register9 => buffer.fill(0),

            OpArg::Uint8(value) => {
                buffer[0..1].copy_from_slice(&value.to_le_bytes());
                buffer[1..].fill(0);
            }

            OpArg::Uint16(value) => {
                buffer[0..2].copy_from_slice(&value.to_le_bytes());
                buffer[2..].fill(0);
            }

            OpArg::Uint32(value) => {
                buffer[0..4].copy_from_slice(&value.to_le_bytes());
                buffer[4..].fill(0);
            }

            OpArg::Uint64(value) | OpArg::Instruction(value) => {
                buffer[0..8].copy_from_slice(&value.to_le_bytes());
                buffer[8..].fill(0);
            }

            OpArg::Int8(value) => {
                buffer[0..1].copy_from_slice(&value.to_le_bytes());
                buffer[1..].fill(0);
            }

            OpArg::Int16(value) => {
                buffer[0..2].copy_from_slice(&value.to_le_bytes());
                buffer[2..].fill(0);
            }

            OpArg::Int32(value) => {
                buffer[0..4].copy_from_slice(&value.to_le_bytes());
                buffer[4..].fill(0);
            }

            OpArg::Int64(value) => {
                buffer[0..8].copy_from_slice(&value.to_le_bytes());
                buffer[8..].fill(0);
            }
        }
    }

    pub fn encode(&self, buffer: &mut [u8]) {
        buffer[0] = self.id();
        self.encode_value(&mut buffer[1..]);
    }
}

impl Op {
    pub const fn new(code: OpCode, arg: OpArg) -> Self {
        Self { code, arg }
    }

    pub fn decode(buffer: &[u8]) -> Option<Self> {
        if buffer.len() != Self::encoded_len() {
            return None;
        }
        let code = OpCode::decode(buffer[0])?;
        let arg = OpArg::decode(&buffer[1..])?;
        Some(Op::new(code, arg))
    }

    pub const fn encoded_len() -> usize {
        OpCode::encoded_len() + OpArg::encoded_len()
    }

    pub fn encode(&self, buffer: &mut [u8]) {
        buffer[0] = self.code.encode();
        let argument = &mut buffer[1..];
        self.arg.encode(argument);
    }
}
