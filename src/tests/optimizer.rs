use crate::machine::intermediate::{IntermediateOp, IntermediateProgram};
use crate::machine::optimizer::{OptimizedProgram, ValueType};
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineLoopState, MachineProgram, ops};
use crate::op;
use crate::op::OpArg::{Instruction, Register1, Register2, Uint64};
use crate::op::OpCode::{Add, Call, Divide, Exit, Jump, JumpIfZero, Pop, Push, Return, Subtract};
use crate::program;
use crate::program::RawProgram;

fn optimize(program: &RawProgram) -> OptimizedProgram {
    OptimizedProgram::compile(&IntermediateProgram::compile(program).unwrap())
}

fn run_uncompiled(program: &RawProgram) -> MachineValue {
    let mut machine = Machine::new(ops::all());
    machine.run(&MachineProgram::Uncompiled(program)).unwrap();
    machine.state().pop().unwrap()
}

fn run_optimized(program: &RawProgram) -> MachineValue {
    let mut machine = Machine::new(ops::all());
    machine
        .run(&MachineProgram::Optimized(optimize(program)))
        .unwrap();
    machine.state().pop().unwrap()
}

#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn run_jit(program: &RawProgram) -> MachineValue {
    use crate::machine::jit::JitProgram;

    let jit = JitProgram::compile_optimized(&optimize(program)).unwrap();
    let mut machine = Machine::new(ops::all());
    machine.run(&MachineProgram::Jit(jit)).unwrap();
    machine.state().pop().unwrap()
}

#[test]
fn folds_constant_arithmetic_into_copies() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(7)),
        op!(Pop, Register1),
        op!(Push, Register1),
        op!(Push, Uint64(3)),
        op!(Add),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Exit),
    );
    let optimized = optimize(&PROGRAM);
    assert!(
        optimized
            .ops()
            .iter()
            .all(|op| !matches!(op, IntermediateOp::Binary { .. }))
    );
    assert!(
        optimized
            .ops()
            .contains(&IntermediateOp::PushValue(MachineValue::Uint64(10)))
    );
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(10));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint64(10));
}

#[test]
fn folds_constant_conditions_to_static_jumps() {
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

    let taken = optimize(&TAKEN);
    assert_eq!(taken.ops()[0], IntermediateOp::Jump(3));
    assert_eq!(run_uncompiled(&TAKEN), MachineValue::Uint64(222));
    assert_eq!(run_optimized(&TAKEN), MachineValue::Uint64(222));

    // The untaken jump folds to a fall-through and is dropped entirely.
    let skipped = optimize(&SKIPPED);
    assert_eq!(skipped.ops().len(), taken.ops().len() - 1);
    assert_eq!(run_uncompiled(&SKIPPED), MachineValue::Uint64(111));
    assert_eq!(run_optimized(&SKIPPED), MachineValue::Uint64(111));
}

// The counter and accumulator are only ever written with `Uint64` values, so
// at the loop head — where the preheader constants merge with the
// loop-carried results — both registers are still proven `Uint64`, while an
// untouched register stays unknown.
#[test]
fn infers_register_types_around_a_loop() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(5)),
        op!(Pop, Register1),
        op!(Push, Uint64(0)),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Push, Uint64(1)),
        op!(Add),
        op!(Pop, Register2),
        op!(Push, Register1),
        op!(Push, Uint64(1)),
        op!(Subtract),
        op!(Pop, Register1),
        op!(Push, Register1),
        op!(JumpIfZero, Instruction(15)),
        op!(Jump, Instruction(4)),
        op!(Push, Register2),
        op!(Exit),
    );

    // Fusion compiles the raw program down to two preheader copies followed
    // by the loop body, so the loop head is op index 2.
    let optimized = optimize(&PROGRAM);
    let head = &optimized.types()[2];
    assert_eq!(head.get(0), ValueType::Uint64);
    assert_eq!(head.get(1), ValueType::Uint64);
    assert_eq!(head.get(2), ValueType::Unknown);

    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(5));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint64(5));
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    assert_eq!(run_jit(&PROGRAM), MachineValue::Uint64(5));
}

// Any call site may be the caller of any return, so constants must not
// propagate across a call boundary: the push after the call has to read the
// register the subroutine overwrote.
#[test]
fn calls_reset_register_facts() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(7)),
        op!(Pop, Register1),
        op!(Call, Instruction(5)),
        op!(Push, Register1),
        op!(Exit),
        op!(Push, Uint64(9)),
        op!(Pop, Register1),
        op!(Return),
    );
    let optimized = optimize(&PROGRAM);
    assert!(optimized.ops().contains(&IntermediateOp::PushRegister(0)));
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(9));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint64(9));
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    assert_eq!(run_jit(&PROGRAM), MachineValue::Uint64(9));
}

// Folding a division by zero would panic at compile time; the op must stay
// in the program and panic only if execution actually reaches it.
#[test]
fn division_by_constant_zero_is_not_folded() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(1)),
        op!(Push, Uint64(0)),
        op!(Divide),
        op!(Pop, Register1),
        op!(Exit),
    );
    let optimized = optimize(&PROGRAM);
    assert!(
        optimized
            .ops()
            .iter()
            .any(|op| matches!(op, IntermediateOp::Binary { .. }))
    );
}

#[test]
fn steps_an_optimized_program() {
    static PROGRAM: RawProgram = program!(op!(Push, Uint64(42)), op!(Exit));

    let program = MachineProgram::Optimized(optimize(&PROGRAM));
    let mut machine = Machine::new(ops::all());
    while machine.step(&program).unwrap() != MachineLoopState::Break {}
    assert_eq!(machine.state().pop().unwrap(), MachineValue::Uint64(42));
}
