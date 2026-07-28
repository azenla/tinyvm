//! Correlating the compilation stages with each other.
//!
//! A program passes through four representations on its way to native code:
//! the text it was written in, the raw ops it parses to, the intermediate ops
//! fusion collapses those into, the optimized ops constant propagation
//! rewrites, and the machine code the jit emits. Each stage records where its
//! ops came from and what became of them, and [`SourceMap`] joins those
//! records into one table: forward from a line of text to the bytes it
//! became, and backward from a program counter — or, with
//! [`JitProgram::op_at`](crate::machine::jit::JitProgram::op_at), a native
//! address — to the op that produced it.
//!
//! The uncompiled and inlined interpreters execute the raw ops themselves, so
//! their program counters are raw op indices and need no map; the stages below
//! begin where fusion first makes one op stand for several.

use crate::machine::intermediate::IntermediateProgram;
use crate::machine::optimizer::OptimizedProgram;
use std::fmt::Display;
use std::ops::Range;

#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use crate::machine::jit::JitProgram;

/// A half-open range: ops within one stage's program, bytes within the jit's
/// generated code, or lines within the program text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    /// The span covering one index alone.
    pub const fn single(index: usize) -> Span {
        Span {
            start: index,
            end: index + 1,
        }
    }

    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn contains(&self, index: usize) -> bool {
        self.start <= index && index < self.end
    }

    pub const fn indices(&self) -> Range<usize> {
        self.start..self.end
    }
}

impl Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.len() == 1 {
            write!(f, "{}", self.start)
        } else {
            write!(f, "{}..{}", self.start, self.end)
        }
    }
}

/// How the jit lowered one op, as recorded while the code was emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lowering {
    /// Straight-line machine code, no call.
    Inline,
    /// An inlined `Uint64` fast path, with an out-of-line helper for anything
    /// else.
    Fast,
    /// Always an out-of-line helper call.
    Helper,
}

impl Lowering {
    pub const ALL: &'static [Lowering] = &[Lowering::Inline, Lowering::Fast, Lowering::Helper];

    pub const fn label(&self) -> &'static str {
        match self {
            Lowering::Inline => "inline",
            Lowering::Fast => "fast",
            Lowering::Helper => "helper",
        }
    }
}

impl Display for Lowering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// What the optimizer did to one op: which of its rewrites changed the op, and
/// whether the analysis reached it at all. An op the analysis never reached
/// keeps whatever fusion produced, since there are no facts to rewrite it
/// with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rewrites {
    /// A register operand was replaced by the constant it was proven to hold.
    pub substituted: bool,
    /// The op itself was replaced: arithmetic on constants by a copy of the
    /// result, a condition on constants by an unconditional jump.
    pub folded: bool,
    /// A jump target was forwarded past another jump.
    pub threaded: bool,
    /// The analysis never reached the op.
    pub unreached: bool,
}

impl Rewrites {
    /// Whether any rewrite changed the op.
    pub const fn any(&self) -> bool {
        self.substituted || self.folded || self.threaded
    }
}

impl Display for Rewrites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let labels = [
            (self.substituted, "substituted"),
            (self.folded, "folded"),
            (self.threaded, "threaded"),
            (self.unreached, "unreached"),
        ];
        let mut written = false;
        for (set, label) in labels {
            if !set {
                continue;
            }
            if written {
                write!(f, "+")?;
            }
            write!(f, "{label}")?;
            written = true;
        }
        if !written {
            write!(f, "-")?;
        }
        Ok(())
    }
}

/// One op followed through the pipeline: the raw ops fused into it, the index
/// it holds in each later stage, and the machine code it became. Rows are
/// keyed by intermediate op, since that is the first stage where one op may
/// stand for several of the ops written down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    /// The 1-based text lines the raw ops were written on, from the first op's
    /// line through the last op's. Only set when the map was joined with the
    /// lines the parser recorded; blank and comment lines in between are
    /// covered by the span without belonging to any op.
    pub lines: Option<Span>,
    /// The raw ops fused into this op.
    pub raw: Span,
    /// The index in the intermediate program.
    pub intermediate: usize,
    /// The index in the optimized program, or `None` when the optimizer
    /// dropped the op.
    pub optimized: Option<usize>,
    /// What the optimizer did to the op.
    pub rewrites: Rewrites,
    /// How the jit lowered the op.
    pub lowering: Option<Lowering>,
    /// The op's byte range within the jit's generated code.
    pub code: Option<Span>,
    /// The address generated code branches to in order to run the op.
    pub address: Option<usize>,
}

impl Mapping {
    fn of(intermediate: usize, raw: Span) -> Mapping {
        Mapping {
            lines: None,
            raw,
            intermediate,
            optimized: None,
            rewrites: Rewrites::default(),
            lowering: None,
            code: None,
            address: None,
        }
    }
}

/// The stages of one compilation joined together. Built up a stage at a time,
/// each stage joined against the one before it:
///
/// ```no_run
/// # use tinyvm::machine::intermediate::IntermediateProgram;
/// # use tinyvm::machine::optimizer::OptimizedProgram;
/// # use tinyvm::machine::trace::SourceMap;
/// # use tinyvm::program::RawProgram;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let text = std::fs::read_to_string("programs/fib.tinyvm")?;
/// let (raw, lines) = RawProgram::parse_with_lines(&text)?;
/// let intermediate = IntermediateProgram::compile(&raw)?;
/// let optimized = OptimizedProgram::compile(&intermediate);
///
/// let map = SourceMap::of(&intermediate)
///     .with_lines(&lines)
///     .with_optimized(&optimized);
///
/// // Where the third op written down ended up.
/// let mapping = map.of_raw(2).unwrap();
/// println!("{} -> {:?}", mapping.raw, mapping.optimized);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    /// One row per intermediate op, in intermediate order.
    mappings: Vec<Mapping>,
    /// The row each raw op belongs to.
    raw: Vec<usize>,
    /// The row each optimized op belongs to, empty until the optimized stage
    /// is joined.
    optimized: Vec<usize>,
    /// Whether the optimized stage was joined, and so which stage a jit
    /// program's ops are indexed by.
    optimizer_joined: bool,
}

impl SourceMap {
    /// Starts a map from the fusion stage: what every intermediate op was
    /// fused from, and nothing yet about what became of it.
    pub fn of(program: &IntermediateProgram) -> SourceMap {
        SourceMap {
            mappings: program
                .origins()
                .iter()
                .enumerate()
                .map(|(index, raw)| Mapping::of(index, *raw))
                .collect(),
            raw: program.destinations().to_vec(),
            optimized: Vec::new(),
            optimizer_joined: false,
        }
    }

    /// Joins the 1-based text lines the parser recorded, one per raw op.
    pub fn with_lines(mut self, lines: &[usize]) -> SourceMap {
        for mapping in &mut self.mappings {
            let first = lines.get(mapping.raw.start).copied();
            let last = mapping
                .raw
                .end
                .checked_sub(1)
                .and_then(|index| lines.get(index))
                .copied();
            mapping.lines = match (first, last) {
                (Some(first), Some(last)) => Some(Span::new(first, last + 1)),
                _ => None,
            };
        }
        self
    }

    /// Joins the optimizer's rewrites. The program must have been compiled
    /// from the intermediate program this map was started from.
    pub fn with_optimized(mut self, program: &OptimizedProgram) -> SourceMap {
        self.optimized = program.origins().to_vec();
        for (index, mapping) in self.mappings.iter_mut().enumerate() {
            mapping.optimized = program.destinations().get(index).copied().flatten();
            mapping.rewrites = program.rewrites().get(index).copied().unwrap_or_default();
        }
        self.optimizer_joined = true;
        self
    }

    /// Joins the jit's lowering decisions and generated code. The program must
    /// have been compiled from the last program joined into this map: the
    /// optimized one if there is one, the intermediate one otherwise.
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    pub fn with_jit(mut self, program: &JitProgram) -> SourceMap {
        for pc in 0..program.spans().len() {
            let row = if self.optimizer_joined {
                self.optimized.get(pc).copied()
            } else {
                Some(pc)
            };
            let Some(mapping) = row.and_then(|row| self.mappings.get_mut(row)) else {
                continue;
            };
            mapping.lowering = program.lowerings().get(pc).copied();
            mapping.code = program.spans().get(pc).copied();
            mapping.address = program.address(pc);
        }
        self
    }

    /// Every row, in intermediate order.
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }

    /// The row the given raw op became part of.
    pub fn of_raw(&self, index: usize) -> Option<&Mapping> {
        let row = self.raw.get(index).copied()?;
        self.mappings.get(row)
    }

    pub fn of_intermediate(&self, index: usize) -> Option<&Mapping> {
        self.mappings.get(index)
    }

    /// The row the given optimized op came from, once the optimized stage has
    /// been joined.
    pub fn of_optimized(&self, index: usize) -> Option<&Mapping> {
        let row = self.optimized.get(index).copied()?;
        self.mappings.get(row)
    }
}
