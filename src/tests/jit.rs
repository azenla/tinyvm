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
use crate::op::OpArg::Uint64;
use crate::op::OpCode::{Add, Exit, Push};
use crate::program;
use crate::program::RawProgram;

fn jit(program: &'static RawProgram) -> MachineProgram<'static> {
    let optimized = OptimizedProgram::compile(program).unwrap();
    MachineProgram::Jit(JitProgram::compile(&optimized).unwrap())
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
