use std::time::Instant;
use tinyvm::machine::error::Result;
use tinyvm::machine::value::MachineValue::Uint64;
use tinyvm::machine::{Machine, MachineProgram, ops};

pub mod fib;

const START: u64 = 8;
const END: u64 = 30;
const ITERATIONS: u64 = 100000;

fn run(machine: &mut Machine, program: &MachineProgram, print: bool) -> Result<()> {
    for input in START..END + 1 {
        machine.state_mut().push(Uint64(input));
        machine.run(program)?;
        let value = machine.state_mut().pop()?;
        if print {
            println!("fib({input}) = {}", value.as_u64());
        }
        machine.state_mut().reset();
    }
    Ok(())
}

fn main() -> Result<()> {
    let handlers = ops::all();
    let program = MachineProgram::Inlined(handlers.inline(&fib::FIB)?);
    let mut machine = Machine::new(ops::all());
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
