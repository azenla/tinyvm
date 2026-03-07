use tinyvm::machine::Machine;
use tinyvm::machine::error::Result;
use tinyvm::machine::value::MachineValue::Uint64;

pub mod fib;

const INPUT: u64 = 8;

fn main() -> Result<()> {
    let mut machine = Machine::new(&fib::FIB);

    machine.push(Uint64(INPUT));
    machine.run()?;
    let value = machine.pop()?;
    println!("fib({INPUT}) = {}", value.as_u64());

    Ok(())
}
