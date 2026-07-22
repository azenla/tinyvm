use crate::machine::MachineState;
use crate::machine::error::{MachineError, Result};
use crate::machine::registers::RegisterBank;
use crate::machine::value::MachineValue;
use crate::op::{Op, OpArg, OpCode};
use crate::program::RawProgram;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Register(usize),
    Value(MachineValue),
}

impl Source {
    fn of(op: &Op) -> Option<Source> {
        if op.code != OpCode::Push {
            return None;
        }
        match op.arg.register_index() {
            Some(index) => Some(Source::Register(index)),
            None => match op.arg {
                OpArg::Instruction(_) => None,
                arg => Some(Source::Value(MachineValue::from(arg))),
            },
        }
    }

    #[inline(always)]
    fn resolve(&self, bank: &RegisterBank) -> MachineValue {
        match *self {
            Source::Register(index) => bank.get(index),
            Source::Value(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinaryOpKind {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

impl BinaryOpKind {
    fn of(code: OpCode) -> Option<BinaryOpKind> {
        Some(match code {
            OpCode::Add => BinaryOpKind::Add,
            OpCode::Subtract => BinaryOpKind::Subtract,
            OpCode::Multiply => BinaryOpKind::Multiply,
            OpCode::Divide => BinaryOpKind::Divide,
            OpCode::Remainder => BinaryOpKind::Remainder,
            _ => return None,
        })
    }

    #[inline(always)]
    fn apply(&self, lhs: MachineValue, rhs: MachineValue) -> MachineValue {
        match self {
            BinaryOpKind::Add => lhs + rhs,
            BinaryOpKind::Subtract => lhs - rhs,
            BinaryOpKind::Multiply => lhs * rhs,
            BinaryOpKind::Divide => lhs / rhs,
            BinaryOpKind::Remainder => lhs % rhs,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompiledOp {
    PushValue(MachineValue),
    PushRegister(usize),
    PopRegister(usize),
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    JumpIfEqual(usize),
    JumpIfZero(usize),
    Jump(usize),
    Call(usize),
    Return,
    Exit,
    CountLeadingZeros,
    CountLeadingOnes,
    CountTrailingZeros,
    CountTrailingOnes,
    Binary {
        kind: BinaryOpKind,
        lhs: Source,
        rhs: Source,
        dst: usize,
    },
    Copy {
        src: Source,
        dst: usize,
    },
    JumpIfEqualValues {
        lhs: Source,
        rhs: Source,
        target: usize,
    },
    JumpIfZeroValue {
        src: Source,
        target: usize,
    },
}

impl CompiledOp {
    pub fn compile(op: &Op) -> Result<Self> {
        Ok(match op.code {
            OpCode::Push => match op.arg.register_index() {
                Some(index) => CompiledOp::PushRegister(index),
                None => match op.arg {
                    OpArg::Instruction(_) => return Err(MachineError::ValueExpected),
                    arg => CompiledOp::PushValue(MachineValue::from(arg)),
                },
            },
            OpCode::Pop => CompiledOp::PopRegister(
                op.arg
                    .register_index()
                    .ok_or(MachineError::RegisterExpected)?,
            ),
            OpCode::Add => CompiledOp::Add,
            OpCode::Subtract => CompiledOp::Subtract,
            OpCode::Multiply => CompiledOp::Multiply,
            OpCode::Divide => CompiledOp::Divide,
            OpCode::Remainder => CompiledOp::Remainder,
            OpCode::JumpIfEqual => CompiledOp::JumpIfEqual(instruction_target(op)?),
            OpCode::JumpIfZero => CompiledOp::JumpIfZero(instruction_target(op)?),
            OpCode::Jump => CompiledOp::Jump(instruction_target(op)?),
            OpCode::Call => CompiledOp::Call(instruction_target(op)?),
            OpCode::Return => CompiledOp::Return,
            OpCode::Exit => CompiledOp::Exit,
            OpCode::CountLeadingZeros => CompiledOp::CountLeadingZeros,
            OpCode::CountLeadingOnes => CompiledOp::CountLeadingOnes,
            OpCode::CountTrailingZeros => CompiledOp::CountTrailingZeros,
            OpCode::CountTrailingOnes => CompiledOp::CountTrailingOnes,
        })
    }

    /// Fuses a stack-neutral sequence starting at `ops[0]` into a single
    /// superinstruction. Positions `1..length` must not be jump targets, since
    /// control flow may never land inside a fused sequence.
    fn fuse(ops: &[Op], targets: &[bool]) -> Option<(Self, usize)> {
        let clear = |length: usize| targets[1..length].iter().all(|target| !target);

        if ops.len() >= 4
            && clear(4)
            && let Some(lhs) = Source::of(&ops[0])
            && let Some(rhs) = Source::of(&ops[1])
            && let Some(kind) = BinaryOpKind::of(ops[2].code)
            && ops[3].code == OpCode::Pop
            && let Some(dst) = ops[3].arg.register_index()
        {
            return Some((
                CompiledOp::Binary {
                    kind,
                    lhs,
                    rhs,
                    dst,
                },
                4,
            ));
        }

        if ops.len() >= 3
            && clear(3)
            && let Some(lhs) = Source::of(&ops[0])
            && let Some(rhs) = Source::of(&ops[1])
            && ops[2].code == OpCode::JumpIfEqual
            && let OpArg::Instruction(target) = ops[2].arg
        {
            return Some((
                CompiledOp::JumpIfEqualValues {
                    lhs,
                    rhs,
                    target: target as usize,
                },
                3,
            ));
        }

        if ops.len() >= 2
            && clear(2)
            && let Some(src) = Source::of(&ops[0])
        {
            if ops[1].code == OpCode::JumpIfZero
                && let OpArg::Instruction(target) = ops[1].arg
            {
                return Some((
                    CompiledOp::JumpIfZeroValue {
                        src,
                        target: target as usize,
                    },
                    2,
                ));
            }

            if ops[1].code == OpCode::Pop
                && let Some(dst) = ops[1].arg.register_index()
            {
                return Some((CompiledOp::Copy { src, dst }, 2));
            }
        }

        None
    }

    fn remap(&mut self, remap: impl Fn(usize) -> usize) {
        match self {
            CompiledOp::JumpIfEqual(target)
            | CompiledOp::JumpIfZero(target)
            | CompiledOp::Jump(target)
            | CompiledOp::Call(target)
            | CompiledOp::JumpIfEqualValues { target, .. }
            | CompiledOp::JumpIfZeroValue { target, .. } => *target = remap(*target),
            _ => {}
        }
    }

    #[inline(always)]
    pub(crate) fn perform(&self, machine: &mut MachineState, pc: usize) -> Result<Option<usize>> {
        Ok(match *self {
            CompiledOp::PushValue(value) => {
                machine.push(value);
                Some(pc + 1)
            }
            CompiledOp::PushRegister(index) => {
                machine.push(machine.bank.get(index));
                Some(pc + 1)
            }
            CompiledOp::PopRegister(index) => {
                let value = machine.pop()?;
                machine.bank.set(index, value);
                Some(pc + 1)
            }
            CompiledOp::Add => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 + value1);
                Some(pc + 1)
            }
            CompiledOp::Subtract => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 - value1);
                Some(pc + 1)
            }
            CompiledOp::Multiply => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 * value1);
                Some(pc + 1)
            }
            CompiledOp::Divide => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 / value1);
                Some(pc + 1)
            }
            CompiledOp::Remainder => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                machine.push(value2 % value1);
                Some(pc + 1)
            }
            CompiledOp::JumpIfEqual(target) => {
                let value1 = machine.pop()?;
                let value2 = machine.pop()?;
                if value1 == value2 {
                    Some(target)
                } else {
                    Some(pc + 1)
                }
            }
            CompiledOp::JumpIfZero(target) => {
                let value = machine.pop()?;
                if value.is_zero() {
                    Some(target)
                } else {
                    Some(pc + 1)
                }
            }
            CompiledOp::Jump(target) => Some(target),
            CompiledOp::Call(target) => {
                machine.calls.push(MachineValue::ReturnAddress(pc + 1));
                Some(target)
            }
            CompiledOp::Return => {
                let value = machine.calls.pop().ok_or(MachineError::CallStackEmpty)?;
                match value {
                    MachineValue::ReturnAddress(target) => Some(target),
                    _ => return Err(MachineError::InstructionExpected),
                }
            }
            CompiledOp::Exit => None,
            CompiledOp::CountLeadingZeros => {
                let value = machine.pop()?;
                machine.push(value.leading_zeros());
                Some(pc + 1)
            }
            CompiledOp::CountLeadingOnes => {
                let value = machine.pop()?;
                machine.push(value.leading_ones());
                Some(pc + 1)
            }
            CompiledOp::CountTrailingZeros => {
                let value = machine.pop()?;
                machine.push(value.trailing_zeros());
                Some(pc + 1)
            }
            CompiledOp::CountTrailingOnes => {
                let value = machine.pop()?;
                machine.push(value.trailing_ones());
                Some(pc + 1)
            }
            CompiledOp::Binary {
                kind,
                lhs,
                rhs,
                dst,
            } => {
                let result = kind.apply(lhs.resolve(&machine.bank), rhs.resolve(&machine.bank));
                machine.bank.set(dst, result);
                Some(pc + 1)
            }
            CompiledOp::Copy { src, dst } => {
                let value = src.resolve(&machine.bank);
                machine.bank.set(dst, value);
                Some(pc + 1)
            }
            CompiledOp::JumpIfEqualValues { lhs, rhs, target } => {
                if rhs.resolve(&machine.bank) == lhs.resolve(&machine.bank) {
                    Some(target)
                } else {
                    Some(pc + 1)
                }
            }
            CompiledOp::JumpIfZeroValue { src, target } => {
                if src.resolve(&machine.bank).is_zero() {
                    Some(target)
                } else {
                    Some(pc + 1)
                }
            }
        })
    }
}

fn instruction_target(op: &Op) -> Result<usize> {
    match op.arg {
        OpArg::Instruction(target) => Ok(target as usize),
        _ => Err(MachineError::InstructionExpected),
    }
}

#[derive(Clone, Debug)]
pub struct CompiledProgram {
    ops: Vec<CompiledOp>,
}

impl CompiledProgram {
    pub fn compile(program: &RawProgram) -> Result<Self> {
        let source = program.ops();

        let mut targets = vec![false; source.len()];
        for (index, op) in source.iter().enumerate() {
            if let OpArg::Instruction(target) = op.arg
                && let Some(flag) = targets.get_mut(target as usize)
            {
                *flag = true;
            }

            if op.code == OpCode::Call
                && let Some(flag) = targets.get_mut(index + 1)
            {
                *flag = true;
            }
        }

        let mut ops = Vec::with_capacity(source.len());
        let mut map = vec![0; source.len()];
        let mut index = 0;
        while index < source.len() {
            let (op, length) = match CompiledOp::fuse(&source[index..], &targets[index..]) {
                Some(fused) => fused,
                None => (CompiledOp::compile(&source[index])?, 1),
            };
            for offset in 0..length {
                map[index + offset] = ops.len();
            }
            ops.push(op);
            index += length;
        }

        let length = ops.len();
        for op in &mut ops {
            op.remap(|target| {
                if target < map.len() {
                    map[target]
                } else {
                    length + (target - map.len())
                }
            });
        }

        Ok(Self { ops })
    }

    pub fn ops(&self) -> &[CompiledOp] {
        &self.ops
    }
}
