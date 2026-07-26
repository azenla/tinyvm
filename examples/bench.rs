use std::collections::VecDeque;
use std::str::FromStr;
use std::time::Instant;
use tinyvm::machine::intermediate::IntermediateProgram;
use tinyvm::machine::optimizer::{OptimizedProgram, ValueType};
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineLoopState, MachineProgram, ops};
use tinyvm::op::OpArg;
use tinyvm::program::RawProgram;

const DEFAULT_PROGRAM: &str = "programs/fib.tinyvm";
const DEFAULT_RUNS: u64 = 20;
const DEFAULT_INPUTS: &[&str] = &["2000000u64"];

fn bench(runs: u64, ops_per_run: u64, label: &str, unit: &str, mut run: impl FnMut()) {
    run();
    let start = Instant::now();
    for _ in 0..runs {
        run();
    }
    let elapsed = start.elapsed();
    let ops = (ops_per_run * runs) as f64;
    let secs = elapsed.as_secs_f64();
    println!(
        "{label:<12} {:>7.1} M {unit:>4}/s  {:>8.1} runs/s  ({elapsed:?})",
        ops / secs / 1e6,
        runs as f64 / secs
    );
}

fn main() {
    let mut args = std::env::args().skip(1).collect::<VecDeque<_>>();
    let program_path = args.pop_front().unwrap_or(DEFAULT_PROGRAM.into());
    let runs = args
        .pop_front()
        .unwrap_or(DEFAULT_RUNS.to_string())
        .parse::<u64>()
        .unwrap();

    if args.is_empty() {
        args = DEFAULT_INPUTS.iter().map(|&s| s.into()).collect();
    }

    let inputs = args
        .iter()
        .map(|item| OpArg::from_str(item).unwrap())
        .collect::<Vec<_>>();
    let types = inputs
        .iter()
        .map(|input| match input {
            OpArg::None => ValueType::None,
            OpArg::Uint8(_) => ValueType::Uint8,
            OpArg::Uint16(_) => ValueType::Uint16,
            OpArg::Uint32(_) => ValueType::Uint32,
            OpArg::Uint64(_) => ValueType::Uint64,
            OpArg::Int8(_) => ValueType::Int8,
            OpArg::Int16(_) => ValueType::Int16,
            OpArg::Int32(_) => ValueType::Int32,
            OpArg::Int64(_) => ValueType::Int64,
            OpArg::Instruction(_) => ValueType::ReturnAddress,
            _ => panic!("bad input type"),
        })
        .collect::<Vec<_>>();
    let text = std::fs::read_to_string(program_path).unwrap();
    let program = RawProgram::from_str(&text).unwrap();

    let intermediate = IntermediateProgram::compile(&program).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &types);

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

    let values = inputs
        .iter()
        .map(|input| MachineValue::from(*input))
        .collect::<Vec<_>>();

    let mut ops_per_run = 0u64;
    {
        let mut machine = Machine::new(ops::all());
        let uncompiled = MachineProgram::Uncompiled(&program);
        for item in &values {
            machine.state().push(*item);
        }
        loop {
            let result = machine.step(&uncompiled).unwrap();
            ops_per_run += 1;
            if result == MachineLoopState::Break {
                break;
            }
        }
    }

    for (label, unit, program) in &modes {
        let mut machine = Machine::new(ops::all());
        bench(runs, ops_per_run, label, unit, || {
            machine.state().reset();
            for item in &values {
                machine.state().push(*item);
            }
            machine.run(program).unwrap();
        });
    }
}
