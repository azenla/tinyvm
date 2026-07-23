use std::str::FromStr;
use std::time::Instant;
use tinyvm::machine::intermediate::IntermediateProgram;
use tinyvm::machine::optimizer::{OptimizedProgram, ValueType};
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineProgram, ops};
use tinyvm::program::RawProgram;

const INPUT: u64 = 2_000_000;
const RUNS: u32 = 20;
const OPS_PER_RUN: u64 = 9 + INPUT * 15;

// Modes that fuse or fold ops execute fewer dispatches than the raw program,
// so their rate is reported in virtual ops — raw ops retired per second —
// labeled "vops" instead of "ops".
fn bench(label: &str, unit: &str, mut run: impl FnMut()) {
    run();
    let start = Instant::now();
    for _ in 0..RUNS {
        run();
    }
    let elapsed = start.elapsed();
    let ops = (OPS_PER_RUN * RUNS as u64) as f64;
    let secs = elapsed.as_secs_f64();
    println!(
        "{label:<12} {:>7.1} M {unit:>4}/s  {:>8.1} runs/s  ({elapsed:?})",
        ops / secs / 1e6,
        RUNS as f64 / secs
    );
}

fn main() {
    let text = std::fs::read_to_string("programs/fib.tinyvm").unwrap();
    let program = RawProgram::from_str(&text).unwrap();

    let intermediate = IntermediateProgram::compile(&program).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &[ValueType::Uint64]);

    let mut modes = vec![
        ("uncompiled", "ops", MachineProgram::Uncompiled(&program)),
        (
            "inlined",
            "ops",
            MachineProgram::Inlined(ops::all().inline(&program).unwrap()),
        ),
        (
            "intermediate",
            "vops",
            MachineProgram::Intermediate(intermediate),
        ),
        (
            "optimized",
            "vops",
            MachineProgram::Optimized(optimized.clone()),
        ),
    ];

    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        use tinyvm::machine::jit::JitProgram;

        modes.push((
            "jit",
            "vops",
            MachineProgram::Jit(JitProgram::compile_optimized(&optimized).unwrap()),
        ));
    }

    for (label, unit, prog) in &modes {
        let mut machine = Machine::new(ops::all());
        bench(label, unit, || {
            machine.state().reset();
            machine.state().push(MachineValue::Uint64(INPUT));
            machine.run(prog).unwrap();
        });
    }
}
