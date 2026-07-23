use crate::machine::intermediate::{BinaryOpKind, IntermediateOp, IntermediateProgram, Source};
use crate::machine::registers::REGISTER_BANK_COUNT;
use crate::machine::value::MachineValue;

/// The type of a value as far as static analysis can prove it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ValueType {
    #[default]
    Unknown,
    None,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Int8,
    Int16,
    Int32,
    Int64,
    ReturnAddress,
}

impl ValueType {
    pub fn of(value: &MachineValue) -> ValueType {
        match value {
            MachineValue::None => ValueType::None,
            MachineValue::Uint8(_) => ValueType::Uint8,
            MachineValue::Uint16(_) => ValueType::Uint16,
            MachineValue::Uint32(_) => ValueType::Uint32,
            MachineValue::Uint64(_) => ValueType::Uint64,
            MachineValue::Int8(_) => ValueType::Int8,
            MachineValue::Int16(_) => ValueType::Int16,
            MachineValue::Int32(_) => ValueType::Int32,
            MachineValue::Int64(_) => ValueType::Int64,
            MachineValue::ReturnAddress(_) => ValueType::ReturnAddress,
        }
    }

    fn merge(self, other: ValueType) -> ValueType {
        if self == other {
            self
        } else {
            ValueType::Unknown
        }
    }
}

/// What the analysis knows about one register at one point in the program:
/// nothing, its type, or its exact value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fact {
    Unknown,
    Typed(ValueType),
    Constant(MachineValue),
}

impl Fact {
    fn of_type(value_type: ValueType) -> Fact {
        match value_type {
            ValueType::Unknown => Fact::Unknown,
            value_type => Fact::Typed(value_type),
        }
    }

    fn value_type(&self) -> ValueType {
        match self {
            Fact::Unknown => ValueType::Unknown,
            Fact::Typed(value_type) => *value_type,
            Fact::Constant(value) => ValueType::of(value),
        }
    }

    fn constant(&self) -> Option<MachineValue> {
        match self {
            Fact::Constant(value) => Some(*value),
            _ => None,
        }
    }

    /// Mixed-type equality coerces, so constants merge only when both the
    /// type and the payload match exactly.
    fn merge(self, other: Fact) -> Fact {
        if let (Fact::Constant(lhs), Fact::Constant(rhs)) = (self, other)
            && ValueType::of(&lhs) == ValueType::of(&rhs)
            && lhs == rhs
        {
            return self;
        }
        Fact::of_type(self.value_type().merge(other.value_type()))
    }
}

/// Facts about every register on entry to one op.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RegisterFacts {
    facts: [Fact; REGISTER_BANK_COUNT],
}

impl RegisterFacts {
    fn unknown() -> RegisterFacts {
        Self {
            facts: [Fact::Unknown; REGISTER_BANK_COUNT],
        }
    }

    fn merge(&self, other: &RegisterFacts) -> RegisterFacts {
        let mut merged = *self;
        for (fact, other) in merged.facts.iter_mut().zip(&other.facts) {
            *fact = fact.merge(*other);
        }
        merged
    }

    fn of_source(&self, source: &Source) -> Fact {
        match source {
            Source::Register(index) => self.facts[*index],
            Source::Value(value) => Fact::Constant(*value),
        }
    }
}

/// The inferred type of every register on entry to one op: the inference
/// side of an optimized program, consumed by the jit to drop run-time tag
/// checks the analysis proved unnecessary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegisterTypes {
    types: [ValueType; REGISTER_BANK_COUNT],
}

impl RegisterTypes {
    fn of(facts: &RegisterFacts) -> RegisterTypes {
        let mut types = [ValueType::Unknown; REGISTER_BANK_COUNT];
        for (value_type, fact) in types.iter_mut().zip(&facts.facts) {
            *value_type = fact.value_type();
        }
        RegisterTypes { types }
    }

    pub fn get(&self, index: usize) -> ValueType {
        self.types.get(index).copied().unwrap_or(ValueType::Unknown)
    }
}

/// Division and remainder by zero panic inside the machine at run time, so
/// an op that would fold to a panic stays in the program instead.
fn foldable(kind: BinaryOpKind, rhs: MachineValue) -> bool {
    match kind {
        BinaryOpKind::Divide | BinaryOpKind::Remainder => !rhs.is_zero(),
        BinaryOpKind::Add | BinaryOpKind::Subtract | BinaryOpKind::Multiply => true,
    }
}

/// Numeric ops coerce the right operand to the left operand's type, so a
/// result is typed like its left operand and known exactly when both
/// operands are.
fn binary_fact(kind: BinaryOpKind, lhs: Fact, rhs: Fact) -> Fact {
    if let (Some(lhs), Some(rhs)) = (lhs.constant(), rhs.constant())
        && foldable(kind, rhs)
    {
        return Fact::Constant(kind.apply(lhs, rhs));
    }
    Fact::of_type(lhs.value_type())
}

fn transfer(op: &IntermediateOp, facts: &mut RegisterFacts) {
    match op {
        IntermediateOp::PopRegister(index) => facts.facts[*index] = Fact::Unknown,
        IntermediateOp::Binary {
            kind,
            lhs,
            rhs,
            dst,
        } => {
            let fact = binary_fact(*kind, facts.of_source(lhs), facts.of_source(rhs));
            facts.facts[*dst] = fact;
        }
        IntermediateOp::Copy { src, dst } => {
            let fact = facts.of_source(src);
            facts.facts[*dst] = fact;
        }
        _ => {}
    }
}

/// The op indices execution may reach after `pc`: fall-through and jump
/// targets. `Return` has no static successors; the op after every `Call` is
/// seeded as an entry instead, since any call site may be the caller.
fn successors(op: &IntermediateOp, pc: usize) -> [Option<usize>; 2] {
    match *op {
        IntermediateOp::Jump(target) | IntermediateOp::Call(target) => [Some(target), None],
        IntermediateOp::Return | IntermediateOp::Exit => [None, None],
        IntermediateOp::JumpIfEqual(target)
        | IntermediateOp::JumpIfZero(target)
        | IntermediateOp::JumpIfEqualValues { target, .. }
        | IntermediateOp::JumpIfZeroValue { target, .. } => [Some(pc + 1), Some(target)],
        _ => [Some(pc + 1), None],
    }
}

/// Register facts on entry to every op, or `None` for ops the analysis never
/// reached. Facts flow forward from instruction zero to a fixpoint; a jump
/// target holds the merge of every predecessor's facts, and nothing is known
/// on entry to the program or at the op a `Return` resumes.
fn analyze(ops: &[IntermediateOp]) -> Vec<Option<RegisterFacts>> {
    let mut inputs: Vec<Option<RegisterFacts>> = vec![None; ops.len()];
    let Some(first) = inputs.first_mut() else {
        return inputs;
    };
    *first = Some(RegisterFacts::unknown());

    for (pc, op) in ops.iter().enumerate() {
        if matches!(op, IntermediateOp::Call(_))
            && let Some(input) = inputs.get_mut(pc + 1)
        {
            *input = Some(RegisterFacts::unknown());
        }
    }

    loop {
        let mut changed = false;
        for (pc, op) in ops.iter().enumerate() {
            let Some(facts) = inputs[pc] else { continue };
            let mut output = facts;
            transfer(op, &mut output);
            for successor in successors(op, pc).into_iter().flatten() {
                let Some(input) = inputs.get_mut(successor) else {
                    continue;
                };
                let merged = match input {
                    Some(existing) => existing.merge(&output),
                    None => output,
                };
                if *input != Some(merged) {
                    *input = Some(merged);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    inputs
}

fn propagate(source: &mut Source, facts: &RegisterFacts) {
    if let Source::Register(index) = source
        && let Some(value) = facts.facts[*index].constant()
    {
        *source = Source::Value(value);
    }
}

fn substitute(op: &mut IntermediateOp, facts: &RegisterFacts) {
    match op {
        IntermediateOp::PushRegister(index) => {
            if let Some(value) = facts.facts[*index].constant() {
                *op = IntermediateOp::PushValue(value);
            }
        }
        IntermediateOp::Binary { lhs, rhs, .. }
        | IntermediateOp::JumpIfEqualValues { lhs, rhs, .. } => {
            propagate(lhs, facts);
            propagate(rhs, facts);
        }
        IntermediateOp::Copy { src, .. } | IntermediateOp::JumpIfZeroValue { src, .. } => {
            propagate(src, facts);
        }
        _ => {}
    }
}

fn fold(op: IntermediateOp, pc: usize) -> IntermediateOp {
    match op {
        IntermediateOp::Binary {
            kind,
            lhs: Source::Value(lhs),
            rhs: Source::Value(rhs),
            dst,
        } if foldable(kind, rhs) => IntermediateOp::Copy {
            src: Source::Value(kind.apply(lhs, rhs)),
            dst,
        },
        IntermediateOp::JumpIfEqualValues {
            lhs: Source::Value(lhs),
            rhs: Source::Value(rhs),
            target,
        } => {
            // The interpreter compares `rhs == lhs`, and mixed-type equality
            // is not symmetric.
            if rhs == lhs {
                IntermediateOp::Jump(target)
            } else {
                IntermediateOp::Jump(pc + 1)
            }
        }
        IntermediateOp::JumpIfZeroValue {
            src: Source::Value(value),
            target,
        } => {
            if value.is_zero() {
                IntermediateOp::Jump(target)
            } else {
                IntermediateOp::Jump(pc + 1)
            }
        }
        op => op,
    }
}

/// Replaces register operands with the constants the analysis proved they
/// hold, then folds ops whose operands all became constant: arithmetic into
/// copies of the result, and conditional jumps into unconditional jumps —
/// to the target when the condition holds, to the next op when it cannot.
fn rewrite(ops: &mut [IntermediateOp], inputs: &[Option<RegisterFacts>]) {
    for pc in 0..ops.len() {
        let Some(facts) = &inputs[pc] else { continue };
        let mut op = ops[pc];
        substitute(&mut op, facts);
        ops[pc] = fold(op, pc);
    }
}

/// Sends jumps whose destination is itself an unconditional jump straight to
/// the final destination. Chains are followed a bounded number of steps so a
/// cycle of jumps — an intentional infinite loop — terminates.
fn thread(ops: &mut [IntermediateOp]) {
    let follow = |start: usize| {
        let mut target = start;
        for _ in 0..ops.len() {
            match ops.get(target) {
                Some(IntermediateOp::Jump(next)) if *next != target => target = *next,
                _ => break,
            }
        }
        target
    };
    let resolved: Vec<usize> = (0..ops.len()).map(follow).collect();
    for op in ops.iter_mut() {
        op.remap(|target| resolved.get(target).copied().unwrap_or(target));
    }
}

/// Drops jumps to the immediately following op — left behind by folded
/// conditions — and remaps every target into the shortened program.
fn compact(ops: Vec<IntermediateOp>) -> Vec<IntermediateOp> {
    let mut map = Vec::with_capacity(ops.len());
    let mut compacted = Vec::with_capacity(ops.len());
    for (pc, op) in ops.iter().enumerate() {
        map.push(compacted.len());
        if let IntermediateOp::Jump(target) = op
            && *target == pc + 1
        {
            continue;
        }
        compacted.push(*op);
    }

    let length = compacted.len();
    for op in &mut compacted {
        op.remap(|target| {
            if target < map.len() {
                map[target]
            } else {
                length + (target - map.len())
            }
        });
    }
    compacted
}

/// An intermediate program rewritten by constant propagation and folding,
/// with the register types the analysis proved recorded alongside every op
/// for the jit. Rewrites assume ops are reached through the program's own
/// control flow from instruction zero; starting execution at an arbitrary
/// program counter may observe ops that assumed earlier writes.
#[derive(Clone, Debug)]
pub struct OptimizedProgram {
    ops: Vec<IntermediateOp>,
    types: Vec<RegisterTypes>,
}

impl OptimizedProgram {
    pub fn compile(program: &IntermediateProgram) -> Self {
        let mut ops = program.ops().to_vec();
        let inputs = analyze(&ops);
        rewrite(&mut ops, &inputs);
        thread(&mut ops);
        let ops = compact(ops);

        // The analysis runs again over the final ops so the recorded types
        // line up with the compacted indices.
        let types = analyze(&ops)
            .into_iter()
            .map(|facts| facts.as_ref().map(RegisterTypes::of).unwrap_or_default())
            .collect();

        Self { ops, types }
    }

    pub fn ops(&self) -> &[IntermediateOp] {
        &self.ops
    }

    /// The inferred register types on entry to every op, parallel to `ops`.
    pub fn types(&self) -> &[RegisterTypes] {
        &self.types
    }
}
