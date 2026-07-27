//! Shows how a program lowers and where it spends its time.
//!
//! For each op of the optimized program this prints how the jit lowers it:
//! `inline` for ops that become straight-line machine code, `fast` for ops
//! with an inlined `Uint64` path that falls back to a helper on other types,
//! and `helper` for ops that always make an out-of-line call. It then runs the
//! program and reports the ops by execution count.
//!
//! The headline number is the share of executed ops lowered to `helper`: those
//! pay a call per op no matter how hot they are, so it bounds what the jit can
//! win over the interpreter.
//!
//! usage: cargo run --release --example dump <program> [inputs...]

use std::collections::VecDeque;
use std::str::FromStr;
use tinyvm::machine::intermediate::{BinaryOpKind, IntermediateOp, IntermediateProgram, Source};
use tinyvm::machine::optimizer::{OptimizedProgram, RegisterTypes, ValueType};
use tinyvm::machine::value::MachineValue;
use tinyvm::machine::{Machine, MachineLoopState, MachineProgram, ops};
use tinyvm::op::OpArg;
use tinyvm::program::RawProgram;

const DEFAULT_PROGRAM: &str = "programs/fib.tinyvm";

/// How the jit lowers one op, mirroring the cases in `jit::emit`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lowering {
    /// Straight-line machine code, no call.
    Inline,
    /// Inlined `Uint64` path, out-of-line helper for anything else.
    Fast,
    /// Always an out-of-line helper call.
    Helper,
}

impl Lowering {
    fn label(&self) -> &'static str {
        match self {
            Lowering::Inline => "inline",
            Lowering::Fast => "fast",
            Lowering::Helper => "helper",
        }
    }
}

/// An operand is eligible for an inlined fast path unless it is a constant of
/// some type other than `Uint64`.
fn fast_operand(source: &Source) -> bool {
    !matches!(source, Source::Value(value) if !matches!(value, MachineValue::Uint64(_)))
}

fn lowering(op: &IntermediateOp, types: &RegisterTypes) -> Lowering {
    match op {
        // Control flow the backend resolves to a fixed address.
        IntermediateOp::Jump(_) => Lowering::Inline,

        // Stack traffic, inlined against the stack's own layout: a bounds
        // compare, a sixteen-byte copy, and a length write-back. The helper
        // takes over only to grow a full stack or report an empty one.
        IntermediateOp::PushValue(_)
        | IntermediateOp::PushRegister(_)
        | IntermediateOp::PopRegister(_)
        | IntermediateOp::Call(_) => Lowering::Fast,

        // Also inlined, plus a tag check and a range check before dispatching
        // through the entry table.
        IntermediateOp::Return => Lowering::Fast,

        // Register-to-register arithmetic. Divide and remainder stay in the
        // helper so a zero divisor reaches the interpreter instead of trapping.
        IntermediateOp::Binary { kind, lhs, rhs, .. } => {
            let divides = matches!(kind, BinaryOpKind::Divide | BinaryOpKind::Remainder);
            if !divides && fast_operand(lhs) && fast_operand(rhs) {
                Lowering::Fast
            } else {
                Lowering::Helper
            }
        }

        // A copy is a payload move only when the source is a proven `Uint64`;
        // otherwise the whole tagged slot goes through memory.
        IntermediateOp::Copy { src, .. } => match src {
            Source::Register(index) if types.get(*index) == ValueType::Uint64 => Lowering::Inline,
            Source::Value(MachineValue::Uint64(_)) => Lowering::Inline,
            _ => Lowering::Fast,
        },

        IntermediateOp::JumpIfZeroValue { src, .. } => match src {
            Source::Register(_) => Lowering::Fast,
            Source::Value(_) => Lowering::Inline,
        },
        IntermediateOp::JumpIfEqualValues { lhs, rhs, .. } => {
            if matches!((lhs, rhs), (Source::Value(_), Source::Value(_))) {
                Lowering::Inline
            } else if fast_operand(lhs) && fast_operand(rhs) {
                Lowering::Fast
            } else {
                Lowering::Helper
            }
        }

        // Everything else: arithmetic and conditions that never got fused, so
        // they still take their operands off the stack inside a helper.
        _ => Lowering::Helper,
    }
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

fn main() {
    let mut args = std::env::args().skip(1).collect::<VecDeque<_>>();
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
    let raw = RawProgram::from_str(&text).unwrap();
    let intermediate = IntermediateProgram::compile(&raw).unwrap();
    let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &types);

    println!(
        "{path}: {} raw ops -> {} fused -> {} optimized",
        raw.ops().len(),
        intermediate.ops().len(),
        optimized.ops().len()
    );

    let counts = profile(&optimized, &values);
    let lowerings = optimized
        .ops()
        .iter()
        .zip(optimized.types())
        .map(|(op, types)| lowering(op, types))
        .collect::<Vec<_>>();

    println!("\n--- program ---");
    println!("{:>4}  {:<7} {:>12}  op", "pc", "lowered", "executed");
    for (pc, op) in optimized.ops().iter().enumerate() {
        println!(
            "{pc:>4}  {:<7} {:>12}  {op:?}",
            lowerings[pc].label(),
            counts[pc]
        );
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
        println!(
            "{:>5.1}%  {:>5.1}% cumulative  {:<7} {:>4}  {:?}",
            counts[pc] as f64 / executed as f64 * 100.0,
            cumulative as f64 / executed as f64 * 100.0,
            lowerings[pc].label(),
            pc,
            optimized.ops()[pc]
        );
    }

    let share = |kind: Lowering| {
        let total: u64 = counts
            .iter()
            .zip(&lowerings)
            .filter(|(_, lowering)| **lowering == kind)
            .map(|(count, _)| count)
            .sum();
        total as f64 / executed as f64 * 100.0
    };
    println!("\n--- executed op mix ({executed} ops) ---");
    for kind in [Lowering::Inline, Lowering::Fast, Lowering::Helper] {
        println!("{:>5.1}%  {}", share(kind), kind.label());
    }
}
