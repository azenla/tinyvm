mod impls;

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
