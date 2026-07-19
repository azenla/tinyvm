use crate::op::OpArg;

mod numerics;

#[derive(Clone, Copy, Debug, Default)]
pub enum MachineValue {
    #[default]
    None,
    Uint8(u8),
    Uint16(u16),
    Uint32(u32),
    Uint64(u64),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    ReturnAddress(usize),
}

impl From<OpArg> for MachineValue {
    fn from(value: OpArg) -> Self {
        match value {
            OpArg::None => MachineValue::None,
            OpArg::Uint8(value) => MachineValue::Uint8(value),
            OpArg::Uint16(value) => MachineValue::Uint16(value),
            OpArg::Uint32(value) => MachineValue::Uint32(value),
            OpArg::Uint64(value) => MachineValue::Uint64(value),
            OpArg::Int8(value) => MachineValue::Int8(value),
            OpArg::Int16(value) => MachineValue::Int16(value),
            OpArg::Int32(value) => MachineValue::Int32(value),
            OpArg::Int64(value) => MachineValue::Int64(value),
            _ => MachineValue::None,
        }
    }
}
