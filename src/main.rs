use tinyvm::error::Result;
use tinyvm::machine::Machine;
use tinyvm::machine::value::MachineValue::Uint64;

pub mod fib;

const COUNT: usize = 100000;
const INPUT: u64 = 60;

fn main() -> Result<()> {
    let mut machine = Machine::new(&fib::FIB);

    for _ in 0..COUNT {
        machine.push(Uint64(INPUT));
        machine.run()?;
        let _value = machine.pop()?;
        machine.reset();
    }

    Ok(())
}
