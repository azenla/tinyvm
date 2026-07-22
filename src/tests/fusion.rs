use crate::machine::compiled::CompiledProgram;
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineProgram, ops};
use crate::op;
use crate::op::OpArg::{Instruction, Register1, Register2, Register3, Uint8, Uint16, Uint64};
use crate::op::OpCode::{Add, Exit, Jump, JumpIfEqual, Pop, Push};
use crate::program;
use crate::program::RawProgram;

fn run_compiled(program: &RawProgram) -> MachineValue {
    let compiled = CompiledProgram::compile(program).unwrap();
    let mut machine = Machine::new(ops::all());
    machine.run(&MachineProgram::Compiled(compiled)).unwrap();
    machine.state().pop().unwrap()
}

fn run_uncompiled(program: &RawProgram) -> MachineValue {
    let mut machine = Machine::new(ops::all());
    machine.run(&MachineProgram::Uncompiled(program)).unwrap();
    machine.state().pop().unwrap()
}

#[test]
fn fuses_stack_neutral_sequences() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Register1),
        op!(Push, Register2),
        op!(Add),
        op!(Pop, Register3),
        op!(Exit),
    );
    let compiled = CompiledProgram::compile(&PROGRAM).unwrap();
    assert_eq!(compiled.ops().len(), 2);
}

// The jump lands on the `pop`, so `push 99; pop r1` must not be fused into a
// copy: the pop has to consume the 5 pushed at instruction 0.
#[test]
fn jump_target_inside_fusible_pair_blocks_fusion() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(5)),
        op!(Jump, Instruction(3)),
        op!(Push, Uint64(99)),
        op!(Pop, Register1),
        op!(Push, Register1),
        op!(Exit),
    );
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(5));
    assert_eq!(run_compiled(&PROGRAM), MachineValue::Uint64(5));
}

// Mixed-type equality is not symmetric: Uint8(5) == Uint16(261) truncates the
// right side to 5 and matches, while the reverse comparison does not. The
// fused jump-if-equal must compare in the same order as the stack ops did.
#[test]
fn fused_jump_if_equal_preserves_operand_order() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint16(261)),
        op!(Push, Uint8(5)),
        op!(JumpIfEqual, Instruction(5)),
        op!(Push, Uint64(0)),
        op!(Exit),
        op!(Push, Uint64(1)),
        op!(Exit),
    );
    assert_eq!(run_uncompiled(&PROGRAM), MachineValue::Uint64(1));
    assert_eq!(run_compiled(&PROGRAM), MachineValue::Uint64(1));
}
