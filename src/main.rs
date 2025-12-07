use std::time::Instant;
use tinyvm::error::Result;
use tinyvm::machine::Machine;
use tinyvm::machine::value::MachineValue::Uint64;

pub mod fib;

const COUNT: usize = 1_000_000;

fn main() -> Result<()> {
    let mut machine = Machine::new(&fib::FIB);

    let time = Instant::now();
    for _ in 0..COUNT {
        machine.push(Uint64(60));
        machine.run()?;
        let _value = machine.pop()?;
        machine.reset();
    }
    let time = time.elapsed();
    println!("one million iterations: {}ms", time.as_millis());

    Ok(())
}
