use crate::machine::MachineState;
use crate::machine::error::{MachineError, Result};
use crate::machine::intermediate::{BinaryOpKind, IntermediateOp, IntermediateProgram, Source};
use crate::machine::optimizer::{OptimizedProgram, RegisterTypes, ValueType};
use crate::machine::registers::RegisterBank;
use crate::machine::value::MachineValue;
use std::sync::Arc;

mod platform;

#[cfg(target_arch = "aarch64")]
mod aarch64;
#[cfg(target_arch = "aarch64")]
use aarch64::Assembler;

#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
use x86_64::Assembler;

type EntryFunction = unsafe extern "C" fn(*mut MachineState, usize) -> i64;

// The fast paths generate loads and stores of raw value slots, which relies
// on the `repr(u8)` layout of MachineValue: the tag byte at offset zero and
// word-sized payloads at PAYLOAD_OFFSET.
const _: () = assert!(std::mem::size_of::<MachineValue>() == 16);
const _: () = assert!(std::mem::align_of::<MachineValue>() == 8);

pub(super) const PAYLOAD_OFFSET: usize = 8;

/// The tag byte identifying a `MachineValue::Uint64`, the type the generated
/// fast paths specialize for.
pub(super) fn uint64_tag() -> u8 {
    let value = MachineValue::Uint64(0);
    unsafe { *(&value as *const MachineValue as *const u8) }
}

/// An operand a fast path can evaluate without calling a helper: a register
/// slot checked for the `Uint64` tag at run time, a register slot the
/// optimizer proved holds a `Uint64` and needs no check, or a `Uint64`
/// immediate.
#[derive(Clone, Copy)]
pub(super) enum FastOperand {
    Register(usize),
    TrustedRegister(usize),
    Immediate(u64),
}

#[derive(Clone, Copy)]
pub(super) enum FastBinaryOp {
    Add,
    Subtract,
    Multiply,
}

/// Byte offset of a register's value slot from the machine state pointer.
fn slot(index: usize) -> usize {
    std::mem::offset_of!(MachineState, bank) + RegisterBank::slot_offset(index)
}

/// The operand form of a register slot: trusted when the optimizer proved
/// the register holds a `Uint64`, tag-checked at run time otherwise.
fn register_operand(index: usize, types: &RegisterTypes) -> FastOperand {
    if types.get(index) == ValueType::Uint64 {
        FastOperand::TrustedRegister(slot(index))
    } else {
        FastOperand::Register(slot(index))
    }
}

fn fast_operand(source: &Source, types: &RegisterTypes) -> Option<FastOperand> {
    match source {
        Source::Register(index) => Some(register_operand(*index, types)),
        Source::Value(MachineValue::Uint64(value)) => Some(FastOperand::Immediate(*value)),
        Source::Value(_) => None,
    }
}

/// An intermediate program translated to native code. The generated code keeps
/// control flow, dispatch, and program-counter bookkeeping in machine
/// instructions; hot fused ops are inlined with a `Uint64` fast path, and
/// everything else calls the `helper` functions below, so execution semantics
/// match the interpreter exactly.
#[derive(Clone)]
pub struct JitProgram {
    code: Arc<JitCode>,
}

struct JitCode {
    memory: platform::ExecutableMemory,
    /// Native code address of every op, indexed by instruction; also consulted
    /// by generated code to dispatch `Return` ops.
    entries: Box<[usize]>,
    /// Operand pool referenced by pointers embedded in the generated code.
    _ops: Box<[IntermediateOp]>,
}

impl JitProgram {
    pub fn compile(program: &IntermediateProgram) -> Result<JitProgram> {
        Self::build(program.ops(), &[])
    }

    /// Compiles an optimized program, using its type analysis to drop the
    /// run-time tag checks on registers proven to hold `Uint64` values.
    pub fn compile_optimized(program: &OptimizedProgram) -> Result<JitProgram> {
        Self::build(program.ops(), program.types())
    }

    fn build(source: &[IntermediateOp], types: &[RegisterTypes]) -> Result<JitProgram> {
        let ops: Box<[IntermediateOp]> = source.into();
        let mut entries: Box<[usize]> = vec![0; ops.len()].into();

        let mut assembler = Assembler::new(entries.as_ptr() as usize);
        assembler.prologue();

        let mut offsets = Vec::with_capacity(ops.len());
        for (pc, op) in ops.iter().enumerate() {
            offsets.push(assembler.offset());
            let types = types.get(pc).copied().unwrap_or_default();
            emit(&mut assembler, op, pc, ops.len(), &types);
        }

        let overflow = assembler.offset();
        assembler.call(helper::overflow as *const (), &[ops.len() as u64]);
        assembler.epilogue();
        assembler.patch(&offsets, overflow)?;

        let memory = platform::ExecutableMemory::new(&assembler.finish())?;
        let base = memory.as_ptr() as usize;
        for (entry, offset) in entries.iter_mut().zip(&offsets) {
            *entry = base + offset;
        }

        Ok(JitProgram {
            code: Arc::new(JitCode {
                memory,
                entries,
                _ops: ops,
            }),
        })
    }

    pub(crate) fn run(&self, state: &mut MachineState) -> Result<()> {
        let Some(entry) = self.code.entries.get(state.current) else {
            return Err(MachineError::InstructionOverflow);
        };
        let function: EntryFunction = unsafe { std::mem::transmute(self.code.memory.as_ptr()) };
        let status = unsafe { function(state, *entry) };
        if status == 0 {
            Ok(())
        } else {
            Err(error_of_status(status))
        }
    }
}

fn emit(
    assembler: &mut Assembler,
    op: &IntermediateOp,
    pc: usize,
    length: usize,
    types: &RegisterTypes,
) {
    let data = op as *const IntermediateOp as u64;
    let position = pc as u64;
    match op {
        IntermediateOp::PushValue(_) => {
            assembler.call(helper::push_value as *const (), &[data]);
        }
        IntermediateOp::PushRegister(_) => {
            assembler.call(helper::push_register as *const (), &[data]);
        }
        IntermediateOp::PopRegister(_) => {
            assembler.call(helper::pop_register as *const (), &[data, position]);
            assembler.check_error();
        }
        IntermediateOp::Add
        | IntermediateOp::Subtract
        | IntermediateOp::Multiply
        | IntermediateOp::Divide
        | IntermediateOp::Remainder => {
            assembler.call(helper::binary_stack as *const (), &[data, position]);
            assembler.check_error();
        }
        IntermediateOp::CountLeadingZeros
        | IntermediateOp::CountLeadingOnes
        | IntermediateOp::CountTrailingZeros
        | IntermediateOp::CountTrailingOnes => {
            assembler.call(helper::count_stack as *const (), &[data, position]);
            assembler.check_error();
        }
        IntermediateOp::JumpIfEqual(target) => {
            assembler.call(helper::jump_if_equal_stack as *const (), &[position]);
            assembler.branch_status(*target);
        }
        IntermediateOp::JumpIfZero(target) => {
            assembler.call(helper::jump_if_zero_stack as *const (), &[position]);
            assembler.branch_status(*target);
        }
        IntermediateOp::Jump(target) => {
            assembler.jump(*target);
        }
        IntermediateOp::Call(target) => {
            assembler.call(helper::push_call as *const (), &[(pc + 1) as u64]);
            assembler.jump(*target);
        }
        IntermediateOp::Return => {
            assembler.call(helper::pop_return as *const (), &[length as u64, position]);
            assembler.return_dispatch();
        }
        IntermediateOp::Exit => {
            assembler.call(helper::finish as *const (), &[position]);
            assembler.jump_epilogue();
        }
        IntermediateOp::Binary {
            kind,
            lhs,
            rhs,
            dst,
        } => {
            let fast = match kind {
                BinaryOpKind::Add => Some(FastBinaryOp::Add),
                BinaryOpKind::Subtract => Some(FastBinaryOp::Subtract),
                BinaryOpKind::Multiply => Some(FastBinaryOp::Multiply),
                // Dividing an actual zero must reach the interpreter's own
                // division, not a hardware trap.
                BinaryOpKind::Divide | BinaryOpKind::Remainder => None,
            };
            match (fast, fast_operand(lhs, types), fast_operand(rhs, types)) {
                (Some(kind), Some(lhs), Some(rhs)) => {
                    assembler.binary_fast(
                        kind,
                        lhs,
                        rhs,
                        slot(*dst),
                        helper::binary_values as *const (),
                        data,
                    );
                }
                _ => assembler.call(helper::binary_values as *const (), &[data]),
            }
        }
        IntermediateOp::Copy { src, dst } => match src {
            Source::Register(index) => assembler.copy_slot(slot(*index), slot(*dst)),
            Source::Value(value) => {
                assembler.copy_constant(value as *const MachineValue as usize, slot(*dst));
            }
        },
        IntermediateOp::JumpIfEqualValues { lhs, rhs, target } => {
            if let (Source::Value(lhs), Source::Value(rhs)) = (lhs, rhs) {
                if rhs == lhs {
                    assembler.jump(*target);
                }
            } else if let (Some(lhs), Some(rhs)) =
                (fast_operand(lhs, types), fast_operand(rhs, types))
            {
                assembler.jump_if_equal_fast(
                    lhs,
                    rhs,
                    *target,
                    helper::jump_if_equal_values as *const (),
                    data,
                );
            } else {
                assembler.call(helper::jump_if_equal_values as *const (), &[data]);
                assembler.branch_taken(*target);
            }
        }
        IntermediateOp::JumpIfZeroValue { src, target } => match src {
            Source::Register(index) => assembler.jump_if_zero_fast(
                register_operand(*index, types),
                *target,
                helper::jump_if_zero_values as *const (),
                data,
            ),
            Source::Value(value) => {
                if value.is_zero() {
                    assembler.jump(*target);
                }
            }
        },
    }
}

fn status_of_error(error: MachineError) -> i64 {
    -(match error {
        MachineError::StackEmpty => 1,
        MachineError::InstructionExpected => 2,
        MachineError::InstructionOverflow => 3,
        MachineError::ValueExpected => 4,
        MachineError::RegisterExpected => 5,
        MachineError::CallStackEmpty => 6,
        MachineError::InvalidOpCode => 7,
        MachineError::MemoryUnavailable => 8,
        MachineError::StepUnsupported => 9,
    })
}

fn error_of_status(status: i64) -> MachineError {
    match -status {
        1 => MachineError::StackEmpty,
        2 => MachineError::InstructionExpected,
        3 => MachineError::InstructionOverflow,
        4 => MachineError::ValueExpected,
        5 => MachineError::RegisterExpected,
        6 => MachineError::CallStackEmpty,
        8 => MachineError::MemoryUnavailable,
        9 => MachineError::StepUnsupported,
        _ => MachineError::InvalidOpCode,
    }
}

/// Functions called by generated code. Every helper takes the machine state
/// first; helpers reading operands take a pointer to their `IntermediateOp`, and
/// fallible helpers take the op's program counter so they can record it in
/// `state.current` before reporting an error. Status returns are `0` for
/// success, `1` for branch-taken, and negative error codes from
/// `status_of_error`.
mod helper {
    use super::status_of_error;
    use crate::machine::MachineState;
    use crate::machine::error::MachineError;
    use crate::machine::intermediate::{BinaryOpKind, IntermediateOp};
    use crate::machine::value::MachineValue;
    use std::hint::unreachable_unchecked;

    fn fail(state: &mut MachineState, error: MachineError, pc: usize) -> i64 {
        state.current = pc;
        status_of_error(error)
    }

    pub unsafe extern "C" fn push_value(state: *mut MachineState, op: *const IntermediateOp) {
        let state = unsafe { &mut *state };
        let IntermediateOp::PushValue(value) = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        state.push(*value);
    }

    pub unsafe extern "C" fn push_register(state: *mut MachineState, op: *const IntermediateOp) {
        let state = unsafe { &mut *state };
        let IntermediateOp::PushRegister(index) = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        state.push(state.bank.get(*index));
    }

    pub unsafe extern "C" fn pop_register(
        state: *mut MachineState,
        op: *const IntermediateOp,
        pc: usize,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let IntermediateOp::PopRegister(index) = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        match state.pop() {
            Ok(value) => {
                state.bank.set(*index, value);
                0
            }
            Err(error) => fail(state, error, pc),
        }
    }

    pub unsafe extern "C" fn binary_stack(
        state: *mut MachineState,
        op: *const IntermediateOp,
        pc: usize,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let kind = match unsafe { &*op } {
            IntermediateOp::Add => BinaryOpKind::Add,
            IntermediateOp::Subtract => BinaryOpKind::Subtract,
            IntermediateOp::Multiply => BinaryOpKind::Multiply,
            IntermediateOp::Divide => BinaryOpKind::Divide,
            IntermediateOp::Remainder => BinaryOpKind::Remainder,
            _ => unsafe { unreachable_unchecked() },
        };
        let value1 = match state.pop() {
            Ok(value) => value,
            Err(error) => return fail(state, error, pc),
        };
        let value2 = match state.pop() {
            Ok(value) => value,
            Err(error) => return fail(state, error, pc),
        };
        state.push(kind.apply(value2, value1));
        0
    }

    pub unsafe extern "C" fn count_stack(
        state: *mut MachineState,
        op: *const IntermediateOp,
        pc: usize,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let value = match state.pop() {
            Ok(value) => value,
            Err(error) => return fail(state, error, pc),
        };
        let result = match unsafe { &*op } {
            IntermediateOp::CountLeadingZeros => value.leading_zeros(),
            IntermediateOp::CountLeadingOnes => value.leading_ones(),
            IntermediateOp::CountTrailingZeros => value.trailing_zeros(),
            IntermediateOp::CountTrailingOnes => value.trailing_ones(),
            _ => unsafe { unreachable_unchecked() },
        };
        state.push(result);
        0
    }

    pub unsafe extern "C" fn jump_if_equal_stack(state: *mut MachineState, pc: usize) -> i64 {
        let state = unsafe { &mut *state };
        let value1 = match state.pop() {
            Ok(value) => value,
            Err(error) => return fail(state, error, pc),
        };
        let value2 = match state.pop() {
            Ok(value) => value,
            Err(error) => return fail(state, error, pc),
        };
        (value1 == value2) as i64
    }

    pub unsafe extern "C" fn jump_if_zero_stack(state: *mut MachineState, pc: usize) -> i64 {
        let state = unsafe { &mut *state };
        let value = match state.pop() {
            Ok(value) => value,
            Err(error) => return fail(state, error, pc),
        };
        value.is_zero() as i64
    }

    pub unsafe extern "C" fn push_call(state: *mut MachineState, target: usize) {
        let state = unsafe { &mut *state };
        state.calls.push(MachineValue::ReturnAddress(target));
    }

    pub unsafe extern "C" fn pop_return(state: *mut MachineState, length: usize, pc: usize) -> i64 {
        let state = unsafe { &mut *state };
        let Some(value) = state.calls.pop() else {
            return fail(state, MachineError::CallStackEmpty, pc);
        };
        let MachineValue::ReturnAddress(target) = value else {
            return fail(state, MachineError::InstructionExpected, pc);
        };
        if target >= length {
            return fail(state, MachineError::InstructionOverflow, target);
        }
        target as i64
    }

    pub unsafe extern "C" fn binary_values(state: *mut MachineState, op: *const IntermediateOp) {
        let state = unsafe { &mut *state };
        let IntermediateOp::Binary {
            kind,
            lhs,
            rhs,
            dst,
        } = (unsafe { &*op })
        else {
            unsafe { unreachable_unchecked() }
        };
        let result = kind.apply(lhs.resolve(&state.bank), rhs.resolve(&state.bank));
        state.bank.set(*dst, result);
    }

    pub unsafe extern "C" fn jump_if_equal_values(
        state: *mut MachineState,
        op: *const IntermediateOp,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let IntermediateOp::JumpIfEqualValues { lhs, rhs, .. } = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        (rhs.resolve(&state.bank) == lhs.resolve(&state.bank)) as i64
    }

    pub unsafe extern "C" fn jump_if_zero_values(
        state: *mut MachineState,
        op: *const IntermediateOp,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let IntermediateOp::JumpIfZeroValue { src, .. } = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        src.resolve(&state.bank).is_zero() as i64
    }

    pub unsafe extern "C" fn finish(state: *mut MachineState, pc: usize) -> i64 {
        let state = unsafe { &mut *state };
        state.current = pc;
        0
    }

    pub unsafe extern "C" fn overflow(state: *mut MachineState, pc: usize) -> i64 {
        let state = unsafe { &mut *state };
        fail(state, MachineError::InstructionOverflow, pc)
    }
}
