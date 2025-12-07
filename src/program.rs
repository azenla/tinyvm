use crate::op::Op;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Program {
    Owned(Vec<Op>),
    Static(&'static [Op]),
}

impl Program {
    pub fn new(ops: Vec<Op>) -> Self {
        Self::Owned(ops)
    }

    pub fn ops(&self) -> &[Op] {
        match self {
            Self::Owned(ops) => ops,
            Self::Static(ops) => ops,
        }
    }

    pub fn decode(buffer: &[u8]) -> Option<Self> {
        let mut ops = Vec::new();
        for i in 0..buffer.len() / Op::encoded_len() {
            let op = Op::decode(
                &buffer[i * Op::encoded_len()..i * Op::encoded_len() + Op::encoded_len()],
            )?;
            ops.push(op);
        }
        Some(Self::Owned(ops))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buffer = vec![0; self.ops().len() * Op::encoded_len()];
        for (i, op) in self.ops().iter().enumerate() {
            op.encode(&mut buffer[i * Op::encoded_len()..]);
        }
        buffer
    }
}

#[macro_export]
macro_rules! program_static {
    ($($op:expr),+ $(,)?) => {
        $crate::program::Program::Static(&[$($op),+])
    }
}

#[macro_export]
macro_rules! program{
    ($($op:expr),+ $(,)?) => {
        $crate::program::Program::new(vec![$($op),+])
    }
}
