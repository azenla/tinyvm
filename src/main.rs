use std::path::Path;
use std::str::FromStr;
use tinyvm::machine::error::Result;
use tinyvm::machine::intermediate::IntermediateProgram;
use tinyvm::machine::jit::JitProgram;
use tinyvm::machine::optimizer::{OptimizedProgram, ValueType};
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineProgram, ops};
use tinyvm::op::OpArg;
use tinyvm::program::RawProgram;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().map(|arg| arg.to_string()).collect();
    if args.len() < 2 {
        eprintln!("usage: tinyvm <program>");
        std::process::exit(1);
    }
    let path = Path::new(&args[1]);

    if !path.exists() {
        eprintln!("file '{}' does not exist", path.display());
        std::process::exit(1);
    }

    let mut inputs = Vec::new();

    for arg in args.iter().skip(2) {
        let argument = OpArg::from_str(arg).unwrap();
        let value: MachineValue = argument.into();
        inputs.push(value);
    }

    let content = std::fs::read_to_string(path).unwrap();
    let program = RawProgram::from_str(&content).unwrap();
    let ops = ops::all();
    let intermediate = IntermediateProgram::compile(&program)?;
    let optimized = OptimizedProgram::compile_with_inputs(
        &intermediate,
        &inputs.iter().map(ValueType::of).collect::<Vec<_>>(),
    );

    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    let program = {
        let jit = JitProgram::compile_optimized(&optimized)?;
        MachineProgram::Jit(jit)
    };

    #[cfg(not(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    let program = MachineProgram::Optimized(optimized);

    let mut machine = Machine::new(ops);

    for input in inputs {
        machine.state().push(input);
    }

    machine.run(&program)?;
    let result = machine.state().pop()?;
    println!("{:?}", result);
    Ok(())
}
