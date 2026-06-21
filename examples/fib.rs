use std::time::Instant;
use tinyvm::machine::error::Result;
use tinyvm::machine::ops::OpHandlerSet;
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineProgram, ops};
use tinyvm::op::OpArg::{Instruction, Register1, Register2, Register3, Register4, Uint64};
use tinyvm::op::OpCode::{Add, Exit, Jump, JumpIfZero, Pop, Push, Subtract};
use tinyvm::program::RawProgram;
use tinyvm::{op, program};

const START: u64 = 8;
const END: u64 = 30;
const ITERATIONS: u64 = 100000;

pub static FIB: RawProgram = program!(
    // Pop the input value into r3.
    op!(Pop, Register3),
    // fib(0) = 0: store in r1.
    op!(Push, Uint64(0)),
    op!(Pop, Register1),
    // fib(1) = 1: store in r2.
    op!(Push, Uint64(1)),
    op!(Pop, Register2),
    // Pushes the counter-value. (loop start: instruction 5)
    op!(Push, Register3),
    // Exit loop if counter-value == 0.
    op!(JumpIfZero, Instruction(20)),
    // Calculate next fibonacci: next = r1 + r2
    op!(Push, Register1),
    op!(Push, Register2),
    op!(Add),
    // Store result in r4.
    op!(Pop, Register4),
    // Shift values in registers: r1 => R2, r2 => next
    op!(Push, Register2),
    op!(Pop, Register1),
    op!(Push, Register4),
    op!(Pop, Register2),
    // Decrement the counter-value: r3 = r3 - 1
    op!(Push, Register3),
    op!(Push, Uint64(1)),
    op!(Subtract),
    op!(Pop, Register3),
    // Jump back to the loop start.
    op!(Jump, Instruction(5)),
    // Push the result to the stack.
    op!(Push, Register1),
    // Exit.
    op!(Exit)
);

fn run(machine: &mut Machine, program: &MachineProgram, print: bool) -> Result<()> {
    for input in START..END + 1 {
        machine.state().push(MachineValue::Uint64(input));
        machine.run(program)?;
        let value = machine.state().pop()?;
        if print {
            println!("fib({}) = {}", input, value.as_u64());
        }
        machine.state().reset();
    }
    Ok(())
}

fn test(program: MachineProgram, ops: OpHandlerSet) -> Result<()> {
    let mut machine = Machine::new(ops);
    run(&mut machine, &program, true)?;

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        run(&mut machine, &program, false)?;
    }
    let duration = start.elapsed();
    println!("total: {} us", duration.as_micros());
    println!(
        "average: {} us",
        duration.as_micros() as f64 / ITERATIONS as f64
    );
    Ok(())
}

fn main() -> Result<()> {
    let source = &FIB;
    let handlers = ops::all();

    println!("== program ==");
    for op in source.ops() {
        println!("{}", op);
    }

    println!("== uncompiled ==");
    let uncompiled = MachineProgram::Uncompiled(source);
    test(uncompiled, handlers.clone())?;

    println!("== inlined ==");
    let inlined = MachineProgram::Inlined(handlers.inline(source)?);
    test(inlined, handlers.clone())?;
    Ok(())
}
