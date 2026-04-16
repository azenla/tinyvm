use tinyvm::machine::Machine;
use tinyvm::machine::error::Result;
use tinyvm::machine::value::MachineValue::Uint64;

pub mod fib;

const START: u64 = 8;
const END: u64 = 30;

fn main() -> Result<()> {
    let mut machine = Machine::new(&fib::FIB);

    for input in START..END + 1 {
        machine.push(Uint64(input));
        machine.run()?;
        let value = machine.pop()?;
        println!("fib({input}) = {}", value.as_u64());
        machine.reset();
    }

    Ok(())
}
