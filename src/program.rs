use crate::op::Op;
use std::borrow::Cow;

pub mod textual;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawProgram<'ops> {
    ops: Cow<'ops, [Op]>,
}

impl<'ops> RawProgram<'ops> {
    pub const fn new_owned(ops: Vec<Op>) -> Self {
        Self {
            ops: Cow::Owned(ops),
        }
    }

    pub const fn new_borrowed(ops: &'ops [Op]) -> RawProgram<'ops> {
        Self {
            ops: Cow::Borrowed(ops),
        }
    }

    pub const fn from_cow(ops: Cow<'ops, [Op]>) -> Self {
        Self { ops }
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn decode(buffer: &[u8]) -> Option<Self> {
        let mut ops = Vec::new();
        for i in 0..buffer.len() / Op::encoded_len() {
            let op = Op::decode(
                &buffer[i * Op::encoded_len()..i * Op::encoded_len() + Op::encoded_len()],
            )?;
            ops.push(op);
        }
        Some(Self::new_owned(ops))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buffer = vec![0; self.ops().len() * Op::encoded_len()];
        for (i, op) in self.ops().iter().enumerate() {
            op.encode(&mut buffer[i * Op::encoded_len()..]);
        }
        buffer
    }

    pub fn into_cow(self) -> Cow<'ops, [Op]> {
        self.ops
    }
}

#[macro_export]
macro_rules! program {
    ($($op:expr),+ $(,)?) => {
       $crate::program::RawProgram::new_borrowed(&[$($op),+])
    }
}

#[macro_export]
macro_rules! program_owned {
    ($($op:expr),+ $(,)?) => {
        $crate::program::RawProgram::new_owned(vec![$($op),+])
    }
}
