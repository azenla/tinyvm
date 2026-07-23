use crate::machine::intermediate::IntermediateProgram;
use crate::machine::ops::OpHandlerSet;
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineProgram, ops};
use crate::program::RawProgram;
use std::str::FromStr;

mod constants;

#[test]
fn textual_decode() {
    let decoded = RawProgram::from_str(constants::FIB_PROGRAM_TEXT).unwrap();
    assert_eq!(decoded, constants::FIB_PROGRAM);
}

#[test]
fn survives_encode_decode() {
    let encoded = constants::FIB_PROGRAM.encode();
    let decoded = RawProgram::decode(&encoded).unwrap();
    assert_eq!(decoded, constants::FIB_PROGRAM);
}

fn run_fib_program(ops: OpHandlerSet, program: MachineProgram) -> MachineValue {
    let mut machine = Machine::new(ops);
    machine.state().push(MachineValue::Uint64(30));
    machine.run(&program).unwrap();
    machine.state().pop().unwrap()
}

#[test]
fn runs_uncompiled() {
    let program = MachineProgram::Uncompiled(&constants::FIB_PROGRAM);
    let value = run_fib_program(ops::all(), program);
    assert_eq!(value, MachineValue::Uint64(832040));
}

#[test]
fn runs_inlined() {
    let ops = ops::all();
    let inlined = ops.inline(&constants::FIB_PROGRAM).unwrap();
    let program = MachineProgram::Inlined(inlined);
    let value = run_fib_program(ops::all(), program);
    assert_eq!(value, MachineValue::Uint64(832040));
}

#[test]
fn runs_intermediate() {
    let intermediate = IntermediateProgram::compile(&constants::FIB_PROGRAM).unwrap();
    let program = MachineProgram::Intermediate(intermediate);
    let value = run_fib_program(ops::all(), program);
    assert_eq!(value, MachineValue::Uint64(832040));
}

#[test]
fn runs_optimized() {
    use crate::machine::optimizer::{OptimizedProgram, ValueType};

    let intermediate = IntermediateProgram::compile(&constants::FIB_PROGRAM).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &[ValueType::Uint64]);
    let program = MachineProgram::Optimized(optimized);
    let value = run_fib_program(ops::all(), program);
    assert_eq!(value, MachineValue::Uint64(832040));
}

#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn runs_jit() {
    use crate::machine::jit::JitProgram;

    let intermediate = IntermediateProgram::compile(&constants::FIB_PROGRAM).unwrap();
    let program = MachineProgram::Jit(JitProgram::compile(&intermediate).unwrap());
    let value = run_fib_program(ops::all(), program);
    assert_eq!(value, MachineValue::Uint64(832040));
}

#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn runs_jit_optimized() {
    use crate::machine::jit::JitProgram;
    use crate::machine::optimizer::{OptimizedProgram, ValueType};

    let intermediate = IntermediateProgram::compile(&constants::FIB_PROGRAM).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &[ValueType::Uint64]);
    let program = MachineProgram::Jit(JitProgram::compile_optimized(&optimized).unwrap());
    let value = run_fib_program(ops::all(), program);
    assert_eq!(value, MachineValue::Uint64(832040));
}
