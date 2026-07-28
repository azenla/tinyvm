use super::{
    FastBinary, FastBinaryOp, FastDest, FastOperand, PAYLOAD_OFFSET, PushSource, Spill, StackFields,
    StackKind,
};
use crate::machine::error::{MachineError, Result};

// Register conventions for generated code: x19 holds the machine state
// pointer, x20 the entry table base (both callee-saved), and x16 is the
// intra-procedure scratch register used for helper addresses. x0/x1 are
// operand scratch and w2 the tag scratch; x21-x28 are handed to the register
// allocator to pin hot VM registers (callee-saved, so they survive helper
// calls untouched). x5-x10 hold both stacks' `length`, `capacity` and `data`,
// which is what lets a push or a pop touch memory only for the value it moves.
const STATE: u32 = 19;
const TABLE: u32 = 20;
const SCRATCH: u32 = 16;
const ARGUMENTS: [u32; 3] = [0, 1, 2];
const ZERO_REGISTER: u32 = 31;
/// `lsl` amount that scales a stack index into a byte offset, since a
/// `MachineValue` is sixteen bytes.
const VALUE_SHIFT: u32 = 4;

/// The cpu registers one stack's fields are kept in. These are caller-saved, so
/// they say nothing across a helper call and are reloaded after every one; that
/// is wanted anyway, since a helper that grows a stack moves its buffer.
#[derive(Clone, Copy)]
struct StackPins {
    length: Option<u32>,
    capacity: Option<u32>,
    data: Option<u32>,
}

fn pins_of(kind: StackKind) -> StackPins {
    match kind {
        StackKind::Value => StackPins {
            length: Some(5),
            capacity: Some(6),
            data: Some(7),
        },
        StackKind::Call => StackPins {
            length: Some(8),
            capacity: Some(9),
            data: Some(10),
        },
    }
}

/// Every stack field held in a cpu register, as `(register, offset, written)`.
/// `written` marks the ones generated code updates — the lengths — which are the
/// only ones that have to reach memory again.
fn stack_pins() -> Vec<(u32, usize, bool)> {
    let mut pins = Vec::new();
    for kind in StackKind::ALL {
        let fields = super::stack_fields(kind);
        let held = pins_of(kind);
        for (pin, offset, written) in [
            (held.length, fields.length, true),
            (held.capacity, fields.capacity, false),
            (held.data, fields.data, false),
        ] {
            if let Some(register) = pin {
                pins.push((register, offset, written));
            }
        }
    }
    pins
}

enum FixupKind {
    Imm26,
    Imm19,
}

enum FixupTarget {
    Op(usize),
    Epilogue,
}

struct Fixup {
    at: usize,
    kind: FixupKind,
    target: FixupTarget,
}

/// A forward branch within a single op's code, patched by `bind` as soon as
/// its destination is emitted.
struct PendingBranch {
    at: usize,
    kind: FixupKind,
}

pub(super) struct Assembler {
    code: Vec<u8>,
    fixups: Vec<Fixup>,
    table: usize,
    epilogue_offset: usize,
    /// The `(cpu register, state slot)` pair for each pinned VM register.
    pinned: Vec<(u32, usize)>,
}

/// Pairs pinned registers for `stp`/`ldp`, padding an odd count with the zero
/// register so the stack stays 16-byte aligned.
fn pin_pairs(pinned: &[(u32, usize)]) -> Vec<(u32, u32)> {
    let registers: Vec<u32> = pinned.iter().map(|(register, _)| *register).collect();
    let mut pairs = Vec::new();
    let mut index = 0;
    while index < registers.len() {
        let first = registers[index];
        let second = registers.get(index + 1).copied().unwrap_or(ZERO_REGISTER);
        pairs.push((first, second));
        index += 2;
    }
    pairs
}

impl Assembler {
    pub(super) const PIN_REGISTERS: &'static [u32] = &[21, 22, 23, 24, 25, 26, 27, 28];

    pub(super) fn new(table: usize, pinned: Vec<(u32, usize)>) -> Self {
        Self {
            code: Vec::new(),
            fixups: Vec::new(),
            table,
            epilogue_offset: 0,
            pinned,
        }
    }

    pub(super) fn offset(&self) -> usize {
        self.code.len()
    }

    fn word(&mut self, word: u32) {
        self.code.extend_from_slice(&word.to_le_bytes());
    }

    fn branch_fixup(&mut self, word: u32, kind: FixupKind, target: FixupTarget) {
        self.fixups.push(Fixup {
            at: self.code.len(),
            kind,
            target,
        });
        self.word(word);
    }

    fn load_immediate(&mut self, register: u32, value: u64) {
        self.word(0xD280_0000 | (((value & 0xFFFF) as u32) << 5) | register); // movz
        for half in 1..4u32 {
            let chunk = (value >> (16 * half)) & 0xFFFF;
            if chunk != 0 {
                self.word(0xF280_0000 | (half << 21) | ((chunk as u32) << 5) | register); // movk
            }
        }
    }

    fn move_register(&mut self, destination: u32, source: u32) {
        self.word(0xAA00_03E0 | (source << 16) | destination); // orr rd, xzr, rm
    }

    /// `ldr Xt, [Xbase, #byte_offset]` — a word-aligned load.
    fn load_from(&mut self, target: u32, base: u32, byte_offset: usize) {
        let scaled = (byte_offset / 8) as u32;
        self.word(0xF940_0000 | (scaled << 10) | (base << 5) | target);
    }

    /// `ldr Xt, [x19, #byte_offset]` — a word-aligned load from the state.
    fn load(&mut self, target: u32, byte_offset: usize) {
        self.load_from(target, STATE, byte_offset);
    }

    /// `ldrb Wt, [Xbase]` — reads a value's tag byte, which sits at offset zero.
    fn load_tag(&mut self, target: u32, base: u32) {
        self.word(0x3940_0000 | (base << 5) | target);
    }

    /// `cmp Wn, #value`.
    fn compare_word_immediate(&mut self, register: u32, value: u32) {
        self.word(0x7100_001F | (value << 10) | (register << 5));
    }

    /// `cmp Xn, #value`.
    fn compare_immediate(&mut self, register: u32, value: u32) {
        self.word(0xF100_001F | (value << 10) | (register << 5));
    }

    /// `str Xt, [x19, #byte_offset]`.
    fn store(&mut self, source: u32, byte_offset: usize) {
        self.store_to(source, STATE, byte_offset);
    }

    /// `str Xt, [Xbase, #byte_offset]`.
    fn store_to(&mut self, source: u32, base: u32, byte_offset: usize) {
        let scaled = (byte_offset / 8) as u32;
        self.word(0xF900_0000 | (scaled << 10) | (base << 5) | source);
    }

    /// Both words of a value, as two single loads rather than one `ldp`.
    ///
    /// A `MachineValue` is sixteen bytes, so a pair instruction moves one in a
    /// single op, and the two-instruction form looks like a plain regression.
    /// It is not: Apple cores do not forward a pair store to a pair load, so an
    /// `stp` followed by an `ldp` of the same address stalls the load until the
    /// store reaches L1. Moving a value through the value stack or the register
    /// bank does exactly that — a `pop` writing a bank slot feeds the `push`
    /// that reads it back — which put a store-buffer stall on the loop-carried
    /// dependency of every stack-using program. Two eight-byte accesses forward
    /// normally and cost a cycle of issue instead of the stall.
    fn load_split(&mut self, first: u32, second: u32, base: u32, byte_offset: usize) {
        self.load_from(first, base, byte_offset);
        self.load_from(second, base, byte_offset + 8);
    }

    /// Both words of a value, as two single stores rather than one `stp`, so the
    /// load that reads them back can forward. See [`Self::load_split`].
    fn store_split(&mut self, first: u32, second: u32, base: u32, byte_offset: usize) {
        self.store_to(first, base, byte_offset);
        self.store_to(second, base, byte_offset + 8);
    }

    fn add_registers(&mut self, destination: u32, lhs: u32, rhs: u32) {
        self.word(0x8B00_0000 | (rhs << 16) | (lhs << 5) | destination);
    }

    fn subtract_registers(&mut self, destination: u32, lhs: u32, rhs: u32) {
        self.word(0xCB00_0000 | (rhs << 16) | (lhs << 5) | destination);
    }

    fn multiply_registers(&mut self, destination: u32, lhs: u32, rhs: u32) {
        self.word(0x9B00_7C00 | (rhs << 16) | (lhs << 5) | destination); // madd rd, rn, rm, xzr
    }

    fn add_immediate(&mut self, destination: u32, source: u32, value: u32) {
        self.word(0x9100_0000 | (value << 10) | (source << 5) | destination);
    }

    fn subtract_immediate(&mut self, destination: u32, source: u32, value: u32) {
        self.word(0xD100_0000 | (value << 10) | (source << 5) | destination);
    }

    fn compare_registers(&mut self, lhs: u32, rhs: u32) {
        self.word(0xEB00_001F | (rhs << 16) | (lhs << 5)); // subs xzr, lhs, rhs
    }

    fn compare_zero(&mut self) {
        self.word(0xF100_001F); // cmp x0, #0
    }

    fn branch_negative_epilogue(&mut self) {
        self.branch_fixup(0x5400_000B, FixupKind::Imm19, FixupTarget::Epilogue); // b.lt
    }

    /// Entered as `function(state: x0, entry: x1)`; establishes the frame and
    /// pinned registers, loads the pinned VM registers from the bank, then
    /// jumps into the generated body at `entry`.
    pub(super) fn prologue(&mut self) {
        let table = self.table as u64;
        let pinned = self.pinned.clone();
        self.word(0xA9BF_7BFD); // stp x29, x30, [sp, #-16]!
        self.word(0x9100_03FD); // mov x29, sp
        self.word(0xA9BF_53F3); // stp x19, x20, [sp, #-16]!
        for (first, second) in pin_pairs(&pinned) {
            self.word(0xA9BF_03E0 | (second << 10) | first); // stp first, second, [sp, #-16]!
        }
        self.move_register(STATE, ARGUMENTS[0]);
        self.load_immediate(TABLE, table);
        for (register, slot) in &pinned {
            self.load(*register, slot + PAYLOAD_OFFSET);
        }
        self.load_stack_pins();
        self.word(0xD61F_0020); // br x1
    }

    /// The single exit from generated code. Pinned registers hold payloads their
    /// bank slots do not, so they are written back here, once, rather than at
    /// every call along the way.
    ///
    /// The stack lengths go back too, though as it stands every path that
    /// reaches here ran a helper first — `Exit`, the overflow stub, a failed
    /// check — and a helper call already spills them. Writing them anyway makes
    /// coherence on the way out a property of the exit itself, not of what each
    /// path happens to do on its way to it.
    pub(super) fn epilogue(&mut self) {
        self.epilogue_offset = self.code.len();
        self.spill_pinned();
        self.spill_stack_pins();
        let pinned = self.pinned.clone();
        let mut pairs = pin_pairs(&pinned);
        pairs.reverse();
        for (first, second) in pairs {
            self.word(0xA8C1_03E0 | (second << 10) | first); // ldp first, second, [sp], #16
        }
        self.word(0xA8C1_53F3); // ldp x19, x20, [sp], #16
        self.word(0xA8C1_7BFD); // ldp x29, x30, [sp], #16
        self.word(0xD65F_03C0); // ret
    }

    /// Reads every held stack field out of the state.
    fn load_stack_pins(&mut self) {
        for (register, offset, _) in stack_pins() {
            self.load(register, offset);
        }
    }

    /// Writes back the held stack fields generated code updates, so whatever
    /// reads the state next — a helper, or the machine once generated code
    /// returns — sees the length the generated code has been keeping.
    fn spill_stack_pins(&mut self) {
        for (register, offset, written) in stack_pins() {
            if written {
                self.store(register, offset);
            }
        }
    }

    /// The register a stack field is held in, or `scratch` once the field has
    /// been loaded into it.
    fn stack_field(&mut self, pin: Option<u32>, offset: usize, scratch: u32) -> u32 {
        match pin {
            Some(register) => register,
            None => {
                self.load(scratch, offset);
                scratch
            }
        }
    }

    /// Commits a length computed in `source`: nothing to do when `source` is
    /// already the register the length is held in.
    fn set_stack_length(&mut self, pin: Option<u32>, offset: usize, source: u32) {
        match pin {
            Some(register) if register == source => {}
            Some(register) => self.move_register(register, source),
            None => self.store(source, offset),
        }
    }

    /// Reloads every pinned register from its bank slot: used after a helper
    /// wrote the bank directly, leaving the CPU registers stale.
    pub(super) fn reload_pinned(&mut self) {
        let pinned = self.pinned.clone();
        for (register, slot) in &pinned {
            self.load(*register, slot + PAYLOAD_OFFSET);
        }
    }

    /// Reloads a single pinned register from its slot.
    pub(super) fn reload_one(&mut self, register: u32, slot: usize) {
        self.load(register, slot + PAYLOAD_OFFSET);
    }

    pub(super) fn call(&mut self, function: *const (), args: &[u64], spill: Spill) {
        // Flush whatever the helper reads from the bank; callers reload
        // afterward if it may have written the bank.
        self.apply_spill(spill);
        // A helper reaches the stacks through the state, so the lengths held in
        // registers go back to memory before it runs and every held field is
        // read again after: it may have pushed, popped, or grown a stack, and
        // growing one moves its buffer. The reload leaves x0 alone, which is
        // where the helper's status is waiting.
        self.spill_stack_pins();
        self.move_register(ARGUMENTS[0], STATE);
        for (index, argument) in args.iter().enumerate() {
            self.load_immediate(ARGUMENTS[index + 1], *argument);
        }
        self.load_immediate(SCRATCH, function as u64);
        self.word(0xD63F_0000 | (SCRATCH << 5)); // blr x16
        self.load_stack_pins();
    }

    pub(super) fn check_error(&mut self) {
        self.compare_zero();
        self.branch_negative_epilogue();
    }

    pub(super) fn branch_status(&mut self, target: usize) {
        self.compare_zero();
        self.branch_negative_epilogue();
        self.branch_fixup(0x5400_000C, FixupKind::Imm19, FixupTarget::Op(target)); // b.gt
    }

    pub(super) fn branch_taken(&mut self, target: usize) {
        self.branch_fixup(0xB500_0000, FixupKind::Imm19, FixupTarget::Op(target)); // cbnz x0
    }

    pub(super) fn jump(&mut self, target: usize) {
        self.branch_fixup(0x1400_0000, FixupKind::Imm26, FixupTarget::Op(target)); // b
    }

    pub(super) fn jump_epilogue(&mut self) {
        self.branch_fixup(0x1400_0000, FixupKind::Imm26, FixupTarget::Epilogue); // b
    }

    /// Returns without calling a helper: pop the call stack, confirm the value
    /// is a return address inside the program, and dispatch through the entry
    /// table. Anything unexpected — an empty stack, a foreign tag, a target past
    /// the end — branches to `slow` so the helper reports the error the
    /// interpreter would. The fast path always transfers control, so no branch
    /// over the slow path is needed.
    ///
    /// `limit` bounds the target. It is compared as a 12-bit immediate, so a
    /// program with more ops than that keeps the helper.
    pub(super) fn return_inline(
        &mut self,
        stack: StackFields,
        limit: u32,
        tag: u8,
        slow: impl FnOnce(&mut Self),
    ) {
        let pins = pins_of(stack.kind);
        let mut checks = Vec::new();
        // The popped depth is built in x1, which leaves x0 free for the target
        // the dispatch wants and keeps the held depth intact until the checks
        // that could still send this to the helper have passed.
        let depth = self.stack_field(pins.length, stack.length, 1);
        checks.push(self.forward(0xB400_0000 | depth, FixupKind::Imm19)); // cbz depth
        self.subtract_immediate(1, depth, 1);
        self.slot_address(stack, 1);
        self.load_tag(3, 2);
        self.compare_word_immediate(3, u32::from(tag));
        checks.push(self.forward(0x5400_0001, FixupKind::Imm19)); // b.ne
        self.load_from(0, 2, PAYLOAD_OFFSET);
        self.compare_immediate(0, limit);
        checks.push(self.forward(0x5400_0002, FixupKind::Imm19)); // b.hs
        self.set_stack_length(pins.length, stack.length, 1);
        self.word(0xF860_7800 | (TABLE << 5) | SCRATCH); // ldr x16, [x20, x0, lsl #3]
        self.word(0xD61F_0000 | (SCRATCH << 5)); // br x16
        for check in checks {
            self.bind(check);
        }
        slow(self);
    }

    pub(super) fn return_dispatch(&mut self) {
        self.compare_zero();
        self.branch_negative_epilogue();
        self.word(0xF860_7800 | (TABLE << 5) | SCRATCH); // ldr x16, [x20, x0, lsl #3]
        self.word(0xD61F_0000 | (SCRATCH << 5)); // br x16
    }

    fn forward(&mut self, word: u32, kind: FixupKind) -> PendingBranch {
        let branch = PendingBranch {
            at: self.code.len(),
            kind,
        };
        self.word(word);
        branch
    }

    fn bind(&mut self, branch: PendingBranch) {
        let delta = ((self.code.len() - branch.at) / 4) as u32;
        let (mask, shift) = match branch.kind {
            FixupKind::Imm26 => (0x03FF_FFFF, 0),
            FixupKind::Imm19 => (0x0007_FFFF, 5),
        };
        let position = branch.at;
        let existing = u32::from_le_bytes(self.code[position..position + 4].try_into().unwrap());
        let patched = existing | ((delta & mask) << shift);
        self.code[position..position + 4].copy_from_slice(&patched.to_le_bytes());
    }

    /// Materializes an operand's payload into a CPU register and returns it: a
    /// pinned register directly, a trusted slot loaded into `scratch`, a
    /// tag-checked slot loaded into `scratch` after a `Uint64` check whose
    /// failure branches to the pending slow path, or an immediate.
    fn operand_register(
        &mut self,
        operand: FastOperand,
        scratch: u32,
        checks: &mut Vec<PendingBranch>,
    ) -> u32 {
        match operand {
            FastOperand::Trusted {
                pin: Some(register),
                ..
            } => register,
            FastOperand::Trusted { slot, pin: _ } => {
                self.load(scratch, slot + PAYLOAD_OFFSET);
                scratch
            }
            FastOperand::Checked { slot } => {
                let tag = u32::from(super::uint64_tag());
                self.word(0x3940_0000 | ((slot as u32) << 10) | (STATE << 5) | 2); // ldrb w2, [x19, #slot]
                self.word(0x7100_001F | (tag << 10) | (2 << 5)); // cmp w2, #tag
                checks.push(self.forward(0x5400_0001, FixupKind::Imm19)); // b.ne slow
                self.load(scratch, slot + PAYLOAD_OFFSET);
                scratch
            }
            FastOperand::Immediate(value) => {
                self.load_immediate(scratch, value);
                scratch
            }
        }
    }

    /// Records the result of a fast-path write. A memory destination's only
    /// home is its slot, so the payload is stored there. A pinned destination
    /// keeps its payload in the CPU register — spilled to the slot only at call
    /// boundaries — so just the tag is written through, and only when the slot
    /// did not already hold a `Uint64`. The tag scratch (x1) never aliases a
    /// result register, which is only x0 or a pinned register.
    fn store_through(&mut self, destination: &FastDest, register: u32, write_tag: bool) {
        if destination.pin.is_none() {
            self.store(register, destination.slot + PAYLOAD_OFFSET);
        }
        if write_tag {
            self.load_immediate(1, u64::from(super::uint64_tag()));
            self.store(1, destination.slot);
        }
    }

    /// Writes every pinned register's payload back to its bank slot.
    fn spill_pinned(&mut self) {
        let pinned = self.pinned.clone();
        for (register, slot) in &pinned {
            self.store(*register, slot + PAYLOAD_OFFSET);
        }
    }

    /// Brings the bank slots a helper is about to read up to date.
    fn apply_spill(&mut self, spill: Spill) {
        match spill {
            Spill::None => {}
            Spill::One { register, slot } => self.store(register, slot + PAYLOAD_OFFSET),
            Spill::All => self.spill_pinned(),
        }
    }

    /// `add Xd, Xn, Xm, lsl #shift`.
    fn add_shifted(&mut self, destination: u32, base: u32, index: u32, shift: u32) {
        self.word(0x8B00_0000 | (index << 16) | (shift << 10) | (base << 5) | destination);
    }

    /// Leaves `x2` pointing at the slot `index` addresses in `stack`, where
    /// `index` is a register the caller has already loaded and bounds-checked.
    /// Held `data` folds the whole address into one instruction.
    fn slot_address(&mut self, stack: StackFields, index: u32) {
        let data = self.stack_field(pins_of(stack.kind).data, stack.data, 2);
        self.add_shifted(2, data, index, VALUE_SHIFT);
    }

    /// Pushes a value onto `stack` without calling a helper: bounds-check the
    /// length against the capacity, store both words at the top, and write back
    /// the incremented length. Returns the check that must reach the slow path,
    /// taken when the stack is full and has to grow.
    pub(super) fn push_inline(
        &mut self,
        stack: StackFields,
        source: PushSource,
        slow: impl FnOnce(&mut Self),
    ) {
        let pins = pins_of(stack.kind);
        let mut checks = Vec::new();
        let length = self.stack_field(pins.length, stack.length, 0);
        let capacity = self.stack_field(pins.capacity, stack.capacity, 1);
        self.compare_registers(length, capacity);
        // b.hs — length and capacity are sizes, and length never exceeds
        // capacity, so this is taken exactly when the stack is full.
        checks.push(self.forward(0x5400_0002, FixupKind::Imm19));
        self.slot_address(stack, length);
        match source {
            PushSource::Slot(offset) => self.load_split(3, 4, STATE, offset),
            PushSource::PinnedSlot { slot, register } => {
                self.load(3, slot);
                self.move_register(4, register);
            }
            PushSource::Constant(address) => {
                self.load_immediate(SCRATCH, address as u64);
                self.load_split(3, 4, SCRATCH, 0);
            }
            PushSource::Words { tag, payload } => {
                self.load_immediate(3, tag);
                self.load_immediate(4, payload);
            }
        }
        self.store_split(3, 4, 2, 0);
        // Past the only check, so the length can move on: in place when it is
        // held in a register, and through the scratch it was loaded into if not.
        self.add_immediate(length, length, 1);
        self.set_stack_length(pins.length, stack.length, length);
        self.slow_path(checks, slow);
    }

    /// Pops a value off `stack` into a register slot without calling a helper.
    /// Both words go to the slot, so its tag stays right for helpers and
    /// unproven reads, and a pinned destination also takes the payload directly
    /// from the popped value. Returns the check taken when the stack is empty,
    /// which must reach the slow path so the helper reports the error.
    pub(super) fn pop_inline(
        &mut self,
        stack: StackFields,
        destination: FastDest,
        slow: impl FnOnce(&mut Self),
    ) {
        let pins = pins_of(stack.kind);
        let mut checks = Vec::new();
        let length = self.stack_field(pins.length, stack.length, 0);
        checks.push(self.forward(0xB400_0000 | length, FixupKind::Imm19)); // cbz length
        // The empty check is the only way out, so the pop is already committed.
        self.subtract_immediate(length, length, 1);
        self.slot_address(stack, length);
        self.load_split(3, 4, 2, 0);
        self.store_split(3, 4, STATE, destination.slot);
        self.set_stack_length(pins.length, stack.length, length);
        if let Some(register) = destination.pin {
            self.move_register(register, 4);
        }
        self.slow_path(checks, slow);
    }

    fn slow_path(&mut self, checks: Vec<PendingBranch>, emit: impl FnOnce(&mut Self)) {
        if checks.is_empty() {
            return;
        }
        let done = self.forward(0x1400_0000, FixupKind::Imm26); // b done
        for check in checks {
            self.bind(check);
        }
        emit(self);
        self.bind(done);
    }

    pub(super) fn binary_fast(&mut self, binary: FastBinary) {
        let FastBinary {
            kind,
            lhs,
            rhs,
            dst,
            helper,
            op,
            write_tag,
        } = binary;
        let mut checks = Vec::new();
        // A memory destination lands its result in x0 before the store; a
        // pinned destination is computed into its own register.
        let result = dst.pin.unwrap_or(0);
        // add/sub of a base operand and an immediate that fits the imm12 field
        // fold into a single instruction, sparing the scratch-register load.
        if let Some((base, value)) = super::immediate_form(kind, lhs, rhs)
            && value <= 0xFFF
        {
            let base = self.operand_register(base, 0, &mut checks);
            match kind {
                FastBinaryOp::Add => self.add_immediate(result, base, value as u32),
                FastBinaryOp::Subtract => self.subtract_immediate(result, base, value as u32),
                FastBinaryOp::Multiply => unreachable!(),
            }
        } else {
            let left = self.operand_register(lhs, 0, &mut checks);
            let right = self.operand_register(rhs, 1, &mut checks);
            match kind {
                FastBinaryOp::Add => self.add_registers(result, left, right),
                FastBinaryOp::Subtract => self.subtract_registers(result, left, right),
                FastBinaryOp::Multiply => self.multiply_registers(result, left, right),
            }
        }
        self.store_through(&dst, result, write_tag);
        self.slow_path(checks, |assembler| {
            assembler.call(helper, &[op], Spill::All);
            assembler.reload_pinned();
        });
    }

    /// Copies a proven `Uint64` payload from `source` to `destination`,
    /// keeping it in the destination's pinned register when it has one and
    /// writing it through to the slot either way.
    pub(super) fn copy_register(
        &mut self,
        source: FastOperand,
        destination: FastDest,
        write_tag: bool,
    ) {
        let mut checks = Vec::new();
        let source = self.operand_register(source, 0, &mut checks);
        debug_assert!(checks.is_empty(), "a proven source never needs a tag check");
        let value = match destination.pin {
            Some(register) => {
                if register != source {
                    self.move_register(register, source);
                }
                register
            }
            None => source,
        };
        self.store_through(&destination, value, write_tag);
    }

    pub(super) fn copy_slot(&mut self, source: usize, destination: usize) {
        self.load_split(0, 1, STATE, source);
        self.store_split(0, 1, STATE, destination);
    }

    pub(super) fn copy_constant(&mut self, constant: usize, destination: usize) {
        self.load_immediate(SCRATCH, constant as u64);
        self.load_split(0, 1, SCRATCH, 0);
        self.store_split(0, 1, STATE, destination);
    }

    pub(super) fn jump_if_zero_fast(
        &mut self,
        source: FastOperand,
        target: usize,
        helper: *const (),
        op: u64,
    ) {
        let mut checks = Vec::new();
        let register = self.operand_register(source, 0, &mut checks);
        self.branch_fixup(
            0xB400_0000 | register,
            FixupKind::Imm19,
            FixupTarget::Op(target),
        ); // cbz register
        self.slow_path(checks, |assembler| {
            assembler.call(helper, &[op], Spill::All);
            assembler.branch_taken(target);
        });
    }

    pub(super) fn jump_if_equal_fast(
        &mut self,
        lhs: FastOperand,
        rhs: FastOperand,
        target: usize,
        helper: *const (),
        op: u64,
    ) {
        let mut checks = Vec::new();
        let left = self.operand_register(lhs, 0, &mut checks);
        let right = self.operand_register(rhs, 1, &mut checks);
        self.compare_registers(left, right);
        self.branch_fixup(0x5400_0000, FixupKind::Imm19, FixupTarget::Op(target)); // b.eq
        self.slow_path(checks, |assembler| {
            assembler.call(helper, &[op], Spill::All);
            assembler.branch_taken(target);
        });
    }

    pub(super) fn patch(&mut self, offsets: &[usize], overflow: usize) -> Result<()> {
        for fixup in &self.fixups {
            let destination = match fixup.target {
                FixupTarget::Op(index) => offsets.get(index).copied().unwrap_or(overflow),
                FixupTarget::Epilogue => self.epilogue_offset,
            };
            let delta = (destination as i64 - fixup.at as i64) / 4;
            let (bits, shift) = match fixup.kind {
                FixupKind::Imm26 => (26, 0),
                FixupKind::Imm19 => (19, 5),
            };
            if delta >= (1 << (bits - 1)) || delta < -(1 << (bits - 1)) {
                return Err(MachineError::InstructionOverflow);
            }
            let mask = (1u32 << bits) - 1;
            let position = fixup.at;
            let existing =
                u32::from_le_bytes(self.code[position..position + 4].try_into().unwrap());
            let patched = existing | (((delta as u32) & mask) << shift);
            self.code[position..position + 4].copy_from_slice(&patched.to_le_bytes());
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.code
    }
}
