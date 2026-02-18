use tinyvm::error::Result;
use tinyvm::machine::Machine;
use tinyvm::machine::value::MachineValue::Uint64;

pub mod fib;

const COUNT: usize = 10;
const INPUT: u64 = 60;

fn main() -> Result<()> {
    let mut machine = Machine::new(&fib::FIB);

    for i in 0..COUNT {
        println!("iteration: {}", i);
        machine.push(Uint64(INPUT));
        machine.run()?;
        let value = machine.pop()?;
        machine.reset();
        println!("value: {}", value.as_u64());
    }

    Ok(())
}
