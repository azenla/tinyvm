//! Shows what each stage of the pipeline made of a program, and where the
//! program spends its time.
//!
//! Every stage records where its ops came from, and a `SourceMap` joins those
//! records, so each op can be followed from the lines it was written on,
//! through the superinstruction fusion collapsed it into and the rewrites the
//! optimizer applied, to the machine code the jit emitted for it: `inline` for
//! ops that become straight-line code, `fast` for ops with an inlined `Uint64`
//! path that falls back to a helper on other types, and `helper` for ops that
//! always make an out-of-line call. The program is then run to report the ops
//! by execution count.
//!
//! The headline number is the share of executed ops lowered to `helper`: those
//! pay a call per op no matter how hot they are, so it bounds what the jit can
//! win over the interpreter.
//!
//! usage: cargo run --release --example dump <program> [--code] [inputs...]

use std::collections::VecDeque;
use std::str::FromStr;
use tinyvm::machine::intermediate::IntermediateProgram;
use tinyvm::machine::optimizer::{OptimizedProgram, ValueType};
use tinyvm::machine::trace::{Lowering, SourceMap, Span};
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineLoopState, MachineProgram, ops};
use tinyvm::op::{Op, OpArg};
use tinyvm::program::RawProgram;

const DEFAULT_PROGRAM: &str = "programs/fib.tinyvm";

/// The jit is compiled in only on the platforms it supports, so what it
/// produced is reduced to this before the rest of the example touches it. The
/// per-op facts — lowering, code range, address — reach the example through
/// the map instead.
struct Native {
    base: usize,
    code: Vec<u8>,
    prologue: Span,
    epilogue: Span,
    /// Holding the compiled program keeps its code mapped, so the addresses
    /// printed below are addresses the code is really at.
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    _jit: tinyvm::machine::jit::JitProgram,
}

#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn compile_native(map: SourceMap, optimized: &OptimizedProgram) -> (SourceMap, Option<Native>) {
    use tinyvm::machine::jit::JitProgram;

    let jit = match JitProgram::compile_optimized(optimized) {
        Ok(jit) => jit,
        Err(error) => {
            eprintln!("jit unavailable: {error}");
            return (map, None);
        }
    };
    let native = Native {
        base: jit.code().as_ptr() as usize,
        code: jit.code().to_vec(),
        prologue: jit.prologue(),
        epilogue: jit.epilogue(),
        _jit: jit.clone(),
    };
    (map.with_jit(&jit), Some(native))
}

#[cfg(not(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn compile_native(map: SourceMap, _optimized: &OptimizedProgram) -> (SourceMap, Option<Native>) {
    (map, None)
}

fn value_type(arg: &OpArg) -> ValueType {
    match arg {
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
    }
}

/// Executes the program one op at a time, counting visits per program counter.
fn profile(optimized: &OptimizedProgram, values: &[MachineValue]) -> Vec<u64> {
    let mut counts = vec![0u64; optimized.ops().len()];
    let mut machine = Machine::new(ops::all());
    for value in values {
        machine.state().push(*value);
    }

    let program = MachineProgram::Optimized(optimized.clone());
    loop {
        let pc = machine.state().current();
        if let Some(count) = counts.get_mut(pc) {
            *count += 1;
        }
        match machine.step(&program) {
            Ok(MachineLoopState::Break) => break,
            Ok(_) => {}
            Err(error) => {
                eprintln!("stopped at {pc}: {error}");
                break;
            }
        }
    }
    counts
}

/// The ops of one raw span as they were written, which names a fused
/// superinstruction by the sequence it replaced.
fn source(raw: &[Op], span: Span) -> String {
    raw[span.indices()]
        .iter()
        .map(|op| op.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

fn label(lowering: Option<Lowering>) -> &'static str {
    lowering.map(|lowering| lowering.label()).unwrap_or("-")
}

/// Text lines as a reader counts them: one line, or the inclusive range the
/// fused raw ops were written across.
fn lines_of(span: Option<Span>) -> String {
    match span {
        Some(span) if span.len() == 1 => span.start.to_string(),
        Some(span) => format!("{}-{}", span.start, span.end.saturating_sub(1)),
        None => "-".into(),
    }
}

fn main() {
    let mut args = std::env::args().skip(1).collect::<VecDeque<_>>();
    let code_wanted = args.iter().any(|arg| arg == "--code");
    args.retain(|arg| arg != "--code");
    let path = args.pop_front().unwrap_or(DEFAULT_PROGRAM.into());

    let inputs = args
        .iter()
        .map(|item| OpArg::from_str(item).unwrap())
        .collect::<Vec<_>>();
    let types = inputs.iter().map(value_type).collect::<Vec<_>>();
    let values = inputs
        .iter()
        .copied()
        .map(MachineValue::from)
        .collect::<Vec<_>>();

    let text = std::fs::read_to_string(&path).unwrap();
    let (raw, lines) = RawProgram::parse_with_lines(&text).unwrap();
    let intermediate = IntermediateProgram::compile(&raw).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &types);

    let map = SourceMap::of(&intermediate)
        .with_lines(&lines)
        .with_optimized(&optimized);
    let (map, native) = compile_native(map, &optimized);

    println!(
        "{path}: {} raw ops -> {} fused -> {} optimized",
        raw.ops().len(),
        intermediate.ops().len(),
        optimized.ops().len()
    );
    if let Some(native) = &native {
        println!(
            "{} bytes of code at {:#x}: prologue {}, ops {}..{}, epilogue {}",
            native.code.len(),
            native.base,
            native.prologue,
            native.prologue.end,
            native.epilogue.start,
            native.epilogue,
        );
    }

    println!("\n--- fused and optimized ---");
    println!(
        "{:>6}  {:>6}  {:>5}  {:>7}  {:<12}  written",
        "lines", "raw", "fused", "opt", "rewrites"
    );
    for mapping in map.mappings() {
        let optimized = match mapping.optimized {
            Some(index) => index.to_string(),
            None => "dropped".into(),
        };
        println!(
            "{:>6}  {:>6}  {:>5}  {optimized:>7}  {:<12}  {}",
            lines_of(mapping.lines),
            mapping.raw.to_string(),
            mapping.intermediate,
            mapping.rewrites.to_string(),
            source(raw.ops(), mapping.raw),
        );
    }

    let counts = profile(&optimized, &values);

    println!("\n--- optimized and native ---");
    println!(
        "{:>4}  {:<7} {:>11} {:>10}  {:>6}  {:>6}  op",
        "pc", "lowered", "code", "executed", "lines", "raw"
    );
    for (pc, op) in optimized.ops().iter().enumerate() {
        let mapping = map.of_optimized(pc);
        let code = match mapping.and_then(|mapping| mapping.code) {
            Some(code) => format!("{}+{}", code.start, code.len()),
            None => "-".into(),
        };
        let raw_span = mapping
            .map(|mapping| mapping.raw.to_string())
            .unwrap_or("-".into());
        println!(
            "{pc:>4}  {:<7} {code:>11} {:>10}  {:>6}  {raw_span:>6}  {op:?}",
            label(mapping.and_then(|mapping| mapping.lowering)),
            counts[pc],
            lines_of(mapping.and_then(|mapping| mapping.lines)),
        );
    }

    if let (Some(native), true) = (&native, code_wanted) {
        println!("\n--- native code ---");
        for pc in 0..optimized.ops().len() {
            let Some(mapping) = map.of_optimized(pc) else {
                continue;
            };
            let Some(code) = mapping.code else { continue };
            println!(
                "{pc:>4}  {:<7} {:#x} {}  {}",
                label(mapping.lowering),
                mapping.address.unwrap_or(0),
                code,
                source(raw.ops(), mapping.raw),
            );
            for chunk in native.code[code.indices()].chunks(16) {
                let hex = chunk
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("      {hex}");
            }
        }
    }

    let executed: u64 = counts.iter().sum();
    if executed == 0 {
        return;
    }

    println!("\n--- hottest ops ---");
    let mut order = (0..optimized.ops().len()).collect::<Vec<_>>();
    order.sort_by(|&a, &b| counts[b].cmp(&counts[a]).then(a.cmp(&b)));
    let mut cumulative = 0u64;
    for pc in order.into_iter().take(10) {
        if counts[pc] == 0 {
            break;
        }
        cumulative += counts[pc];
        let mapping = map.of_optimized(pc);
        println!(
            "{:>5.1}%  {:>5.1}% cumulative  {:<7} {:>4}  {}",
            counts[pc] as f64 / executed as f64 * 100.0,
            cumulative as f64 / executed as f64 * 100.0,
            label(mapping.and_then(|mapping| mapping.lowering)),
            pc,
            mapping
                .map(|mapping| source(raw.ops(), mapping.raw))
                .unwrap_or_else(|| format!("{:?}", optimized.ops()[pc])),
        );
    }

    if native.is_none() {
        return;
    }

    // Each lowering's share of the ops actually executed, and of the code the
    // jit spent on them.
    println!("\n--- executed op mix ({executed} ops) ---");
    for kind in Lowering::ALL {
        let mut ops = 0u64;
        let mut bytes = 0usize;
        for (pc, count) in counts.iter().enumerate() {
            let Some(mapping) = map.of_optimized(pc) else {
                continue;
            };
            if mapping.lowering != Some(*kind) {
                continue;
            }
            ops += count;
            bytes += mapping.code.map(|code| code.len()).unwrap_or(0);
        }
        println!(
            "{:>5.1}%  {:<7} {bytes:>5} bytes of code",
            ops as f64 / executed as f64 * 100.0,
            kind.label(),
        );
    }
}
