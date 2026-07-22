use crate::machine::compiled::CompiledProgram;
use crate::machine::ops::OpHandlerSet;
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineProgram, ops};
use crate::op::OpArg::{Instruction, Uint64};
use crate::op::OpCode::{Call, Exit, Push, Return};
use crate::program::RawProgram;
use crate::{op, program};

// A subroutine is invoked with `call`, and execution must resume at the
// instruction immediately following the `call` once it returns.
//
//   0: call 3   -> record return address 1, jump to 3
//   1: push 99  -> must run right after the subroutine returns
//   2: exit
//   3: push 42  -> subroutine body
//   4: ret      -> resume at instruction 1
static CALL_PROGRAM: RawProgram = program!(
    op!(Call, Instruction(3)),
    op!(Push, Uint64(99)),
    op!(Exit),
    op!(Push, Uint64(42)),
    op!(Return),
);

fn run(handlers: OpHandlerSet, program: MachineProgram) -> MachineValue {
    let mut machine = Machine::new(handlers);
    machine.run(&program).unwrap();
    machine.state().pop().unwrap()
}

#[test]
fn returns_to_instruction_after_call_uncompiled() {
    let program = MachineProgram::Uncompiled(&CALL_PROGRAM);
    assert_eq!(run(ops::all(), program), MachineValue::Uint64(99));
}

#[test]
fn returns_to_instruction_after_call_inlined() {
    let ops = ops::all();
    let inlined = ops.inline(&CALL_PROGRAM).unwrap();
    let program = MachineProgram::Inlined(inlined);
    assert_eq!(run(ops::all(), program), MachineValue::Uint64(99));
}

#[test]
fn returns_to_instruction_after_call_compiled() {
    let compiled = CompiledProgram::compile(&CALL_PROGRAM).unwrap();
    let program = MachineProgram::Compiled(compiled);
    assert_eq!(run(ops::all(), program), MachineValue::Uint64(99));
}
