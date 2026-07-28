use crate::machine::error::MachineError;
use crate::machine::intermediate::{IntermediateOp, IntermediateProgram};
use crate::machine::optimizer::{OptimizedProgram, ValueType};
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineLoopState, MachineProgram, ops};
use crate::op;
use crate::op::OpArg::{Instruction, Register1, Register2, Uint64};
use crate::op::OpCode::{
    Add, Call, CountLeadingZeros, Divide, Exit, Jump, JumpIfZero, Pop, Push, Return, Subtract,
};
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

// A resumption point takes the facts the return brought, never the ones the
// caller had: the subroutine overwrote `Register1`, so the push after the call
// must see 9 and the caller's 7 must not survive anywhere. Every reachable
// return here agrees on 9, so it is the callee's constant that propagates.
#[test]
fn calls_resume_with_the_callee_facts() {
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
    let pushed = IntermediateOp::PushValue(MachineValue::Uint64(9));
    assert!(optimized.ops().contains(&pushed));
    assert!(
        !optimized
            .ops()
            .contains(&IntermediateOp::PushValue(MachineValue::Uint64(7)))
    );
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(9));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint64(9));
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    assert_eq!(run_jit(&PROGRAM), MachineValue::Uint64(9));
}

// Any call site may be the caller of any return, so a resumption point gets the
// merge of every return. These two disagree on what `Register1` holds, which
// leaves no constant to propagate — the push has to read the register — while
// still proving the type both returns share.
//
// The branch is taken on a counted value rather than a constant, so folding
// cannot decide it and both returns stay reachable.
#[test]
fn returns_that_disagree_leave_no_constant() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(5)),
        op!(CountLeadingZeros),
        op!(Pop, Register2),
        op!(Push, Uint64(7)),
        op!(Pop, Register1),
        op!(Call, Instruction(8)),
        op!(Push, Register1),
        op!(Exit),
        op!(Push, Register2),
        op!(JumpIfZero, Instruction(13)),
        op!(Push, Uint64(9)),
        op!(Pop, Register1),
        op!(Return),
        op!(Push, Uint64(11)),
        op!(Pop, Register1),
        op!(Return),
    );
    let optimized = optimize(&PROGRAM);
    assert!(optimized.ops().contains(&IntermediateOp::PushRegister(0)));
    for value in [7u64, 9, 11] {
        assert!(
            !optimized
                .ops()
                .contains(&IntermediateOp::PushValue(MachineValue::Uint64(value)))
        );
    }

    // Disagreeing on the value still agrees on the type, which is what the jit
    // needs to drop the tag check.
    let resumption = optimized
        .ops()
        .iter()
        .position(|op| *op == IntermediateOp::PushRegister(0))
        .unwrap();
    assert_eq!(optimized.types()[resumption].get(0), ValueType::Uint64);

    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(9));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint64(9));
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    assert_eq!(run_jit(&PROGRAM), MachineValue::Uint64(9));
}

/// The type of `Register2` where the reclaiming push reads it — the last of the
/// two pushes, the one after the call.
fn reclaimed_type(optimized: &OptimizedProgram) -> ValueType {
    let at = optimized
        .ops()
        .iter()
        .rposition(|op| *op == IntermediateOp::PushRegister(1))
        .unwrap();
    optimized.types()[at].get(1)
}

// Spilling a register across a call and reclaiming it afterwards is how a
// recursive program keeps a value the callee clobbers, so the value stack has to
// survive a call for the reclaimed register to have a type at all. This callee
// is balanced and stays within its own frame, so it does.
//
// The spilled value is counted rather than written, which types it without
// making it a constant — a constant would prove nothing about the stack, since
// folding could carry it around the call in the op itself.
#[test]
fn a_balanced_call_leaves_the_stack_it_was_handed() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(5)),
        op!(CountLeadingZeros),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Call, Instruction(8)),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Exit),
        op!(Push, Uint64(9)),
        op!(Pop, Register2),
        op!(Return),
    );
    assert_eq!(reclaimed_type(&optimize(&PROGRAM)), ValueType::Uint32);
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint32(61));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint32(61));
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    assert_eq!(run_jit(&PROGRAM), MachineValue::Uint32(61));
}

// This callee returns one value deeper than it started, so what the caller
// reclaims is not what it spilled and nothing about the stack carries across.
#[test]
fn an_unbalanced_call_leaves_the_stack_unknown() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(5)),
        op!(CountLeadingZeros),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Call, Instruction(8)),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Exit),
        op!(Push, Uint64(9)),
        op!(Return),
    );
    assert_eq!(reclaimed_type(&optimize(&PROGRAM)), ValueType::Unknown);
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(9));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint64(9));
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    assert_eq!(run_jit(&PROGRAM), MachineValue::Uint64(9));
}

// Balance alone is not enough: this callee returns at the depth it started at,
// but it got there by taking the caller's value and leaving one of another type
// in its place. Carrying the caller's stack across a call that reaches beneath
// its own frame would type the reclaimed `Uint64` as the spilled `Uint32` and
// let the jit skip the tag check that catches it, so reaching beneath a frame
// disqualifies the program too.
#[test]
fn a_call_reaching_beneath_its_frame_leaves_the_stack_unknown() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(5)),
        op!(CountLeadingZeros),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Call, Instruction(8)),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Exit),
        op!(Pop, Register1),
        op!(Push, Uint64(9)),
        op!(Return),
    );
    assert_eq!(reclaimed_type(&optimize(&PROGRAM)), ValueType::Unknown);
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

// Without a declaration the popped input is untypeable; with one, the type
// flows through the abstract stack into the register.
#[test]
fn declared_inputs_type_popped_values() {
    static PROGRAM: RawProgram = program!(op!(Pop, Register1), op!(Push, Register1), op!(Exit),);

    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let untyped = OptimizedProgram::compile(&intermediate);
    assert_eq!(untyped.types()[1].get(0), ValueType::Unknown);

    let typed = OptimizedProgram::compile_with_inputs(&intermediate, &[ValueType::Uint64]);
    assert_eq!(typed.types()[1].get(0), ValueType::Uint64);
}

// The declaration is a contract the analysis trusted, so running with the
// wrong type on the stack — or nothing at all — must fail up front rather
// than let type-specialized code read the wrong payloads.
#[test]
fn rejects_undeclared_inputs() {
    static PROGRAM: RawProgram = program!(op!(Pop, Register1), op!(Push, Register1), op!(Exit),);

    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &[ValueType::Uint64]);
    let program = MachineProgram::Optimized(optimized.clone());

    let mut machine = Machine::new(ops::all());
    machine.state().push(MachineValue::Uint32(5));
    let error = machine.run(&program).unwrap_err();
    assert_eq!(error, MachineError::InputMismatch);

    let mut machine = Machine::new(ops::all());
    let error = machine.run(&program).unwrap_err();
    assert_eq!(error, MachineError::InputMismatch);

    let mut machine = Machine::new(ops::all());
    machine.state().push(MachineValue::Uint64(5));
    machine.run(&program).unwrap();
    assert_eq!(machine.state().pop().unwrap(), MachineValue::Uint64(5));

    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        use crate::machine::jit::JitProgram;

        let jit = MachineProgram::Jit(JitProgram::compile_optimized(&optimized).unwrap());
        let mut machine = Machine::new(ops::all());
        machine.state().push(MachineValue::Uint32(5));
        assert_eq!(machine.run(&jit).unwrap_err(), MachineError::InputMismatch);

        let mut machine = Machine::new(ops::all());
        machine.state().push(MachineValue::Uint64(5));
        machine.run(&jit).unwrap();
        assert_eq!(machine.state().pop().unwrap(), MachineValue::Uint64(5));
    }
}

// Both branches push a `Uint64` before the join, so the pop after it is
// typed even though fusion could not see across the jump target.
#[test]
fn stack_types_survive_branch_joins() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Register1),
        op!(JumpIfZero, Instruction(4)),
        op!(Push, Uint64(7)),
        op!(Jump, Instruction(5)),
        op!(Push, Uint64(9)),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Exit),
    );

    // Fusion leaves the pop at op index 4, after the branch join, and the
    // push of the popped register at index 5.
    let optimized = optimize(&PROGRAM);
    assert_eq!(optimized.types()[5].get(1), ValueType::Uint64);
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(9));
    assert_eq!(run_optimized(&PROGRAM), MachineValue::Uint64(9));
}

#[test]
fn steps_an_optimized_program() {
    static PROGRAM: RawProgram = program!(op!(Push, Uint64(42)), op!(Exit));

    let program = MachineProgram::Optimized(optimize(&PROGRAM));
    let mut machine = Machine::new(ops::all());
    while machine.step(&program).unwrap() != MachineLoopState::Break {}
    assert_eq!(machine.state().pop().unwrap(), MachineValue::Uint64(42));
}
