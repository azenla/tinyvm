use std::path::Path;
use std::str::FromStr;
use tinyvm::machine::error::Result;
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

    let content = std::fs::read_to_string(path).unwrap();
    let program = RawProgram::from_str(&content).unwrap();
    let ops = ops::all();
    let inlined = ops.inline(&program)?;
    let program = MachineProgram::Inlined(inlined);
    let mut machine = Machine::new(ops);

    for arg in args.iter().skip(2) {
        let argument = OpArg::from_str(arg).unwrap();
        let value: MachineValue = argument.into();
        machine.state().push(value);
    }

    machine.run(&program)?;
    let result = machine.state().pop()?;
    println!("{:?}", result);
    Ok(())
}
