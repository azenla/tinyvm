#![cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use crate::machine::error::MachineError;
use crate::machine::intermediate::IntermediateProgram;
use crate::machine::jit::JitProgram;
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineProgram, ops};
use crate::op;
use crate::op::OpArg::{Instruction, Register1, Register2, Register3, Uint32, Uint64};
use crate::op::OpCode::{Add, Exit, JumpIfZero, Pop, Push};
use crate::program;
use crate::program::RawProgram;

fn jit(program: &'static RawProgram) -> MachineProgram<'static> {
    let intermediate = IntermediateProgram::compile(program).unwrap();
    MachineProgram::Jit(JitProgram::compile(&intermediate).unwrap())
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
    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    interpreted
        .run(&MachineProgram::Intermediate(intermediate))
        .unwrap();
    let expected = interpreted.state().pop().unwrap();

    assert_eq!(expected, MachineValue::Uint32(10));
    assert_eq!(top(&PROGRAM), expected);
}

// The optimizer proves r3 is a Uint64 and the allocator pins it, yet the add
// that writes it reads r2, whose runtime Uint32 fails the tag check and drives
// the slow-path helper. The pinned r3 must be reloaded from the bank the
// helper wrote, so the jit's result and bank match the interpreter.
#[test]
fn slow_path_reloads_pinned_destination() {
    use crate::machine::optimizer::{OptimizedProgram, ValueType};

    static PROGRAM: RawProgram = program!(
        op!(Pop, Register2),
        op!(Pop, Register1),
        op!(Push, Register1),
        op!(Push, Register2),
        op!(Add),
        op!(Pop, Register3),
        op!(Push, Register3),
        op!(Exit),
    );

    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let optimized =
        OptimizedProgram::compile_with_inputs(&intermediate, &[ValueType::Uint64, ValueType::Uint32]);
    let jit = MachineProgram::Jit(JitProgram::compile_optimized(&optimized).unwrap());
    let interpreted = MachineProgram::Intermediate(intermediate);

    for (base, addend) in [(0u64, 0u32), (5, 7), (u64::MAX, 1), (1000, u32::MAX)] {
        let mut jitted = Machine::new(ops::all());
        jitted.state().push(MachineValue::Uint64(base));
        jitted.state().push(MachineValue::Uint32(addend));
        jitted.run(&jit).unwrap();

        let mut interpreter = Machine::new(ops::all());
        interpreter.state().push(MachineValue::Uint64(base));
        interpreter.state().push(MachineValue::Uint32(addend));
        interpreter.run(&interpreted).unwrap();

        let jit_result = jitted.state().pop().unwrap();
        let interpreter_result = interpreter.state().pop().unwrap();
        assert_eq!(jit_result, interpreter_result, "result mismatch for {base} + {addend}");
        assert_eq!(jitted.state().bank(), interpreter.state().bank());
    }
}

// An odd number of pinned registers pads the saved-register area for
// alignment (x86_64) and pairs a register with the zero register (aarch64).
// Popping then pushing three Uint64 registers pins three of them and routes
// every access through a helper, exercising that padding plus the
// spill-before-call and reload-after-call paths.
#[test]
fn odd_pin_count_round_trips_through_helpers() {
    use crate::machine::optimizer::{OptimizedProgram, ValueType};

    static PROGRAM: RawProgram = program!(
        op!(Pop, Register1),
        op!(Pop, Register2),
        op!(Pop, Register3),
        op!(Push, Register1),
        op!(Push, Register2),
        op!(Push, Register3),
        op!(Exit),
    );

    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(
        &intermediate,
        &[ValueType::Uint64, ValueType::Uint64, ValueType::Uint64],
    );
    let jit = MachineProgram::Jit(JitProgram::compile_optimized(&optimized).unwrap());
    let interpreted = MachineProgram::Intermediate(intermediate);

    let mut jitted = Machine::new(ops::all());
    let mut interpreter = Machine::new(ops::all());
    for value in [10u64, 20, 30] {
        jitted.state().push(MachineValue::Uint64(value));
        interpreter.state().push(MachineValue::Uint64(value));
    }
    jitted.run(&jit).unwrap();
    interpreter.run(&interpreted).unwrap();

    for _ in 0..3 {
        assert_eq!(
            jitted.state().pop().unwrap(),
            interpreter.state().pop().unwrap()
        );
    }
    assert_eq!(jitted.state().bank(), interpreter.state().bank());
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
    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let interpreted_error = interpreted
        .run(&MachineProgram::Intermediate(intermediate))
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
