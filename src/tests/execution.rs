use crate::machine::error::MachineError;
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineProgram, ops};
use crate::op;
use crate::op::Op;
use crate::op::OpArg::{Instruction, Register5, Uint64};
use crate::op::OpCode::{
    Add, CountLeadingOnes, CountLeadingZeros, CountTrailingOnes, CountTrailingZeros, Divide, Exit,
    Jump, JumpIfEqual, JumpIfZero, Multiply, Pop, Push, Remainder, Return, Subtract,
};
use crate::program::RawProgram;

/// Runs a program to completion after seeding the stack with `setup` (pushed
/// left-to-right, so the last element ends up on top).
fn run(setup: &[MachineValue], body: Vec<Op>) -> Machine {
    let program = RawProgram::new_owned(body);
    let mut machine = Machine::new(ops::all());
    for &value in setup {
        machine.state().push(value);
    }
    machine
        .run(&MachineProgram::Uncompiled(&program))
        .expect("program should run without error");
    machine
}

/// Runs a program and returns the value left on top of the stack.
fn top(setup: &[MachineValue], body: Vec<Op>) -> MachineValue {
    let mut machine = run(setup, body);
    machine.state().pop().expect("stack should not be empty")
}

fn u64v(value: u64) -> MachineValue {
    MachineValue::Uint64(value)
}

#[test]
fn add() {
    assert_eq!(
        top(&[u64v(10), u64v(32)], vec![op!(Add), op!(Exit)]),
        u64v(42)
    );
}

#[test]
fn subtract_uses_second_operand_first() {
    // value2 - value1, where value1 is the top of the stack.
    assert_eq!(
        top(&[u64v(50), u64v(8)], vec![op!(Subtract), op!(Exit)]),
        u64v(42)
    );
}

#[test]
fn multiply() {
    assert_eq!(
        top(&[u64v(6), u64v(7)], vec![op!(Multiply), op!(Exit)]),
        u64v(42)
    );
}

#[test]
fn divide() {
    assert_eq!(
        top(&[u64v(84), u64v(2)], vec![op!(Divide), op!(Exit)]),
        u64v(42)
    );
}

#[test]
fn divide_signed() {
    assert_eq!(
        top(
            &[MachineValue::Int32(-84), MachineValue::Int32(2)],
            vec![op!(Divide), op!(Exit)]
        ),
        MachineValue::Int32(-42)
    );
}

#[test]
fn remainder() {
    assert_eq!(
        top(&[u64v(50), u64v(8)], vec![op!(Remainder), op!(Exit)]),
        u64v(2)
    );
}

#[test]
fn mixed_types_coerce_to_left_operand() {
    // value2 (Uint64) is the left operand, so the result stays Uint64.
    assert_eq!(
        top(
            &[MachineValue::Uint64(40), MachineValue::Uint32(2)],
            vec![op!(Add), op!(Exit)]
        ),
        MachineValue::Uint64(42)
    );
}

#[test]
fn count_leading_zeros() {
    assert_eq!(
        top(
            &[MachineValue::Uint8(1)],
            vec![op!(CountLeadingZeros), op!(Exit)]
        ),
        MachineValue::Uint32(7)
    );
}

#[test]
fn count_leading_ones() {
    assert_eq!(
        top(
            &[MachineValue::Uint8(0b1111_0000)],
            vec![op!(CountLeadingOnes), op!(Exit)]
        ),
        MachineValue::Uint32(4)
    );
}

#[test]
fn count_trailing_zeros() {
    assert_eq!(
        top(
            &[MachineValue::Uint8(0b0000_1000)],
            vec![op!(CountTrailingZeros), op!(Exit)]
        ),
        MachineValue::Uint32(3)
    );
}

#[test]
fn count_trailing_ones() {
    assert_eq!(
        top(
            &[MachineValue::Uint8(0b0000_0111)],
            vec![op!(CountTrailingOnes), op!(Exit)]
        ),
        MachineValue::Uint32(3)
    );
}

#[test]
fn jump_skips_instructions() {
    let result = top(
        &[],
        vec![
            op!(Jump, Instruction(3)),
            op!(Push, Uint64(111)),
            op!(Exit),
            op!(Push, Uint64(222)),
            op!(Exit),
        ],
    );
    assert_eq!(result, u64v(222));
}

#[test]
fn jump_if_zero_taken() {
    let result = top(
        &[],
        vec![
            op!(Push, Uint64(0)),
            op!(JumpIfZero, Instruction(4)),
            op!(Push, Uint64(111)),
            op!(Exit),
            op!(Push, Uint64(222)),
            op!(Exit),
        ],
    );
    assert_eq!(result, u64v(222));
}

#[test]
fn jump_if_zero_not_taken() {
    let result = top(
        &[],
        vec![
            op!(Push, Uint64(5)),
            op!(JumpIfZero, Instruction(4)),
            op!(Push, Uint64(111)),
            op!(Exit),
            op!(Push, Uint64(222)),
            op!(Exit),
        ],
    );
    assert_eq!(result, u64v(111));
}

#[test]
fn jump_if_equal_taken() {
    let result = top(
        &[],
        vec![
            op!(Push, Uint64(7)),
            op!(Push, Uint64(7)),
            op!(JumpIfEqual, Instruction(5)),
            op!(Push, Uint64(111)),
            op!(Exit),
            op!(Push, Uint64(222)),
            op!(Exit),
        ],
    );
    assert_eq!(result, u64v(222));
}

#[test]
fn jump_if_equal_not_taken() {
    let result = top(
        &[],
        vec![
            op!(Push, Uint64(7)),
            op!(Push, Uint64(8)),
            op!(JumpIfEqual, Instruction(5)),
            op!(Push, Uint64(111)),
            op!(Exit),
            op!(Push, Uint64(222)),
            op!(Exit),
        ],
    );
    assert_eq!(result, u64v(111));
}

#[test]
fn pop_stores_and_push_loads_registers() {
    let mut machine = run(
        &[],
        vec![
            op!(Push, Uint64(42)),
            op!(Pop, Register5),
            op!(Push, Register5),
            op!(Exit),
        ],
    );
    assert_eq!(machine.state().pop().unwrap(), u64v(42));
    assert_eq!(machine.state().bank().load(Register5), Some(u64v(42)));
}

#[test]
fn inlined_execution_matches_uncompiled() {
    let program = RawProgram::new_owned(vec![
        op!(Push, Uint64(6)),
        op!(Push, Uint64(7)),
        op!(Multiply),
        op!(Exit),
    ]);

    let mut uncompiled = Machine::new(ops::all());
    uncompiled
        .run(&MachineProgram::Uncompiled(&program))
        .unwrap();

    let handlers = ops::all();
    let inlined = handlers.inline(&program).unwrap();
    let mut compiled = Machine::new(ops::all());
    compiled.run(&MachineProgram::Inlined(inlined)).unwrap();

    assert_eq!(uncompiled.state().pop().unwrap(), u64v(42));
    assert_eq!(compiled.state().pop().unwrap(), u64v(42));
}

#[test]
fn pop_on_empty_stack_errors() {
    let program = RawProgram::new_owned(vec![op!(Pop, Register5), op!(Exit)]);
    let mut machine = Machine::new(ops::all());
    let error = machine
        .run(&MachineProgram::Uncompiled(&program))
        .unwrap_err();
    assert_eq!(error, MachineError::StackEmpty);
}

#[test]
fn return_with_empty_call_stack_errors() {
    let program = RawProgram::new_owned(vec![op!(Return)]);
    let mut machine = Machine::new(ops::all());
    let error = machine
        .run(&MachineProgram::Uncompiled(&program))
        .unwrap_err();
    assert_eq!(error, MachineError::CallStackEmpty);
}

#[test]
fn running_off_the_end_overflows() {
    // No Exit: the machine steps past the last instruction.
    let program = RawProgram::new_owned(vec![op!(Push, Uint64(1))]);
    let mut machine = Machine::new(ops::all());
    let error = machine
        .run(&MachineProgram::Uncompiled(&program))
        .unwrap_err();
    assert_eq!(error, MachineError::InstructionOverflow);
}

#[test]
fn reset_clears_stack_and_registers() {
    let mut machine = run(
        &[],
        vec![
            op!(Push, Uint64(9)),
            op!(Pop, Register5),
            op!(Push, Uint64(1)),
            op!(Exit),
        ],
    );
    machine.state().reset();
    assert!(machine.state().pop().is_err());
    assert_eq!(
        machine.state().bank().load(Register5),
        Some(MachineValue::None)
    );
}
