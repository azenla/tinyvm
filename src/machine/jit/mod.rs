use crate::machine::MachineState;
use crate::machine::error::{MachineError, Result};
use crate::machine::optimized::{OptimizedOp, OptimizedProgram};
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

/// An optimized program translated to native code. The generated code keeps
/// control flow, dispatch, and program-counter bookkeeping in machine
/// instructions and calls the `helper` functions below for op bodies, so
/// execution semantics match the interpreter exactly.
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
    _ops: Box<[OptimizedOp]>,
}

impl JitProgram {
    pub fn compile(program: &OptimizedProgram) -> Result<JitProgram> {
        let ops: Box<[OptimizedOp]> = program.ops().into();
        let mut entries: Box<[usize]> = vec![0; ops.len()].into();

        let mut assembler = Assembler::new(entries.as_ptr() as usize);
        assembler.prologue();

        let mut offsets = Vec::with_capacity(ops.len());
        for (pc, op) in ops.iter().enumerate() {
            offsets.push(assembler.offset());
            emit(&mut assembler, op, pc, ops.len());
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

fn emit(assembler: &mut Assembler, op: &OptimizedOp, pc: usize, length: usize) {
    let data = op as *const OptimizedOp as u64;
    let position = pc as u64;
    match op {
        OptimizedOp::PushValue(_) => {
            assembler.call(helper::push_value as *const (), &[data]);
        }
        OptimizedOp::PushRegister(_) => {
            assembler.call(helper::push_register as *const (), &[data]);
        }
        OptimizedOp::PopRegister(_) => {
            assembler.call(helper::pop_register as *const (), &[data, position]);
            assembler.check_error();
        }
        OptimizedOp::Add
        | OptimizedOp::Subtract
        | OptimizedOp::Multiply
        | OptimizedOp::Divide
        | OptimizedOp::Remainder => {
            assembler.call(helper::binary_stack as *const (), &[data, position]);
            assembler.check_error();
        }
        OptimizedOp::CountLeadingZeros
        | OptimizedOp::CountLeadingOnes
        | OptimizedOp::CountTrailingZeros
        | OptimizedOp::CountTrailingOnes => {
            assembler.call(helper::count_stack as *const (), &[data, position]);
            assembler.check_error();
        }
        OptimizedOp::JumpIfEqual(target) => {
            assembler.call(helper::jump_if_equal_stack as *const (), &[position]);
            assembler.branch_status(*target);
        }
        OptimizedOp::JumpIfZero(target) => {
            assembler.call(helper::jump_if_zero_stack as *const (), &[position]);
            assembler.branch_status(*target);
        }
        OptimizedOp::Jump(target) => {
            assembler.jump(*target);
        }
        OptimizedOp::Call(target) => {
            assembler.call(helper::push_call as *const (), &[(pc + 1) as u64]);
            assembler.jump(*target);
        }
        OptimizedOp::Return => {
            assembler.call(helper::pop_return as *const (), &[length as u64, position]);
            assembler.return_dispatch();
        }
        OptimizedOp::Exit => {
            assembler.call(helper::finish as *const (), &[position]);
            assembler.jump_epilogue();
        }
        OptimizedOp::Binary { .. } => {
            assembler.call(helper::binary_values as *const (), &[data]);
        }
        OptimizedOp::Copy { .. } => {
            assembler.call(helper::copy_values as *const (), &[data]);
        }
        OptimizedOp::JumpIfEqualValues { target, .. } => {
            assembler.call(helper::jump_if_equal_values as *const (), &[data]);
            assembler.branch_taken(*target);
        }
        OptimizedOp::JumpIfZeroValue { target, .. } => {
            assembler.call(helper::jump_if_zero_values as *const (), &[data]);
            assembler.branch_taken(*target);
        }
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
/// first; helpers reading operands take a pointer to their `OptimizedOp`, and
/// fallible helpers take the op's program counter so they can record it in
/// `state.current` before reporting an error. Status returns are `0` for
/// success, `1` for branch-taken, and negative error codes from
/// `status_of_error`.
mod helper {
    use super::status_of_error;
    use crate::machine::MachineState;
    use crate::machine::error::MachineError;
    use crate::machine::optimized::{BinaryOpKind, OptimizedOp};
    use crate::machine::value::MachineValue;
    use std::hint::unreachable_unchecked;

    fn fail(state: &mut MachineState, error: MachineError, pc: usize) -> i64 {
        state.current = pc;
        status_of_error(error)
    }

    pub unsafe extern "C" fn push_value(state: *mut MachineState, op: *const OptimizedOp) {
        let state = unsafe { &mut *state };
        let OptimizedOp::PushValue(value) = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        state.push(*value);
    }

    pub unsafe extern "C" fn push_register(state: *mut MachineState, op: *const OptimizedOp) {
        let state = unsafe { &mut *state };
        let OptimizedOp::PushRegister(index) = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        state.push(state.bank.get(*index));
    }

    pub unsafe extern "C" fn pop_register(
        state: *mut MachineState,
        op: *const OptimizedOp,
        pc: usize,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let OptimizedOp::PopRegister(index) = (unsafe { &*op }) else {
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
        op: *const OptimizedOp,
        pc: usize,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let kind = match unsafe { &*op } {
            OptimizedOp::Add => BinaryOpKind::Add,
            OptimizedOp::Subtract => BinaryOpKind::Subtract,
            OptimizedOp::Multiply => BinaryOpKind::Multiply,
            OptimizedOp::Divide => BinaryOpKind::Divide,
            OptimizedOp::Remainder => BinaryOpKind::Remainder,
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
        op: *const OptimizedOp,
        pc: usize,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let value = match state.pop() {
            Ok(value) => value,
            Err(error) => return fail(state, error, pc),
        };
        let result = match unsafe { &*op } {
            OptimizedOp::CountLeadingZeros => value.leading_zeros(),
            OptimizedOp::CountLeadingOnes => value.leading_ones(),
            OptimizedOp::CountTrailingZeros => value.trailing_zeros(),
            OptimizedOp::CountTrailingOnes => value.trailing_ones(),
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

    pub unsafe extern "C" fn binary_values(state: *mut MachineState, op: *const OptimizedOp) {
        let state = unsafe { &mut *state };
        let OptimizedOp::Binary {
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

    pub unsafe extern "C" fn copy_values(state: *mut MachineState, op: *const OptimizedOp) {
        let state = unsafe { &mut *state };
        let OptimizedOp::Copy { src, dst } = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        let value = src.resolve(&state.bank);
        state.bank.set(*dst, value);
    }

    pub unsafe extern "C" fn jump_if_equal_values(
        state: *mut MachineState,
        op: *const OptimizedOp,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let OptimizedOp::JumpIfEqualValues { lhs, rhs, .. } = (unsafe { &*op }) else {
            unsafe { unreachable_unchecked() }
        };
        (rhs.resolve(&state.bank) == lhs.resolve(&state.bank)) as i64
    }

    pub unsafe extern "C" fn jump_if_zero_values(
        state: *mut MachineState,
        op: *const OptimizedOp,
    ) -> i64 {
        let state = unsafe { &mut *state };
        let OptimizedOp::JumpIfZeroValue { src, .. } = (unsafe { &*op }) else {
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
