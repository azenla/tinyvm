//! Runs one program through every tier the platform supports with the same
//! inputs and exits nonzero if any result disagrees — a scriptable check of
//! the invariant that all tiers produce identical results.
//!
//! A result is the value popped after the run, compared via `Debug` so tags
//! count, or the error variant when the run fails. The pc recorded on failure
//! is not compared: tiers 1–2 index raw ops while tiers 3–5 index the fused
//! program, so failure pcs only correspond through the source map.
//!
//! usage: cargo run --release --example xcheck <program> [inputs...]
use std::str::FromStr;
use tinyvm::machine::intermediate::IntermediateProgram;
use tinyvm::machine::optimizer::{OptimizedProgram, ValueType};
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineProgram, ops};
use tinyvm::op::OpArg;
use tinyvm::program::RawProgram;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let text = std::fs::read_to_string(&args[0]).unwrap();
    let program = RawProgram::from_str(&text).unwrap();
    let inputs: Vec<OpArg> = args[1..]
        .iter()
        .map(|a| OpArg::from_str(a).unwrap())
        .collect();
    let types: Vec<ValueType> = inputs
        .iter()
        .map(|i| match i {
            OpArg::Uint64(_) => ValueType::Uint64,
            OpArg::Uint32(_) => ValueType::Uint32,
            _ => panic!("only u64/u32 inputs in this checker"),
        })
        .collect();
    let values: Vec<MachineValue> = inputs.iter().map(|&i| i.into()).collect();

    let intermediate = IntermediateProgram::compile(&program).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &types);

    let mut tiers: Vec<(&str, MachineProgram)> = vec![
        ("uncompiled", MachineProgram::Uncompiled(&program)),
        (
            "inlined",
            MachineProgram::Inlined(ops::all().inline(&program).unwrap()),
        ),
        ("intermediate", MachineProgram::Intermediate(intermediate)),
        ("optimized", MachineProgram::Optimized(optimized.clone())),
    ];
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    tiers.push((
        "jit",
        MachineProgram::Jit(
            tinyvm::machine::jit::JitProgram::compile_optimized(&optimized).unwrap(),
        ),
    ));

    let mut results = Vec::new();
    for (name, tier) in &tiers {
        let mut machine = Machine::new(ops::all());
        for value in &values {
            machine.state().push(*value);
        }
        let outcome = machine
            .run(tier)
            .and_then(|_| machine.state().pop())
            .map(|v| format!("{v:?}"))
            .unwrap_or_else(|e| format!("error: {e:?}"));
        results.push((*name, outcome));
    }

    let first = &results[0].1;
    let agree = results.iter().all(|(_, r)| r == first);
    for (name, result) in &results {
        println!("  {name:<13} {result}");
    }
    println!(
        "{} {}",
        if agree { "AGREE" } else { "DISAGREE" },
        args.join(" ")
    );
    if !agree {
        std::process::exit(1);
    }
}
