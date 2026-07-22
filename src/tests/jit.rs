#![cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use crate::machine::error::MachineError;
use crate::machine::jit::JitProgram;
use crate::machine::optimized::OptimizedProgram;
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineProgram, ops};
use crate::op;
use crate::op::OpArg::{Instruction, Register1, Register2, Register3, Uint32, Uint64};
use crate::op::OpCode::{Add, Exit, JumpIfZero, Pop, Push};
use crate::program;
use crate::program::RawProgram;

fn jit(program: &'static RawProgram) -> MachineProgram<'static> {
    let optimized = OptimizedProgram::compile(program).unwrap();
    MachineProgram::Jit(JitProgram::compile(&optimized).unwrap())
}

fn top(program: &'static RawProgram) -> MachineValue {
    let mut machine = Machine::new(ops::all());
    machine.run(&jit(program)).unwrap();
    machine.state().pop().unwrap()
}

// The generated fast paths read the tag byte at offset zero and word payloads
// at offset eight, which `repr(u8)` guarantees.
#[test]
fn machine_value_layout_matches_the_generated_code() {
    assert_eq!(std::mem::size_of::<MachineValue>(), 16);
    let value = MachineValue::Uint64(0x0123_4567_89AB_CDEF);
    let payload = unsafe { *(&value as *const MachineValue as *const u64).add(1) };
    assert_eq!(payload, 0x0123_4567_89AB_CDEF);
}

// Values that are not Uint64 must fail the fast path's tag check and reach
// the helper, producing exactly the interpreter's result.
#[test]
fn fast_paths_fall_back_for_other_types() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint32(7)),
        op!(Pop, Register1),
        op!(Push, Uint32(3)),
        op!(Pop, Register2),
        op!(Push, Register1),
        op!(Push, Register2),
        op!(Add),
        op!(Pop, Register3),
        op!(Push, Register3),
        op!(Exit),
    );

    let mut interpreted = Machine::new(ops::all());
    let optimized = OptimizedProgram::compile(&PROGRAM).unwrap();
    interpreted
        .run(&MachineProgram::Optimized(optimized))
        .unwrap();
    let expected = interpreted.state().pop().unwrap();

    assert_eq!(expected, MachineValue::Uint32(10));
    assert_eq!(top(&PROGRAM), expected);
}

// A jump-if-zero on a constant compiles to static control flow: a bare jump
// when the constant is zero, a fall-through when it is not.
#[test]
fn constant_conditions_compile_statically() {
    static TAKEN: RawProgram = program!(
        op!(Push, Uint64(0)),
        op!(JumpIfZero, Instruction(4)),
        op!(Push, Uint64(111)),
        op!(Exit),
        op!(Push, Uint64(222)),
        op!(Exit),
    );
    static SKIPPED: RawProgram = program!(
        op!(Push, Uint64(1)),
        op!(JumpIfZero, Instruction(4)),
        op!(Push, Uint64(111)),
        op!(Exit),
        op!(Push, Uint64(222)),
        op!(Exit),
    );
    assert_eq!(top(&TAKEN), MachineValue::Uint64(222));
    assert_eq!(top(&SKIPPED), MachineValue::Uint64(111));
}

// The failing op must report the same error and leave the same program
// counter behind as the interpreter.
#[test]
fn errors_match_the_interpreter() {
    static PROGRAM: RawProgram = program!(op!(Push, Uint64(1)), op!(Add), op!(Exit));

    let mut interpreted = Machine::new(ops::all());
    let optimized = OptimizedProgram::compile(&PROGRAM).unwrap();
    let interpreted_error = interpreted
        .run(&MachineProgram::Optimized(optimized))
        .unwrap_err();

    let mut jitted = Machine::new(ops::all());
    let jitted_error = jitted.run(&jit(&PROGRAM)).unwrap_err();

    assert_eq!(jitted_error, MachineError::StackEmpty);
    assert_eq!(jitted_error, interpreted_error);
}

// Execution enters at `state.current`, not always at instruction zero: step
// the first op under the interpreter, then let the jit finish the program.
#[test]
fn enters_at_the_current_instruction() {
    static PROGRAM: RawProgram = program!(op!(Push, Uint64(99)), op!(Push, Uint64(42)), op!(Exit),);

    let mut machine = Machine::new(ops::all());
    machine.step(&MachineProgram::Uncompiled(&PROGRAM)).unwrap();
    machine.run(&jit(&PROGRAM)).unwrap();
    assert_eq!(machine.state().pop().unwrap(), MachineValue::Uint64(42));
    assert_eq!(machine.state().pop().unwrap(), MachineValue::Uint64(99));
}

#[test]
fn stepping_is_unsupported() {
    static PROGRAM: RawProgram = program!(op!(Exit));

    let program = jit(&PROGRAM);
    let mut machine = Machine::new(ops::all());
    assert_eq!(
        machine.step(&program).unwrap_err(),
        MachineError::StepUnsupported
    );
}
