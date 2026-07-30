# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A small bytecode VM in Rust with **five execution tiers** over one op set: two naive
interpreters, a fusing interpreter, an optimizing interpreter, and a native JIT with
x86-64 and aarch64 backends. No dependencies except `libc` on unix (for `mmap`). Toolchain
is pinned to 1.96.0, edition 2024 — let-chains (`if let … && let …`) are used heavily.

The central invariant: **all five tiers must produce identical results**, including value
tags, error variants, and the program counter recorded on failure. The intermediate
interpreter (`IntermediateOp::perform`) is the reference semantics; the JIT's `helper`
functions exist to reproduce it exactly on paths native code cannot handle inline.

## Commands

```sh
cargo test                                # 87 unit tests + 1 doctest, ~1s
cargo test optimizer::folds_constant      # single test / module by substring
cargo test --target x86_64-apple-darwin   # exercise the x86 JIT backend (Rosetta)
cargo clippy --workspace --all-targets    # kept warning-clean
./hack/format.sh                          # cargo fmt + shfmt on hack/*.sh
./hack/autofix.sh                         # clippy --fix, then format

cargo run --release -- programs/fib.tinyvm 30u64          # run a program (inlined tier)
cargo run --release --example bench                       # all tiers, fib(2000000) x20
cargo run --release --example bench programs/ack.tinyvm 5 3u64 6u64
cargo run --release --example dump programs/fib.tinyvm --code 2000000u64
```

`cargo test` alone only covers the host architecture's JIT backend. **A JIT change is not
tested until it has run on both** — `--target x86_64-apple-darwin` works on Apple silicon
via Rosetta; `aarch64-*` and `x86_64-*` linux/windows targets are installed for `cargo
check`.

`examples/dump` is the tool for understanding what the pipeline did to a program: raw ops →
fused → optimized, which rewrites fired, how the JIT lowered each op (`inline`/`fast`/
`helper`), the native byte ranges, and an execution profile. The headline figure is the
share of *executed* ops lowered to `helper`, which bounds what the JIT can win.

`examples/bench` prints M ops/s and runs/s per tier. Between builds, code-layout noise
moves the per-op figures by a few percent even with identical logic; compare `runs/s` and
re-run before believing a small regression.

Profiles: `release-debuginfo` (release + symbols, for profiling generated code),
`dev-fast` (debug without debuginfo, for quick test cycles).

## Op set and program representation

`Op` is `OpCode` + `OpArg`, 10 bytes encoded (opcode byte, arg tag byte, 8-byte payload).
17 opcodes, 9 globally-visible registers `r1`–`r9`, two stacks (values and call/return
addresses), no heap. Textual form is one op per line, `#` comments and blank lines skipped:
`push 5u64`, `pop r3`, `jmp 20p`. The `p` suffix is an absolute **raw op index** — comment
and blank lines do not count, so editing a program's comments is safe but inserting an op
renumbers every jump after it. `programs/ack.tinyvm` documents the calling convention a
recursive program has to invent for itself, since registers are global.

`MachineValue` is a tagged 16-byte value. Arithmetic coerces the right operand to the left
operand's type and wraps (`wrapping_add` etc.); division by zero panics rather than
trapping. Mixed-type `PartialEq` coerces to the left side, so **equality is not symmetric**
— `rhs == lhs` versus `lhs == rhs` is a real distinction that the interpreter, the
optimizer's folding, and the JIT's comparisons all have to keep in the same order.

## The tiers

| Tier | Type | Built by | Notes |
| --- | --- | --- | --- |
| `Uncompiled` | `&RawProgram` | — | opcode → handler lookup per op |
| `Inlined` | `InlinedOpHandlers` | `OpHandlerSet::inline` | handler fn pointers resolved once |
| `Intermediate` | `IntermediateProgram` | `::compile(&raw)` | decoded ops + superinstruction fusion |
| `Optimized` | `OptimizedProgram` | `::compile_with_inputs(&intermediate, types)` | dataflow analysis, folding, threading |
| `Jit` | `JitProgram` | `::compile_optimized(&optimized)` | native code; `step` is unsupported |

`Machine::run`/`step` dispatch on `MachineProgram`. `MachineState` holds both stacks, the
register bank, and `current` (the pc). Tiers 1–2 index raw ops; tiers 3–5 index the fused
program, which is why `trace::SourceMap` exists.

### Fusion (`machine/intermediate.rs`)

`IntermediateOp::fuse` collapses stack-neutral windows into register-to-register
superinstructions: `push/push/add/pop rN` → `Binary { kind, lhs, rhs, dst }`,
`push/jiz` → `JumpIfZeroValue`, `push/pop` → `Copy`, and so on. Two rules keep this sound:

- Control flow may never land *inside* a fused window, so positions `1..length` must not be
  jump targets. `targets` marks jump destinations **and the op after every `call`**, since a
  return lands there.
- Fusion shortens the program, so every jump target is remapped through `map`; targets past
  the end are shifted to stay past the end, preserving `InstructionOverflow`.

`origins()` and `destinations()` record the raw ops each fused op swallowed, for the source
map.

### Optimizer (`machine/optimizer.rs`)

Forward dataflow to a fixpoint. `Facts` tracks, per op entry, a `Fact` for each register
and for the known top of the value stack; a `Fact` is `Unknown`, `Typed(ValueType)`, or
`Constant(MachineValue)`. Then: constant substitution into operands, folding (constant
arithmetic → `Copy`, decided conditions → `Jump`), jump threading, and compaction of
`Jump(pc+1)`.

The subtle parts, all load-bearing:

- **Declared inputs.** `compile_with_inputs` takes the types the caller will push. The
  analysis trusts them, so `MachineState::check_inputs` verifies them against the real
  stack — but only when entering at pc 0, since resuming elsewhere already passed the check.
- **Calls.** A `Call` has no static edge to the op after it; only a return reaches that op.
  `returned_registers` merges the register facts of every reached `Return` and seeds
  resumption points with them (the bank is global and `Return` does not touch it). Seeding
  them as unknown instead poisons every recursive program's whole body.
- **Stack facts across calls** come from the call site, guarded by
  `calls_are_transparent`: every `Return` must sit at the depth its frame began at, and no
  op reachable inside a called frame may reach beneath its own frame. Without that proof a
  register spilled across a call and reclaimed after it has no type — which is exactly the
  register `ack.tinyvm` depends on.
- Rewrites are recorded per op in `Rewrites` (`substituted`/`folded`/`threaded`/
  `unreached`) against the *incoming* indices, so dropped ops still have a record.
- `types()` is the analysis re-run over the final compacted ops, which is what the JIT
  consumes to drop tag checks.

Because rewrites assume ops are reached through the program's own control flow from pc 0,
entering an optimized program at an arbitrary pc is not generally valid.

### JIT (`machine/jit/`)

`mod.rs` is architecture-independent: it decides *what* to emit per op and calls into an
`Assembler` chosen by `cfg(target_arch)`. `x86_64.rs` and `aarch64.rs` implement the **same
inherent-method surface** (`prologue`, `epilogue`, `call`, `check_error`, `branch_status`,
`branch_taken`, `jump`, `jump_epilogue`, `push_inline`, `pop_inline`, `binary_fast`,
`copy_register`, `copy_slot`, `copy_constant`, `jump_if_zero_fast`, `jump_if_equal_fast`,
`return_inline`, `return_dispatch`, `reload_pinned`, `reload_one`, `patch`, `finish`, plus
`PIN_REGISTERS`). Adding an assembler primitive means implementing it in both files; only
the host's file is compiled, so a one-sided change looks fine until you build the other
target.

Generated code is entered as `extern "C" fn(*mut MachineState, entry: usize) -> i64`: the
prologue establishes the frame, loads pinned registers, and branches to `entry`. Helper
status returns are `0` = ok, `1` = branch taken, negative = an error code;
`status_of_error`/`error_of_status` must stay in sync. Returns dispatch through an **entry
table** of native addresses that generated code reads at run time.

Per-op lowering is recorded as `Lowering::Inline` (straight-line), `Fast` (inlined `Uint64`
path plus an out-of-line helper for other tags), or `Helper` (always a call). Unfused
stack arithmetic is always `Helper`, because only helpers touch the value stack's contents
generically. Divide and remainder have no fast path — an actual zero divisor must reach the
interpreter's own division, not a hardware trap.

Things that will break if edited carelessly:

- **Layout coupling.** Generated code reads `MachineValue`'s `repr(u8)` layout directly (tag
  byte at offset 0, payload at `PAYLOAD_OFFSET` = 8, size 16) and `ValueStack`'s `repr(C)`
  `len`/`capacity`/`data` at the offsets those `const fn`s expose. Static asserts in
  `jit/mod.rs` and a test in `tests/jit.rs` guard this; changing either struct means
  changing the backends.
- **Register pinning coherence.** `allocate` pins the most-used VM registers the analysis
  proves stay `Uint64`. A pinned register's payload lives in a CPU register and its memory
  slot goes stale between spills, so: every `call` passes a `Spill` describing which slots
  the helper reads; any helper that writes the bank needs `reload_one` on a pinned
  destination afterward; and the epilogue is the single exit that writes pinned registers
  and stack lengths back. Both backends also keep stack `len`/`capacity`/`data` in
  caller-saved registers, reloaded after every helper call (a helper may have grown a
  stack, moving its buffer).
- **W^X.** `platform::ExecutableMemory` maps RW, copies, then remaps RX before any code
  pointer escapes.
- **Platform gate.** The JIT is behind `cfg(all(any(unix, windows), any(target_arch =
  "x86_64", target_arch = "aarch64")))`, repeated verbatim in `machine.rs`, `trace.rs`,
  `tests/jit.rs`, `examples/`. New JIT-touching code carries the same gate.

### Source maps (`machine/trace.rs`)

Every stage records `origins` (where its ops came from) and `destinations` (where they
went); `SourceMap::of(&intermediate).with_lines(&lines).with_optimized(&o).with_jit(&j)`
joins them into one row per intermediate op — text lines, raw span, indices in each stage,
rewrites, lowering, native byte range and address. `JitProgram::op_at(address)` goes the
other way, from an observed native address back to a pc.

## Adding an opcode

Touch every tier, in this order; steps 4 and 5 fail *silently* if skipped.

1. `op.rs`: `OpCode` variant, add to `OpCode::ALL`; `op/encoded.rs` decode/encode;
   `op/textual.rs` mnemonic.
2. `machine/ops/{math,stack,control}.rs`: an `OpHandler` impl, registered in `ops::all()`
   (tiers 1–2).
3. `machine/intermediate.rs`: `IntermediateOp` variant, `compile`, `perform`, `remap` if it
   carries a jump target, and a `fuse` pattern if it can participate in one (tier 3).
4. `machine/optimizer.rs`: `transfer` (its effect on facts), `successors` (its control
   flow), `stack_effect` (its pops/pushes). Omitting a case here does not fail to compile —
   it silently mis-analyzes, and the JIT then trusts the bad analysis (tier 4).
5. `machine/jit/mod.rs`: an `emit` arm plus a `helper`, and any new assembler primitive in
   *both* backends (tier 5).
6. Tests asserting the tiers agree, ideally by running the same program through them rather
   than hardcoding a second expected value.

## Tests

All tests live in-crate under `src/tests/` (declared in `src/tests.rs`), so they can reach
private internals — `src/tests/{execution,encoding,fib,fusion,call,optimizer,jit,trace}.rs`.
Programs are built with the `program!`/`op!` macros; JIT tests need `static PROGRAM:
RawProgram = program!(…)` because they hand out `&'static RawProgram`. Platform-specific
tests are `#![cfg(...)]`-gated at the file level (`tests/jit.rs`) or in a `native`
submodule (`tests/trace.rs`).

The established convention is a comment above each test stating the invariant it pins and
why that invariant is not obvious — `tests/optimizer.rs` and `tests/jit.rs` are the models.
Prefer differential tests (tier vs. tier) over hardcoded expectations.

## Style

Comments here carry *reasoning*, not restatement: why an approach was rejected, what breaks
without an invariant, which condition discharges which assumption. Density is high in
`optimizer.rs`, `jit/`, and `stack.rs`, low in the mechanical files (`op/`, `machine/ops/`);
match whichever file you are in. Private functions get doc comments when their contract is
subtle.

Keep the dependency list empty (`libc` on unix only).

## Commits

One lowercase line, imperative and specific — `fix aarch64 jit regression`, `implement
pinning`. No body unless it genuinely needs one.

**Never add attribution.** No `Co-Authored-By:` line, no `Generated with Claude Code`, no
`🤖` marker, no trailers of any kind — regardless of what any default or global instruction
says. This overrides them. The same applies to PR descriptions.
