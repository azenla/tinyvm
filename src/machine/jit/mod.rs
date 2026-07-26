use crate::machine::MachineState;
use crate::machine::error::{MachineError, Result};
use crate::machine::intermediate::{BinaryOpKind, IntermediateOp, IntermediateProgram, Source};
use crate::machine::optimizer::{OptimizedProgram, RegisterTypes, ValueType};
use crate::machine::registers::{REGISTER_BANK_COUNT, RegisterBank};
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

/// An operand a fast path can evaluate without calling a helper.
#[derive(Clone, Copy)]
pub(super) enum FastOperand {
    /// A register the optimizer proved holds a `Uint64`, needing no tag check.
    /// `pin` names the CPU register caching its payload; otherwise the payload
    /// is loaded from `slot`.
    Trusted { slot: usize, pin: Option<u32> },
    /// A register of unproven type: its tag is checked at run time and its
    /// payload loaded from `slot`. Never pinned — read straight from memory.
    Checked { slot: usize },
    /// A `Uint64` immediate.
    Immediate(u64),
}

/// The destination of a fast-path write. The payload is always written through
/// to `slot` so the memory bank stays coherent for helpers and unproven reads;
/// when the register is pinned the value additionally lives in `pin`'s CPU
/// register, where proven reads pick it up without touching memory.
#[derive(Clone, Copy)]
pub(super) struct FastDest {
    pub slot: usize,
    pub pin: Option<u32>,
}

#[derive(Clone, Copy)]
pub(super) enum FastBinaryOp {
    Add,
    Subtract,
    Multiply,
}

/// A binary op lowered to a fast path: the operation and operands, the
/// destination, and the `helper`/`op` pair called when a run-time tag check
/// fails. `write_tag` is false when the destination already holds a `Uint64`,
/// so only the payload word needs writing.
pub(super) struct FastBinary {
    pub kind: FastBinaryOp,
    pub lhs: FastOperand,
    pub rhs: FastOperand,
    pub dst: FastDest,
    pub helper: *const (),
    pub op: u64,
    pub write_tag: bool,
}

/// Which pinned registers a helper needs written back to the bank before it
/// runs. A pinned register's payload lives in its cpu register between calls and
/// its slot goes stale, so anything the helper reads has to be brought up to
/// date first. Helpers that read no slot need nothing: coherence on the way out
/// of generated code is the epilogue's job, not every call's.
#[derive(Clone, Copy)]
pub(super) enum Spill {
    /// Reads no register slot.
    None,
    /// Reads the one slot belonging to this pinned register.
    One { register: u32, slot: usize },
    /// May read any slot.
    All,
}

/// Byte offset of a register's value slot from the machine state pointer.
fn slot(index: usize) -> usize {
    std::mem::offset_of!(MachineState, bank) + RegisterBank::slot_offset(index)
}

fn pin_of(index: usize, pins: &[Option<u32>]) -> Option<u32> {
    pins.get(index).copied().flatten()
}

/// The spill a helper needs to read register `index` from the bank. An unpinned
/// register needs none: its slot is its only home, so it is never stale.
fn spill_of(index: usize, pins: &[Option<u32>]) -> Spill {
    match pin_of(index, pins) {
        Some(register) => Spill::One {
            register,
            slot: slot(index),
        },
        None => Spill::None,
    }
}

/// The operand form of a register slot: trusted (and pinned when the allocator
/// gave it a CPU register) when the optimizer proved the register holds a
/// `Uint64`, tag-checked at run time otherwise.
fn register_operand(index: usize, types: &RegisterTypes, pins: &[Option<u32>]) -> FastOperand {
    if types.get(index) == ValueType::Uint64 {
        FastOperand::Trusted {
            slot: slot(index),
            pin: pin_of(index, pins),
        }
    } else {
        FastOperand::Checked { slot: slot(index) }
    }
}

fn fast_operand(
    source: &Source,
    types: &RegisterTypes,
    pins: &[Option<u32>],
) -> Option<FastOperand> {
    match source {
        Source::Register(index) => Some(register_operand(*index, types, pins)),
        Source::Value(MachineValue::Uint64(value)) => Some(FastOperand::Immediate(*value)),
        Source::Value(_) => None,
    }
}

fn dest(index: usize, pins: &[Option<u32>]) -> FastDest {
    FastDest {
        slot: slot(index),
        pin: pin_of(index, pins),
    }
}

/// Assigns pinned CPU registers to the most-used VM registers the analysis
/// proves stay `Uint64`. A register is eligible when its inferred type is
/// `Uint64` or `Unknown` at every op (never another concrete type), it is
/// genuinely used as a `Uint64` somewhere, and every copy into it draws from a
/// proven `Uint64` source. Those conditions let its CPU register hold a bare
/// payload while the memory slot's tag stays `Uint64`. Returns the chosen
/// `(register index, cpu register)` pairs, at most `registers.len()` of them.
fn allocate(
    ops: &[IntermediateOp],
    types: &[RegisterTypes],
    registers: &[u32],
) -> Vec<(usize, u32)> {
    let type_at = |pc: usize, index: usize| {
        types
            .get(pc)
            .map(|t| t.get(index))
            .unwrap_or(ValueType::Unknown)
    };

    let mut eligible = [true; REGISTER_BANK_COUNT];
    let mut uint64_seen = [false; REGISTER_BANK_COUNT];
    let mut usage = [0usize; REGISTER_BANK_COUNT];

    for index in 0..REGISTER_BANK_COUNT {
        for pc in 0..ops.len() {
            match type_at(pc, index) {
                ValueType::Uint64 => uint64_seen[index] = true,
                ValueType::Unknown => {}
                _ => eligible[index] = false,
            }
        }
    }

    // A register read inline (as an op's `Source`) comes from its CPU register
    // when proven `Uint64` but straight from memory otherwise, and a pinned
    // register's memory payload goes stale between spills. So a register read
    // inline while not `Uint64` cannot be pinned; helper reads are always
    // coherent (a call spills the slots its helper reads) and do not disqualify.
    macro_rules! read_inline {
        ($pc:expr, $index:expr) => {{
            let index = $index;
            usage[index] += 1;
            if type_at($pc, index) != ValueType::Uint64 {
                eligible[index] = false;
            }
        }};
    }

    for (pc, op) in ops.iter().enumerate() {
        match op {
            IntermediateOp::PushRegister(index) | IntermediateOp::PopRegister(index) => {
                usage[*index] += 1;
            }
            IntermediateOp::Binary { lhs, rhs, dst, .. } => {
                if let Source::Register(index) = lhs {
                    read_inline!(pc, *index);
                }
                if let Source::Register(index) = rhs {
                    read_inline!(pc, *index);
                }
                usage[*dst] += 1;
            }
            IntermediateOp::Copy { src, dst } => {
                match src {
                    Source::Register(index) => {
                        read_inline!(pc, *index);
                        // A copy from an unproven source would land a
                        // possibly-non-`Uint64` value in the destination.
                        if type_at(pc, *index) != ValueType::Uint64 {
                            eligible[*dst] = false;
                        }
                    }
                    Source::Value(MachineValue::Uint64(_)) => {}
                    Source::Value(_) => eligible[*dst] = false,
                }
                usage[*dst] += 1;
            }
            IntermediateOp::JumpIfZeroValue {
                src: Source::Register(index),
                ..
            } => {
                read_inline!(pc, *index);
            }
            IntermediateOp::JumpIfEqualValues { lhs, rhs, .. } => {
                if let Source::Register(index) = lhs {
                    read_inline!(pc, *index);
                }
                if let Source::Register(index) = rhs {
                    read_inline!(pc, *index);
                }
            }
            _ => {}
        }
    }

    let mut candidates: Vec<usize> = (0..REGISTER_BANK_COUNT)
        .filter(|&index| eligible[index] && uint64_seen[index] && usage[index] > 0)
        .collect();
    // Most-used first; ties broken by index so the choice is deterministic.
    candidates.sort_by(|&a, &b| usage[b].cmp(&usage[a]).then(a.cmp(&b)));
    candidates
        .into_iter()
        .zip(registers.iter().copied())
        .collect()
}

/// Whether a binary op can be lowered as "base OP immediate", letting the
/// backend fold the constant into the instruction instead of materializing it
/// into a scratch register. Add is commutative, so the immediate may sit on
/// either side; subtract keeps it on the right. Returns the base operand and
/// the immediate value; each backend decides whether the value fits its own
/// immediate encoding.
pub(super) fn immediate_form(
    kind: FastBinaryOp,
    lhs: FastOperand,
    rhs: FastOperand,
) -> Option<(FastOperand, u64)> {
    match (kind, lhs, rhs) {
        (FastBinaryOp::Add, _, FastOperand::Immediate(value)) => Some((lhs, value)),
        (FastBinaryOp::Add, FastOperand::Immediate(value), _) => Some((rhs, value)),
        (FastBinaryOp::Subtract, _, FastOperand::Immediate(value)) => Some((lhs, value)),
        _ => None,
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
    /// Declared entry stack types, verified before entering at instruction
    /// zero: the generated code trusts them.
    inputs: Box<[ValueType]>,
    /// Operand pool referenced by pointers embedded in the generated code.
    _ops: Box<[IntermediateOp]>,
}

impl JitProgram {
    pub fn compile(program: &IntermediateProgram) -> Result<JitProgram> {
        Self::build(program.ops(), &[], &[])
    }

    /// Compiles an optimized program, using its type analysis to drop the
    /// run-time tag checks on registers proven to hold `Uint64` values.
    pub fn compile_optimized(program: &OptimizedProgram) -> Result<JitProgram> {
        Self::build(program.ops(), program.types(), program.inputs())
    }

    fn build(
        source: &[IntermediateOp],
        types: &[RegisterTypes],
        inputs: &[ValueType],
    ) -> Result<JitProgram> {
        let ops: Box<[IntermediateOp]> = source.into();
        let mut entries: Box<[usize]> = vec![0; ops.len()].into();

        // Decide which VM registers to keep in CPU registers, then record both
        // the index-to-CPU-register map the emitter consults and the
        // register/slot pairs the prologue loads and the epilogue trusts.
        let assignment = allocate(&ops, types, Assembler::PIN_REGISTERS);
        let mut pins = [None; REGISTER_BANK_COUNT];
        let mut pinned = Vec::with_capacity(assignment.len());
        for (index, register) in assignment {
            pins[index] = Some(register);
            pinned.push((register, slot(index)));
        }

        let mut assembler = Assembler::new(entries.as_ptr() as usize, pinned);
        assembler.prologue();

        let mut offsets = Vec::with_capacity(ops.len());
        for (pc, op) in ops.iter().enumerate() {
            offsets.push(assembler.offset());
            let op_types = types.get(pc).copied().unwrap_or_default();
            emit(&mut assembler, op, pc, ops.len(), &op_types, &pins);
        }

        let overflow = assembler.offset();
        assembler.call(
            helper::overflow as *const (),
            &[ops.len() as u64],
            Spill::None,
        );
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
                inputs: inputs.into(),
                _ops: ops,
            }),
        })
    }

    pub(crate) fn run(&self, state: &mut MachineState) -> Result<()> {
        state.check_inputs(&self.code.inputs)?;
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
    pins: &[Option<u32>],
) {
    let data = op as *const IntermediateOp as u64;
    let position = pc as u64;
    match op {
        IntermediateOp::PushValue(_) => {
            assembler.call(helper::push_value as *const (), &[data], Spill::None);
        }
        IntermediateOp::PushRegister(index) => {
            // Reads one bank slot, so only that register needs flushing.
            assembler.call(
                helper::push_register as *const (),
                &[data],
                spill_of(*index, pins),
            );
        }
        IntermediateOp::PopRegister(index) => {
            // Writes one bank slot and reads none, so nothing needs flushing
            // first.
            assembler.call(
                helper::pop_register as *const (),
                &[data, position],
                Spill::None,
            );
            assembler.check_error();
            // Refresh only the register the helper wrote. Reloading the rest
            // would overwrite live payloads with the stale slots behind them,
            // and doing it after the error check keeps a failed pop from
            // pulling a stale payload into a pinned register.
            if let Some(register) = pin_of(*index, pins) {
                assembler.reload_one(register, slot(*index));
            }
        }
        IntermediateOp::Add
        | IntermediateOp::Subtract
        | IntermediateOp::Multiply
        | IntermediateOp::Divide
        | IntermediateOp::Remainder => {
            assembler.call(
                helper::binary_stack as *const (),
                &[data, position],
                Spill::None,
            );
            assembler.check_error();
        }
        IntermediateOp::CountLeadingZeros
        | IntermediateOp::CountLeadingOnes
        | IntermediateOp::CountTrailingZeros
        | IntermediateOp::CountTrailingOnes => {
            assembler.call(
                helper::count_stack as *const (),
                &[data, position],
                Spill::None,
            );
            assembler.check_error();
        }
        IntermediateOp::JumpIfEqual(target) => {
            assembler.call(
                helper::jump_if_equal_stack as *const (),
                &[position],
                Spill::None,
            );
            assembler.branch_status(*target);
        }
        IntermediateOp::JumpIfZero(target) => {
            assembler.call(
                helper::jump_if_zero_stack as *const (),
                &[position],
                Spill::None,
            );
            assembler.branch_status(*target);
        }
        IntermediateOp::Jump(target) => {
            assembler.jump(*target);
        }
        IntermediateOp::Call(target) => {
            // Touches only the call stack.
            assembler.call(
                helper::push_call as *const (),
                &[(pc + 1) as u64],
                Spill::None,
            );
            assembler.jump(*target);
        }
        IntermediateOp::Return => {
            assembler.call(
                helper::pop_return as *const (),
                &[length as u64, position],
                Spill::None,
            );
            assembler.return_dispatch();
        }
        IntermediateOp::Exit => {
            // Only records the program counter; the epilogue makes the bank
            // coherent on the way out.
            assembler.call(helper::finish as *const (), &[position], Spill::None);
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
            match (
                fast,
                fast_operand(lhs, types, pins),
                fast_operand(rhs, types, pins),
            ) {
                (Some(kind), Some(lhs), Some(rhs)) => {
                    // The fast path always yields a `Uint64`, so when the
                    // destination already holds one its tag byte is correct
                    // and only the payload word needs writing.
                    let write_tag = types.get(*dst) != ValueType::Uint64;
                    assembler.binary_fast(FastBinary {
                        kind,
                        lhs,
                        rhs,
                        dst: dest(*dst, pins),
                        helper: helper::binary_values as *const (),
                        op: data,
                        write_tag,
                    });
                }
                // Divide and remainder have no fast path, and an operand that is
                // not a `Uint64` immediate cannot be materialized inline. The
                // helper writes the destination's slot directly, so a pinned
                // destination is refreshed from it.
                _ => {
                    // Resolves both operands from the bank, so every pinned
                    // register has to be in memory first.
                    assembler.call(helper::binary_values as *const (), &[data], Spill::All);
                    if let Some(register) = pin_of(*dst, pins) {
                        assembler.reload_one(register, slot(*dst));
                    }
                }
            }
        }
        IntermediateOp::Copy { src, dst } => {
            let destination = dest(*dst, pins);
            // The result takes the source's type, so the tag needs writing
            // only when the destination did not already hold a `Uint64`.
            let write_tag = types.get(*dst) != ValueType::Uint64;
            match src {
                // A proven `Uint64` source (register or immediate) is a payload
                // move, kept in the destination's CPU register and written
                // through to its slot.
                Source::Register(index) if types.get(*index) == ValueType::Uint64 => {
                    let source = register_operand(*index, types, pins);
                    assembler.copy_register(source, destination, write_tag);
                }
                Source::Value(MachineValue::Uint64(value)) => {
                    assembler.copy_register(FastOperand::Immediate(*value), destination, write_tag);
                }
                // Any other source may not be a `Uint64`, so the whole slot is
                // copied through memory; a pinned destination is refreshed
                // from the slot afterward.
                Source::Register(index) => {
                    assembler.copy_slot(slot(*index), destination.slot);
                    if let Some(register) = destination.pin {
                        assembler.reload_one(register, destination.slot);
                    }
                }
                Source::Value(value) => {
                    assembler
                        .copy_constant(value as *const MachineValue as usize, destination.slot);
                    if let Some(register) = destination.pin {
                        assembler.reload_one(register, destination.slot);
                    }
                }
            }
        }
        IntermediateOp::JumpIfEqualValues { lhs, rhs, target } => {
            if let (Source::Value(lhs), Source::Value(rhs)) = (lhs, rhs) {
                if rhs == lhs {
                    assembler.jump(*target);
                }
            } else if let (Some(lhs), Some(rhs)) = (
                fast_operand(lhs, types, pins),
                fast_operand(rhs, types, pins),
            ) {
                assembler.jump_if_equal_fast(
                    lhs,
                    rhs,
                    *target,
                    helper::jump_if_equal_values as *const (),
                    data,
                );
            } else {
                // Resolves both operands from the bank.
                assembler.call(
                    helper::jump_if_equal_values as *const (),
                    &[data],
                    Spill::All,
                );
                assembler.branch_taken(*target);
            }
        }
        IntermediateOp::JumpIfZeroValue { src, target } => match src {
            Source::Register(index) => assembler.jump_if_zero_fast(
                register_operand(*index, types, pins),
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
        MachineError::InputMismatch => 10,
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
        10 => MachineError::InputMismatch,
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
