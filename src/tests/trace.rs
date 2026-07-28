use crate::machine::intermediate::IntermediateProgram;
use crate::machine::optimizer::OptimizedProgram;
use crate::machine::trace::{SourceMap, Span};
use crate::machine::value::MachineValue;
use crate::machine::{Machine, MachineProgram, ops};
use crate::op;
use crate::op::OpArg::{Instruction, Register1, Register2, Register3, Uint64};
use crate::op::OpCode::{Add, Exit, Jump, JumpIfZero, Pop, Push, Subtract};
use crate::program;
use crate::program::RawProgram;

/// A loop whose counter and accumulator are carried around the back edge, so
/// the arithmetic cannot be folded away and the analysis still proves both
/// registers hold `Uint64` values.
static LOOP: RawProgram = program!(
    op!(Push, Uint64(5)),
    op!(Pop, Register1),
    op!(Push, Uint64(0)),
    op!(Pop, Register2),
    op!(Push, Register2),
    op!(Push, Uint64(1)),
    op!(Add),
    op!(Pop, Register2),
    op!(Push, Register1),
    op!(Push, Uint64(1)),
    op!(Subtract),
    op!(Pop, Register1),
    op!(Push, Register1),
    op!(JumpIfZero, Instruction(15)),
    op!(Jump, Instruction(4)),
    op!(Push, Register2),
    op!(Exit),
);

// The spans fusion records have to account for every raw op exactly once: they
// tile the raw program in order, and every raw op's destination is the op whose
// span covers it.
#[test]
fn fusion_spans_tile_the_raw_program() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Register1),
        op!(Push, Register2),
        op!(Add),
        op!(Pop, Register3),
        op!(Push, Register3),
        op!(Exit),
    );
    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();

    // The four ops of the arithmetic became one superinstruction, and the two
    // after it stayed themselves.
    assert_eq!(intermediate.origins().len(), 3);
    assert_eq!(intermediate.origins()[0], Span::new(0, 4));
    assert_eq!(intermediate.origins()[1], Span::single(4));
    assert_eq!(intermediate.origins()[2], Span::single(5));

    let mut next = 0;
    for span in intermediate.origins() {
        assert_eq!(span.start, next);
        next = span.end;
    }
    assert_eq!(next, PROGRAM.ops().len());

    for (raw, destination) in intermediate.destinations().iter().enumerate() {
        assert!(intermediate.origins()[*destination].contains(raw));
    }
}

// The optimizer's record has to outlive the ops it removes: the condition it
// folded is marked folded even though compaction then dropped the fall-through
// jump it left behind, and everything that survived still points at the op it
// came from.
#[test]
fn optimizer_records_folds_and_drops() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(1)),
        op!(JumpIfZero, Instruction(4)),
        op!(Push, Uint64(111)),
        op!(Exit),
        op!(Push, Uint64(222)),
        op!(Exit),
    );
    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let optimized = OptimizedProgram::compile(&intermediate);

    assert!(optimized.rewrites()[0].folded);
    assert_eq!(optimized.destinations()[0], None);
    assert_eq!(optimized.origins(), [1, 2, 3, 4]);
    for (index, destination) in optimized.destinations().iter().enumerate().skip(1) {
        assert_eq!(*destination, Some(index - 1));
    }

    // Reading the same thing through the map: the two raw ops of the folded
    // condition led nowhere, and the ops after them survived.
    let map = SourceMap::of(&intermediate).with_optimized(&optimized);
    assert_eq!(map.of_raw(0).unwrap().optimized, None);
    assert_eq!(map.of_raw(1).unwrap().optimized, None);
    assert!(map.of_raw(1).unwrap().rewrites.folded);
    assert_eq!(map.of_raw(2).unwrap().optimized, Some(0));
    assert_eq!(map.of_optimized(0).unwrap().raw, Span::single(2));
}

// Substituting a proven constant and folding the result that becomes possible
// are recorded separately, since they are separate reasons an op no longer
// looks like what was written.
#[test]
fn optimizer_records_substitutions() {
    static PROGRAM: RawProgram = program!(
        op!(Push, Uint64(7)),
        op!(Pop, Register1),
        op!(Push, Register1),
        op!(Push, Uint64(3)),
        op!(Add),
        op!(Pop, Register2),
        op!(Push, Register2),
        op!(Exit),
    );
    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let optimized = OptimizedProgram::compile(&intermediate);

    // `push r1; push 3u64; add; pop r2` fused into one op, whose left operand
    // was replaced by the 7 the analysis proved, which then folded to a copy.
    let arithmetic = optimized.rewrites()[1];
    assert!(arithmetic.substituted);
    assert!(arithmetic.folded);

    // `push r2` only ever had a register to substitute.
    let push = optimized.rewrites()[2];
    assert!(push.substituted);
    assert!(!push.folded);

    let map = SourceMap::of(&intermediate).with_optimized(&optimized);
    assert_eq!(map.of_raw(4).unwrap().raw, Span::new(2, 6));
    assert!(map.of_raw(4).unwrap().rewrites.any());
}

// A jump sent past another jump is recorded against the jump that moved, not
// the one it jumped over, and an op no predecessor reaches is marked as never
// analyzed rather than left looking untouched.
#[test]
fn optimizer_records_threading_and_unreached_ops() {
    static PROGRAM: RawProgram = program!(
        op!(Jump, Instruction(2)),
        op!(Exit),
        op!(Jump, Instruction(4)),
        op!(Exit),
        op!(Push, Uint64(7)),
        op!(Exit),
    );
    let intermediate = IntermediateProgram::compile(&PROGRAM).unwrap();
    let optimized = OptimizedProgram::compile(&intermediate);

    assert!(optimized.rewrites()[0].threaded);
    assert!(!optimized.rewrites()[2].threaded);
    assert!(optimized.rewrites()[1].unreached);
    assert!(!optimized.rewrites()[0].unreached);

    // The first jump now lands on the push directly, skipping the jump it used
    // to go through.
    let mut machine = Machine::new(ops::all());
    machine.run(&MachineProgram::Optimized(optimized)).unwrap();
    assert_eq!(machine.state().pop().unwrap(), MachineValue::Uint64(7));
}

// Comments and blank lines make op indices and text lines drift apart, so the
// line every op was written on is recorded as it is parsed and carried into the
// map.
#[test]
fn source_lines_skip_comments_and_blanks() {
    static TEXT: &str = "# store a value\npush 1u64\n\n# and read it back\npop r1\npush r1\nexit\n";

    let (program, lines) = RawProgram::parse_with_lines(TEXT).unwrap();
    assert_eq!(program.ops().len(), 4);
    assert_eq!(lines, [2, 5, 6, 7]);

    let intermediate = IntermediateProgram::compile(&program).unwrap();
    let map = SourceMap::of(&intermediate).with_lines(&lines);

    // The push and the pop fused into a copy, which spans both their lines.
    assert_eq!(map.of_raw(0).unwrap().raw, Span::new(0, 2));
    assert_eq!(map.of_raw(0).unwrap().lines, Some(Span::new(2, 6)));
    assert_eq!(map.of_raw(2).unwrap().lines, Some(Span::single(6)));
    assert_eq!(map.of_raw(3).unwrap().lines, Some(Span::single(7)));
    assert!(map.of_raw(4).is_none());
}

// Every raw op belongs to exactly one row, whatever fusion and the optimizer
// did to it.
#[test]
fn every_raw_op_has_a_row() {
    let intermediate = IntermediateProgram::compile(&LOOP).unwrap();
    let optimized = OptimizedProgram::compile(&intermediate);
    let map = SourceMap::of(&intermediate).with_optimized(&optimized);

    assert_eq!(map.mappings().len(), intermediate.ops().len());
    for raw in 0..LOOP.ops().len() {
        let mapping = map.of_raw(raw).unwrap();
        assert!(mapping.raw.contains(raw));
        assert_eq!(mapping.intermediate, intermediate.destinations()[raw]);
    }
    for (pc, op) in optimized.ops().iter().enumerate() {
        let mapping = map.of_optimized(pc).unwrap();
        assert_eq!(mapping.optimized, Some(pc));
        assert_eq!(*op, optimized.ops()[pc]);
    }
}

#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native {
    use super::LOOP;
    use crate::machine::intermediate::{BinaryOpKind, IntermediateOp, IntermediateProgram};
    use crate::machine::jit::JitProgram;
    use crate::machine::optimizer::{OptimizedProgram, ValueType};
    use crate::machine::trace::{Lowering, SourceMap};
    use crate::op;
    use crate::op::OpArg::{Register1, Uint64};
    use crate::op::OpCode::{Divide, Exit, Pop, Push};
    use crate::program;
    use crate::program::RawProgram;

    fn compile(program: &RawProgram, inputs: &[ValueType]) -> (OptimizedProgram, JitProgram) {
        let intermediate = IntermediateProgram::compile(program).unwrap();
        let optimized = OptimizedProgram::compile_with_inputs(&intermediate, inputs);
        let jit = JitProgram::compile_optimized(&optimized).unwrap();
        (optimized, jit)
    }

    fn lowering_of(
        optimized: &OptimizedProgram,
        jit: &JitProgram,
        matches: impl Fn(&IntermediateOp) -> bool,
    ) -> Lowering {
        let pc = optimized
            .ops()
            .iter()
            .position(matches)
            .expect("the program contains the op");
        jit.lowerings()[pc]
    }

    // Every byte of generated code belongs to exactly one op, the prologue, or
    // the epilogue: the op spans run in order from the end of the prologue to
    // the start of the epilogue without a gap.
    #[test]
    fn code_spans_tile_the_generated_code() {
        let (optimized, jit) = compile(&LOOP, &[]);

        assert_eq!(jit.spans().len(), optimized.ops().len());
        assert_eq!(jit.lowerings().len(), optimized.ops().len());
        assert_eq!(jit.prologue().start, 0);

        let mut next = jit.prologue().end;
        for span in jit.spans() {
            assert_eq!(span.start, next);
            next = span.end;
        }
        assert_eq!(next, jit.epilogue().start);
        assert_eq!(jit.epilogue().end, jit.code().len());
    }

    // An address observed at run time has to read back as the op it belongs to,
    // and an address in the prologue, the epilogue, or another allocation
    // entirely as no op at all.
    #[test]
    fn addresses_map_back_to_their_ops() {
        let (optimized, jit) = compile(&LOOP, &[]);
        let base = jit.code().as_ptr() as usize;

        for pc in 0..optimized.ops().len() {
            let address = jit.address(pc).unwrap();
            assert_eq!(address, base + jit.spans()[pc].start);
            if jit.spans()[pc].is_empty() {
                continue;
            }
            assert_eq!(jit.op_at(address), Some(pc));
            assert_eq!(jit.op_at(address + jit.spans()[pc].len() - 1), Some(pc));
        }

        assert_eq!(jit.op_at(base), None);
        assert_eq!(jit.op_at(base + jit.epilogue().start), None);
        assert_eq!(jit.op_at(base - 1), None);
        assert_eq!(jit.op_at(base + jit.code().len()), None);
        assert_eq!(jit.address(optimized.ops().len()), None);
    }

    // The recorded lowering is the decision the backend actually made, which
    // depends on what the analysis proved: arithmetic on registers proven
    // `Uint64` needs no tag check and becomes straight-line code, while a
    // divisor that may be zero has to reach the interpreter's own division.
    #[test]
    fn lowerings_record_what_the_backend_inlined() {
        let (optimized, jit) = compile(&LOOP, &[]);
        assert_eq!(
            lowering_of(&optimized, &jit, |op| matches!(
                op,
                IntermediateOp::Binary { .. }
            )),
            Lowering::Inline
        );
        assert_eq!(
            lowering_of(&optimized, &jit, |op| matches!(
                op,
                IntermediateOp::PushRegister(_)
            )),
            Lowering::Fast
        );
        assert_eq!(
            lowering_of(&optimized, &jit, |op| matches!(op, IntermediateOp::Exit)),
            Lowering::Helper
        );

        static DIVIDE: RawProgram = program!(
            op!(Push, Register1),
            op!(Push, Uint64(0)),
            op!(Divide),
            op!(Pop, Register1),
            op!(Exit),
        );
        let (optimized, jit) = compile(&DIVIDE, &[]);
        assert_eq!(
            lowering_of(&optimized, &jit, |op| matches!(
                op,
                IntermediateOp::Binary {
                    kind: BinaryOpKind::Divide,
                    ..
                }
            )),
            Lowering::Helper
        );
    }

    // The whole chain: every op of the optimized program carries the jit's
    // record of it, and every raw op reaches that record through the stages in
    // between.
    #[test]
    fn the_map_follows_a_raw_op_to_its_code() {
        let text = std::fs::read_to_string("programs/fib.tinyvm").unwrap();
        let (raw, lines) = RawProgram::parse_with_lines(&text).unwrap();
        let intermediate = IntermediateProgram::compile(&raw).unwrap();
        let optimized = OptimizedProgram::compile_with_inputs(&intermediate, &[ValueType::Uint64]);
        let jit = JitProgram::compile_optimized(&optimized).unwrap();

        let map = SourceMap::of(&intermediate)
            .with_lines(&lines)
            .with_optimized(&optimized)
            .with_jit(&jit);

        for (pc, span) in jit.spans().iter().enumerate() {
            let mapping = map.of_optimized(pc).unwrap();
            assert_eq!(mapping.optimized, Some(pc));
            assert_eq!(mapping.code, Some(*span));
            assert_eq!(mapping.address, jit.address(pc));
            assert_eq!(mapping.lowering, Some(jit.lowerings()[pc]));
        }

        for index in 0..raw.ops().len() {
            let mapping = map.of_raw(index).unwrap();
            assert!(mapping.raw.contains(index));
            assert!(mapping.lines.is_some());
            // Nothing in fib is dropped, so every raw op reaches real code.
            let code = mapping.code.unwrap();
            assert_eq!(jit.op_at(mapping.address.unwrap()), mapping.optimized);
            assert!(code.end <= jit.code().len());
        }
    }

    // Without an optimizer stage in between, a jit program's ops are the
    // intermediate ops, and the map joins them on those indices.
    #[test]
    fn the_map_joins_a_jit_without_an_optimizer() {
        let intermediate = IntermediateProgram::compile(&LOOP).unwrap();
        let jit = JitProgram::compile(&intermediate).unwrap();
        let map = SourceMap::of(&intermediate).with_jit(&jit);

        assert_eq!(jit.spans().len(), intermediate.ops().len());
        for (pc, span) in jit.spans().iter().enumerate() {
            let mapping = map.of_intermediate(pc).unwrap();
            assert_eq!(mapping.code, Some(*span));
            assert_eq!(mapping.optimized, None);
        }
    }
}
