use std::str::FromStr;
use std::time::Instant;
use tinyvm::machine::optimized::OptimizedProgram;
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineProgram, ops};
use tinyvm::program::RawProgram;

const INPUT: u64 = 2_000_000;
const RUNS: u32 = 20;
const OPS_PER_RUN: u64 = 9 + INPUT * 15;

fn bench(label: &str, mut run: impl FnMut()) {
    run();
    let start = Instant::now();
    for _ in 0..RUNS {
        run();
    }
    let elapsed = start.elapsed();
    let ops = (OPS_PER_RUN * RUNS as u64) as f64;
    let secs = elapsed.as_secs_f64();
    println!(
        "{label:<12} {:>7.1} M ops/s  {:>8.1} runs/s  ({elapsed:?})",
        ops / secs / 1e6,
        RUNS as f64 / secs
    );
}

fn main() {
    let text = std::fs::read_to_string("programs/fib.tinyvm").unwrap();
    let program = RawProgram::from_str(&text).unwrap();

    let mut modes = vec![
        ("uncompiled", MachineProgram::Uncompiled(&program)),
        (
            "inlined",
            MachineProgram::Inlined(ops::all().inline(&program).unwrap()),
        ),
        (
            "optimized",
            MachineProgram::Optimized(OptimizedProgram::compile(&program).unwrap()),
        ),
    ];

    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        use tinyvm::machine::jit::JitProgram;

        let optimized = OptimizedProgram::compile(&program).unwrap();
        modes.push((
            "jit",
            MachineProgram::Jit(JitProgram::compile(&optimized).unwrap()),
        ));
    }

    for (label, prog) in &modes {
        let mut machine = Machine::new(ops::all());
        bench(label, || {
            machine.state().reset();
            machine.state().push(MachineValue::Uint64(INPUT));
            machine.run(prog).unwrap();
        });
    }
}
