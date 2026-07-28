use crate::machine::intermediate::{BinaryOpKind, IntermediateOp, IntermediateProgram, Source};
use crate::machine::registers::REGISTER_BANK_COUNT;
use crate::machine::trace::Rewrites;
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

/// What the analysis knows about one value at one point in the program:
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

/// Facts about the machine on entry to one op: every register, and the top
/// of the value stack from `stack[0]` (deepest known) to the last element
/// (top). Anything beneath the known entries is unknown, since callers may
/// enter with more values than a program's declared inputs.
#[derive(Clone, Debug, PartialEq)]
struct Facts {
    registers: [Fact; REGISTER_BANK_COUNT],
    stack: Vec<Fact>,
}

impl Facts {
    fn entry(inputs: &[ValueType]) -> Facts {
        Facts {
            registers: [Fact::Unknown; REGISTER_BANK_COUNT],
            stack: inputs.iter().map(|input| Fact::of_type(*input)).collect(),
        }
    }

    fn unknown() -> Facts {
        Self::entry(&[])
    }

    fn merge(&self, other: &Facts) -> Facts {
        let mut registers = self.registers;
        for (fact, other) in registers.iter_mut().zip(&other.registers) {
            *fact = fact.merge(*other);
        }

        // Stacks align at the top; whatever only one side knows about the
        // deeper entries is discarded.
        let depth = self.stack.len().min(other.stack.len());
        let lhs = &self.stack[self.stack.len() - depth..];
        let rhs = &other.stack[other.stack.len() - depth..];
        let stack = lhs
            .iter()
            .zip(rhs)
            .map(|(fact, other)| fact.merge(*other))
            .collect();

        Facts { registers, stack }
    }

    fn of_source(&self, source: &Source) -> Fact {
        match source {
            Source::Register(index) => self.registers[*index],
            Source::Value(value) => Fact::Constant(*value),
        }
    }

    fn push(&mut self, fact: Fact) {
        self.stack.push(fact);
    }

    fn pop(&mut self) -> Fact {
        self.stack.pop().unwrap_or(Fact::Unknown)
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
    fn of(facts: &Facts) -> RegisterTypes {
        let mut types = [ValueType::Unknown; REGISTER_BANK_COUNT];
        for (value_type, fact) in types.iter_mut().zip(&facts.registers) {
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

/// Count ops produce a `Uint32` for any numeric input and `None` for `None`.
fn count_fact(input: Fact) -> Fact {
    match input.value_type() {
        ValueType::Unknown => Fact::Unknown,
        ValueType::None => Fact::Typed(ValueType::None),
        _ => Fact::Typed(ValueType::Uint32),
    }
}

fn stack_kind(op: &IntermediateOp) -> Option<BinaryOpKind> {
    Some(match op {
        IntermediateOp::Add => BinaryOpKind::Add,
        IntermediateOp::Subtract => BinaryOpKind::Subtract,
        IntermediateOp::Multiply => BinaryOpKind::Multiply,
        IntermediateOp::Divide => BinaryOpKind::Divide,
        IntermediateOp::Remainder => BinaryOpKind::Remainder,
        _ => return None,
    })
}

fn transfer(op: &IntermediateOp, facts: &mut Facts) {
    if let Some(kind) = stack_kind(op) {
        // The interpreter pops the right operand first and coerces it to the
        // second pop's type.
        let rhs = facts.pop();
        let lhs = facts.pop();
        facts.push(binary_fact(kind, lhs, rhs));
        return;
    }

    match op {
        IntermediateOp::PushValue(value) => facts.push(Fact::Constant(*value)),
        IntermediateOp::PushRegister(index) => {
            let fact = facts.registers[*index];
            facts.push(fact);
        }
        IntermediateOp::PopRegister(index) => {
            let fact = facts.pop();
            facts.registers[*index] = fact;
        }
        IntermediateOp::CountLeadingZeros
        | IntermediateOp::CountLeadingOnes
        | IntermediateOp::CountTrailingZeros
        | IntermediateOp::CountTrailingOnes => {
            let fact = count_fact(facts.pop());
            facts.push(fact);
        }
        IntermediateOp::JumpIfEqual(_) => {
            facts.pop();
            facts.pop();
        }
        IntermediateOp::JumpIfZero(_) => {
            facts.pop();
        }
        IntermediateOp::Binary {
            kind,
            lhs,
            rhs,
            dst,
        } => {
            let fact = binary_fact(*kind, facts.of_source(lhs), facts.of_source(rhs));
            facts.registers[*dst] = fact;
        }
        IntermediateOp::Copy { src, dst } => {
            let fact = facts.of_source(src);
            facts.registers[*dst] = fact;
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

/// Facts on entry to every op, or `None` for ops the analysis never reached.
/// Facts flow forward from instruction zero to a fixpoint; a jump target
/// holds the merge of every predecessor's facts. On entry to the program the
/// stack holds the declared input types and every register is unknown, and
/// nothing at all is known at the op a `Return` resumes.
fn analyze(ops: &[IntermediateOp], inputs: &[ValueType]) -> Vec<Option<Facts>> {
    let mut entries: Vec<Option<Facts>> = vec![None; ops.len()];
    let Some(first) = entries.first_mut() else {
        return entries;
    };
    *first = Some(Facts::entry(inputs));

    for (pc, op) in ops.iter().enumerate() {
        if matches!(op, IntermediateOp::Call(_))
            && let Some(entry) = entries.get_mut(pc + 1)
        {
            *entry = Some(Facts::unknown());
        }
    }

    loop {
        let mut changed = false;
        for (pc, op) in ops.iter().enumerate() {
            let Some(facts) = entries[pc].as_ref() else {
                continue;
            };
            let mut output = facts.clone();
            transfer(op, &mut output);
            for successor in successors(op, pc).into_iter().flatten() {
                let Some(entry) = entries.get_mut(successor) else {
                    continue;
                };
                let merged = match entry.as_ref() {
                    Some(existing) => existing.merge(&output),
                    None => output.clone(),
                };
                if entry.as_ref() != Some(&merged) {
                    *entry = Some(merged);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    entries
}

fn propagate(source: &mut Source, facts: &Facts) {
    if let Source::Register(index) = source
        && let Some(value) = facts.registers[*index].constant()
    {
        *source = Source::Value(value);
    }
}

fn substitute(op: &mut IntermediateOp, facts: &Facts) {
    match op {
        IntermediateOp::PushRegister(index) => {
            if let Some(value) = facts.registers[*index].constant() {
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
/// Records against every op which of the two changed it, so a later stage can
/// say why an op no longer looks like what was written.
fn rewrite(ops: &mut [IntermediateOp], entries: &[Option<Facts>], rewrites: &mut [Rewrites]) {
    for pc in 0..ops.len() {
        let Some(facts) = &entries[pc] else {
            rewrites[pc].unreached = true;
            continue;
        };
        let original = ops[pc];
        let mut op = original;
        substitute(&mut op, facts);
        rewrites[pc].substituted = op != original;
        ops[pc] = fold(op, pc);
        rewrites[pc].folded = ops[pc] != op;
    }
}

/// Sends jumps whose destination is itself an unconditional jump straight to
/// the final destination. Chains are followed a bounded number of steps so a
/// cycle of jumps — an intentional infinite loop — terminates.
fn thread(ops: &mut [IntermediateOp], rewrites: &mut [Rewrites]) {
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
    for (pc, op) in ops.iter_mut().enumerate() {
        let original = *op;
        op.remap(|target| resolved.get(target).copied().unwrap_or(target));
        rewrites[pc].threaded = *op != original;
    }
}

/// Drops jumps to the immediately following op — left behind by folded
/// conditions — and remaps every target into the shortened program. Returns
/// the surviving ops and, for each one, the index it held on the way in.
fn compact(ops: Vec<IntermediateOp>) -> (Vec<IntermediateOp>, Vec<usize>) {
    let mut map = Vec::with_capacity(ops.len());
    let mut compacted = Vec::with_capacity(ops.len());
    let mut origins = Vec::with_capacity(ops.len());
    for (pc, op) in ops.iter().enumerate() {
        map.push(compacted.len());
        if let IntermediateOp::Jump(target) = op
            && *target == pc + 1
        {
            continue;
        }
        compacted.push(*op);
        origins.push(pc);
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
    (compacted, origins)
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
    inputs: Vec<ValueType>,
    origins: Vec<usize>,
    rewrites: Vec<Rewrites>,
    destinations: Vec<Option<usize>>,
}

impl OptimizedProgram {
    pub fn compile(program: &IntermediateProgram) -> Self {
        Self::compile_with_inputs(program, &[])
    }

    /// Compiles against a declared entry stack: `inputs` lists the types of
    /// the values the caller pushes before running, deepest first. The
    /// analysis trusts the declaration, so the machine verifies it against
    /// the actual stack whenever execution enters at instruction zero.
    pub fn compile_with_inputs(program: &IntermediateProgram, inputs: &[ValueType]) -> Self {
        let mut ops = program.ops().to_vec();
        let mut rewrites = vec![Rewrites::default(); ops.len()];
        let entries = analyze(&ops, inputs);
        rewrite(&mut ops, &entries, &mut rewrites);
        thread(&mut ops, &mut rewrites);
        let (ops, origins) = compact(ops);

        // Rewrites are recorded against the incoming indices and stay there;
        // what every incoming op needs on top of them is where it landed, or
        // that compaction dropped it.
        let mut destinations = vec![None; rewrites.len()];
        for (index, origin) in origins.iter().enumerate() {
            destinations[*origin] = Some(index);
        }

        // The analysis runs again over the final ops so the recorded types
        // line up with the compacted indices.
        let types = analyze(&ops, inputs)
            .into_iter()
            .map(|facts| facts.as_ref().map(RegisterTypes::of).unwrap_or_default())
            .collect();

        Self {
            ops,
            types,
            inputs: inputs.to_vec(),
            origins,
            rewrites,
            destinations,
        }
    }

    pub fn ops(&self) -> &[IntermediateOp] {
        &self.ops
    }

    /// The inferred register types on entry to every op, parallel to `ops`.
    pub fn types(&self) -> &[RegisterTypes] {
        &self.types
    }

    /// The declared types of the entry stack, deepest first.
    pub fn inputs(&self) -> &[ValueType] {
        &self.inputs
    }

    /// The intermediate op every op came from, parallel to `ops`. Rewriting is
    /// one-for-one, so an op's origin differs from its own index only by
    /// however many ops before it compaction dropped.
    pub fn origins(&self) -> &[usize] {
        &self.origins
    }

    /// What the optimizer did to every intermediate op, parallel to the
    /// intermediate program's ops rather than to `ops`, so the record survives
    /// for ops that were dropped.
    pub fn rewrites(&self) -> &[Rewrites] {
        &self.rewrites
    }

    /// Where every intermediate op landed, parallel to the intermediate
    /// program's ops: its index in `ops`, or `None` if it was dropped.
    pub fn destinations(&self) -> &[Option<usize>] {
        &self.destinations
    }
}
