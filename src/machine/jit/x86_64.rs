use super::{
    FastBinary, FastBinaryOp, FastDest, FastOperand, PAYLOAD_OFFSET, PushSource, Spill, StackFields,
    StackKind,
};
use crate::machine::error::{MachineError, Result};

// Register conventions for generated code: rbx holds the machine state pointer,
// r12 the entry table base (both callee-saved), rax/rcx are operand scratch,
// and dl the tag scratch. r13/r14/r15/rbp are handed to the register allocator
// to pin hot VM registers (callee-saved, so they survive helper calls). Helper
// argument registers differ between the System V and Windows x64 conventions.
//
// That leaves r10 and r11 — caller-saved under both conventions and argument
// registers under neither — for the value stack's `length` and `data`. There is
// nothing left for its `capacity` or for the call stack, so those stay in
// memory; sixteen registers do not go as far as thirty-two.
//
// The frame keeps rsp 16-byte aligned at every call site; Windows additionally
// requires 32 bytes of shadow space above rsp. Each pinned register is pushed
// (8 bytes), so an odd count is padded with 8 extra frame bytes to preserve
// alignment.
const STATE: u8 = 3; // rbx (r12 holds the entry table, addressed inline)
/// `shl` amount that scales a stack index into a byte offset, since a
/// `MachineValue` is sixteen bytes.
const VALUE_SHIFT: u8 = 4;

#[cfg(windows)]
const FRAME_BASE: u8 = 40;
#[cfg(not(windows))]
const FRAME_BASE: u8 = 8;

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
            length: Some(10),
            capacity: None,
            data: Some(11),
        },
        StackKind::Call => StackPins {
            length: None,
            capacity: None,
            data: None,
        },
    }
}

/// Every stack field held in a cpu register, as `(register, offset, written)`.
/// `written` marks the ones generated code updates — the lengths — which are the
/// only ones that have to reach memory again.
fn stack_pins() -> Vec<(u8, usize, bool)> {
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
                pins.push((register as u8, offset, written));
            }
        }
    }
    pins
}

enum FixupTarget {
    Op(usize),
    Epilogue,
}

struct Fixup {
    at: usize,
    target: FixupTarget,
}

/// A forward branch within a single op's code, patched by `bind` as soon as
/// its destination is emitted.
struct PendingBranch {
    at: usize,
}

pub(super) struct Assembler {
    code: Vec<u8>,
    fixups: Vec<Fixup>,
    table: usize,
    epilogue_offset: usize,
    /// The `(cpu register, state slot)` pair for each pinned VM register.
    pinned: Vec<(u32, usize)>,
}

impl Assembler {
    pub(super) const PIN_REGISTERS: &'static [u32] = &[13, 14, 15, 5]; // r13, r14, r15, rbp

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

    fn bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    /// Frame size that keeps rsp 16-byte aligned at call sites given the pushed
    /// callee-saved registers (rbx, r12, and the pinned set).
    fn frame(&self) -> u8 {
        FRAME_BASE + if self.pinned.len() % 2 == 1 { 8 } else { 0 }
    }

    fn branch_fixup(&mut self, opcode: &[u8], target: FixupTarget) {
        self.bytes(opcode);
        self.fixups.push(Fixup {
            at: self.code.len(),
            target,
        });
        self.bytes(&[0; 4]);
    }

    /// A REX.W prefix carrying the high bits of the ModRM `reg` and `rm` fields.
    fn rex(&mut self, reg: u8, rm: u8) {
        self.bytes(&[0x48 | (((reg >> 3) & 1) << 2) | ((rm >> 3) & 1)]);
    }

    fn move_immediate(&mut self, rex: u8, register: u8, value: u64) {
        self.bytes(&[rex, 0xB8 + register]); // mov r64, imm64
        self.bytes(&value.to_le_bytes());
    }

    /// `mov register, imm64` for any register.
    fn move_immediate_register(&mut self, register: u8, value: u64) {
        self.rex(0, register);
        self.bytes(&[0xB8 + (register & 7)]);
        self.bytes(&value.to_le_bytes());
    }

    /// `mov register, [base + byte_offset]`. `base` must not be `rsp` or `r12`,
    /// whose encoding would need a SIB byte.
    fn load_from(&mut self, register: u8, base: u8, byte_offset: usize) {
        self.rex(register, base);
        self.bytes(&[0x8B, 0x80 | ((register & 7) << 3) | (base & 7)]);
        self.bytes(&(byte_offset as i32).to_le_bytes());
    }

    /// `mov [base + byte_offset], register`, with the same restriction on `base`.
    fn store_to(&mut self, register: u8, base: u8, byte_offset: usize) {
        self.rex(register, base);
        self.bytes(&[0x89, 0x80 | ((register & 7) << 3) | (base & 7)]);
        self.bytes(&(byte_offset as i32).to_le_bytes());
    }

    /// `mov register, [rbx + byte_offset]` for any register.
    fn load(&mut self, register: u8, byte_offset: usize) {
        self.load_from(register, STATE, byte_offset);
    }

    /// `mov [rbx + byte_offset], register` for any register.
    fn store(&mut self, register: u8, byte_offset: usize) {
        self.store_to(register, STATE, byte_offset);
    }

    fn compare_registers(&mut self, lhs: u8, rhs: u8) {
        self.rex(rhs, lhs);
        self.bytes(&[0x39, 0xC0 | ((rhs & 7) << 3) | (lhs & 7)]);
    }

    /// `shl register, amount`.
    fn shift_left(&mut self, register: u8, amount: u8) {
        self.rex(0, register);
        self.bytes(&[0xC1, 0xE0 | (register & 7), amount]);
    }

    fn move_register(&mut self, destination: u8, source: u8) {
        self.rex(source, destination);
        self.bytes(&[0x89, 0xC0 | ((source & 7) << 3) | (destination & 7)]);
    }

    fn add_registers(&mut self, destination: u8, source: u8) {
        self.rex(source, destination);
        self.bytes(&[0x01, 0xC0 | ((source & 7) << 3) | (destination & 7)]);
    }

    fn subtract_registers(&mut self, destination: u8, source: u8) {
        self.rex(source, destination);
        self.bytes(&[0x29, 0xC0 | ((source & 7) << 3) | (destination & 7)]);
    }

    fn multiply_registers(&mut self, destination: u8, source: u8) {
        self.rex(destination, source);
        self.bytes(&[0x0F, 0xAF, 0xC0 | ((destination & 7) << 3) | (source & 7)]);
    }

    fn add_immediate(&mut self, destination: u8, value: u32) {
        self.rex(0, destination);
        self.bytes(&[0x81, 0xC0 | (destination & 7)]); // /0
        self.bytes(&value.to_le_bytes());
    }

    fn subtract_immediate(&mut self, destination: u8, value: u32) {
        self.rex(0, destination);
        self.bytes(&[0x81, 0xE8 | (destination & 7)]); // /5
        self.bytes(&value.to_le_bytes());
    }

    fn push_reg(&mut self, register: u8) {
        if register >= 8 {
            self.bytes(&[0x41]);
        }
        self.bytes(&[0x50 + (register & 7)]);
    }

    fn pop_reg(&mut self, register: u8) {
        if register >= 8 {
            self.bytes(&[0x41]);
        }
        self.bytes(&[0x58 + (register & 7)]);
    }

    fn test_status(&mut self) {
        self.bytes(&[0x48, 0x85, 0xC0]); // test rax, rax
    }

    /// `test register, register`, for a zero check on something other than rax.
    fn test_register(&mut self, register: u8) {
        self.rex(register, register);
        self.bytes(&[0x85, 0xC0 | ((register & 7) << 3) | (register & 7)]);
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
    fn stack_field(&mut self, pin: Option<u32>, offset: usize, scratch: u8) -> u8 {
        match pin {
            Some(register) => register as u8,
            None => {
                self.load(scratch, offset);
                scratch
            }
        }
    }

    /// Commits a length computed in `source`: nothing to do when `source` is
    /// already the register the length is held in.
    fn set_stack_length(&mut self, pin: Option<u32>, offset: usize, source: u8) {
        match pin {
            Some(register) if register as u8 == source => {}
            Some(register) => self.move_register(register as u8, source),
            None => self.store(source, offset),
        }
    }

    fn branch_negative_epilogue(&mut self) {
        self.branch_fixup(&[0x0F, 0x88], FixupTarget::Epilogue); // js
    }

    /// Entered as `function(state, entry)`; establishes the frame and pinned
    /// registers, loads the pinned VM registers from the bank, then jumps into
    /// the generated body at `entry`.
    pub(super) fn prologue(&mut self) {
        let table = self.table as u64;
        let pinned = self.pinned.clone();
        let frame = self.frame();
        self.bytes(&[0x53]); // push rbx
        self.bytes(&[0x41, 0x54]); // push r12
        for (register, _) in &pinned {
            self.push_reg(*register as u8);
        }
        self.bytes(&[0x48, 0x83, 0xEC, frame]); // sub rsp, frame
        #[cfg(windows)]
        self.bytes(&[0x48, 0x89, 0xCB]); // mov rbx, rcx
        #[cfg(not(windows))]
        self.bytes(&[0x48, 0x89, 0xFB]); // mov rbx, rdi
        self.move_immediate(0x49, 4, table); // mov r12, table
        for (register, slot) in &pinned {
            self.load(*register as u8, slot + PAYLOAD_OFFSET);
        }
        self.load_stack_pins();
        #[cfg(windows)]
        self.bytes(&[0xFF, 0xE2]); // jmp rdx
        #[cfg(not(windows))]
        self.bytes(&[0xFF, 0xE6]); // jmp rsi
    }

    /// The single exit from generated code. Pinned registers hold payloads their
    /// bank slots do not, so they are written back here, once, rather than at
    /// every call along the way.
    ///
    /// The stack length goes back too, though as it stands every path that
    /// reaches here ran a helper first — `Exit`, the overflow stub, a failed
    /// check — and a helper call already spills it. Writing it anyway makes
    /// coherence on the way out a property of the exit itself, not of what each
    /// path happens to do on its way to it.
    pub(super) fn epilogue(&mut self) {
        self.epilogue_offset = self.code.len();
        self.spill_pinned();
        self.spill_stack_pins();
        let pinned = self.pinned.clone();
        let frame = self.frame();
        self.bytes(&[0x48, 0x83, 0xC4, frame]); // add rsp, frame
        for (register, _) in pinned.iter().rev() {
            self.pop_reg(*register as u8);
        }
        self.bytes(&[0x41, 0x5C]); // pop r12
        self.bytes(&[0x5B]); // pop rbx
        self.bytes(&[0xC3]); // ret
    }

    /// Writes every pinned register's payload back to its bank slot.
    fn spill_pinned(&mut self) {
        let pinned = self.pinned.clone();
        for (register, slot) in &pinned {
            self.store(*register as u8, slot + PAYLOAD_OFFSET);
        }
    }

    /// Brings the bank slots a helper is about to read up to date.
    fn apply_spill(&mut self, spill: Spill) {
        match spill {
            Spill::None => {}
            Spill::One { register, slot } => self.store(register as u8, slot + PAYLOAD_OFFSET),
            Spill::All => self.spill_pinned(),
        }
    }

    /// Reloads every pinned register from its bank slot: used after a helper
    /// wrote the bank directly, leaving the CPU registers stale.
    pub(super) fn reload_pinned(&mut self) {
        let pinned = self.pinned.clone();
        for (register, slot) in &pinned {
            self.load(*register as u8, slot + PAYLOAD_OFFSET);
        }
    }

    /// Reloads a single pinned register from its slot.
    pub(super) fn reload_one(&mut self, register: u32, slot: usize) {
        self.load(register as u8, slot + PAYLOAD_OFFSET);
    }

    pub(super) fn call(&mut self, function: *const (), args: &[u64], spill: Spill) {
        #[cfg(windows)]
        const ARGUMENTS: [(u8, u8); 2] = [(0x48, 2), (0x49, 0)]; // rdx, r8
        #[cfg(not(windows))]
        const ARGUMENTS: [(u8, u8); 2] = [(0x48, 6), (0x48, 2)]; // rsi, rdx

        // Flush whatever the helper reads from the bank; callers reload
        // afterward if it may have written the bank.
        self.apply_spill(spill);
        // A helper reaches the stacks through the state, so the length held in a
        // register goes back to memory before it runs and every held field is
        // read again after: it may have pushed, popped, or grown a stack, and
        // growing one moves its buffer. The reload leaves rax alone, which is
        // where the helper's status is waiting.
        self.spill_stack_pins();
        #[cfg(windows)]
        self.bytes(&[0x48, 0x89, 0xD9]); // mov rcx, rbx
        #[cfg(not(windows))]
        self.bytes(&[0x48, 0x89, 0xDF]); // mov rdi, rbx
        for (index, argument) in args.iter().enumerate() {
            let (rex, register) = ARGUMENTS[index];
            self.move_immediate(rex, register, *argument);
        }
        self.move_immediate(0x48, 0, function as u64); // mov rax, function
        self.bytes(&[0xFF, 0xD0]); // call rax
        self.load_stack_pins();
    }

    pub(super) fn check_error(&mut self) {
        self.test_status();
        self.branch_negative_epilogue();
    }

    pub(super) fn branch_status(&mut self, target: usize) {
        self.test_status();
        self.branch_negative_epilogue();
        self.branch_fixup(&[0x0F, 0x85], FixupTarget::Op(target)); // jnz
    }

    pub(super) fn branch_taken(&mut self, target: usize) {
        self.test_status();
        self.branch_fixup(&[0x0F, 0x85], FixupTarget::Op(target)); // jnz
    }

    pub(super) fn jump(&mut self, target: usize) {
        self.branch_fixup(&[0xE9], FixupTarget::Op(target)); // jmp
    }

    pub(super) fn jump_epilogue(&mut self) {
        self.branch_fixup(&[0xE9], FixupTarget::Epilogue); // jmp
    }

    /// Returns without calling a helper: pop the call stack, confirm the value
    /// is a return address inside the program, and dispatch through the entry
    /// table. Anything unexpected — an empty stack, a foreign tag, a target past
    /// the end — branches to `slow` so the helper reports the error the
    /// interpreter would. The fast path always transfers control, so no branch
    /// over the slow path is needed.
    pub(super) fn return_inline(
        &mut self,
        stack: StackFields,
        length: u32,
        tag: u8,
        slow: impl FnOnce(&mut Self),
    ) {
        let pins = pins_of(stack.kind);
        let mut checks = Vec::new();
        // The popped depth is built in rax and rcx addresses the slot; the target
        // goes to r8 so the depth survives to be committed once every check has
        // passed, which also keeps a held depth intact until then.
        let depth = self.stack_field(pins.length, stack.length, 0);
        self.test_register(depth);
        checks.push(self.forward(&[0x0F, 0x84])); // jz
        if depth != 0 {
            self.move_register(0, depth);
        }
        self.subtract_immediate(0, 1);
        self.slot_address(stack, 0);
        self.bytes(&[0x80, 0x39, tag]); // cmp byte [rcx], tag
        checks.push(self.forward(&[0x0F, 0x85])); // jne
        self.load_from(8, 1, PAYLOAD_OFFSET);
        self.rex(0, 8); // cmp r8, length
        self.bytes(&[0x81, 0xF8]);
        self.bytes(&length.to_le_bytes());
        checks.push(self.forward(&[0x0F, 0x83])); // jae
        // Every check passed, so the pop is committed and the target dispatched.
        self.set_stack_length(pins.length, stack.length, 0);
        self.move_register(0, 8);
        self.bytes(&[0x49, 0x8B, 0x04, 0xC4]); // mov rax, [r12 + rax*8]
        self.bytes(&[0xFF, 0xE0]); // jmp rax
        for check in checks {
            self.bind(check);
        }
        slow(self);
    }

    pub(super) fn return_dispatch(&mut self) {
        self.test_status();
        self.branch_negative_epilogue();
        self.bytes(&[0x49, 0x8B, 0x04, 0xC4]); // mov rax, [r12 + rax*8]
        self.bytes(&[0xFF, 0xE0]); // jmp rax
    }

    fn forward(&mut self, opcode: &[u8]) -> PendingBranch {
        self.bytes(opcode);
        let branch = PendingBranch {
            at: self.code.len(),
        };
        self.bytes(&[0; 4]);
        branch
    }

    fn bind(&mut self, branch: PendingBranch) {
        let relative = (self.code.len() - (branch.at + 4)) as u32;
        let position = branch.at;
        self.code[position..position + 4].copy_from_slice(&relative.to_le_bytes());
    }

    /// Emits a ModRM byte and 32-bit displacement addressing `[rbx + disp]`,
    /// for a low register (`rax`/`rcx`/`rdx`) in the `reg` field.
    fn memory(&mut self, register: u8, displacement: i32) {
        self.bytes(&[0x83 | (register << 3)]);
        self.bytes(&displacement.to_le_bytes());
    }

    /// Loads an operand's payload into `target` (0 = rax, 1 = rcx): a pinned
    /// register is moved in, a trusted slot loaded from memory, a checked slot
    /// loaded after a `Uint64` tag check whose failure branches to the pending
    /// slow path, or an immediate materialized.
    fn operand_into(&mut self, operand: FastOperand, target: u8, checks: &mut Vec<PendingBranch>) {
        match operand {
            FastOperand::Trusted {
                pin: Some(register),
                ..
            } => {
                let register = register as u8;
                if register != target {
                    self.move_register(target, register);
                }
            }
            FastOperand::Trusted { slot, pin: None } => self.load(target, slot + PAYLOAD_OFFSET),
            FastOperand::Checked { slot } => {
                self.bytes(&[0x0F, 0xB6]); // movzx edx, byte [rbx + slot]
                self.memory(2, slot as i32);
                self.bytes(&[0x80, 0xFA, super::uint64_tag()]); // cmp dl, tag
                checks.push(self.forward(&[0x0F, 0x85])); // jne slow
                self.load(target, slot + PAYLOAD_OFFSET);
            }
            FastOperand::Immediate(value) => self.move_immediate_register(target, value),
        }
    }

    /// Records the result of a fast-path write, which is in rax. A memory
    /// destination's only home is its slot, so the payload is stored there. A
    /// pinned destination already holds the payload in its CPU register
    /// (spilled at call boundaries), so only the tag is written through, and
    /// only when the slot did not already hold a `Uint64`.
    fn store_through(&mut self, destination: &FastDest, write_tag: bool) {
        if destination.pin.is_none() {
            self.store(0, destination.slot + PAYLOAD_OFFSET); // mov [rbx + payload], rax
        }
        if write_tag {
            self.bytes(&[0x48, 0xC7]); // mov qword [rbx + slot], tag
            self.memory(0, destination.slot as i32);
            self.bytes(&u32::from(super::uint64_tag()).to_le_bytes());
        }
    }

    /// Leaves `rcx` pointing at the slot `index` addresses in `stack`. x86 scaled
    /// addressing tops out at a factor of eight, so the shift is explicit; it is
    /// done in `rcx` itself, which leaves `index` intact and needs `rdx` only
    /// when `data` had to be loaded.
    fn slot_address(&mut self, stack: StackFields, index: u8) {
        self.move_register(1, index);
        self.shift_left(1, VALUE_SHIFT);
        match pins_of(stack.kind).data {
            Some(register) => self.add_registers(1, register as u8),
            None => {
                self.load(2, stack.data);
                self.add_registers(1, 2);
            }
        }
    }

    /// Pushes a value onto `stack` without calling a helper: bounds-check the
    /// length against the capacity, store both words at the top, and write back
    /// the incremented length. The check reaching `slow` is taken when the stack
    /// is full and has to grow.
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
        // jae — length and capacity are sizes and length never exceeds
        // capacity, so this is taken exactly when the stack is full.
        checks.push(self.forward(&[0x0F, 0x83]));
        self.slot_address(stack, length);
        match source {
            PushSource::Slot(offset) => {
                self.load(8, offset);
                self.load(9, offset + PAYLOAD_OFFSET);
            }
            PushSource::PinnedSlot { slot, register } => {
                self.load(8, slot);
                self.move_register(9, register as u8);
            }
            PushSource::Constant(address) => {
                self.move_immediate_register(2, address as u64);
                self.load_from(8, 2, 0);
                self.load_from(9, 2, PAYLOAD_OFFSET);
            }
            PushSource::Words { tag, payload } => {
                self.move_immediate_register(8, tag);
                self.move_immediate_register(9, payload);
            }
        }
        self.store_to(8, 1, 0);
        self.store_to(9, 1, PAYLOAD_OFFSET);
        // Past the only check, so the length can move on: in place when it is
        // held in a register, and through the scratch it was loaded into if not.
        self.add_immediate(length, 1);
        self.set_stack_length(pins.length, stack.length, length);
        self.slow_path(checks, slow);
    }

    /// Pops a value off `stack` into a register slot without calling a helper.
    /// Both words go to the slot, so its tag stays right for helpers and
    /// unproven reads, and a pinned destination also takes the payload directly
    /// from the popped value. The check reaching `slow` is taken when the stack
    /// is empty, so the helper reports the error.
    pub(super) fn pop_inline(
        &mut self,
        stack: StackFields,
        destination: FastDest,
        slow: impl FnOnce(&mut Self),
    ) {
        let pins = pins_of(stack.kind);
        let mut checks = Vec::new();
        let length = self.stack_field(pins.length, stack.length, 0);
        self.test_register(length);
        checks.push(self.forward(&[0x0F, 0x84])); // jz
        // The empty check is the only way out, so the pop is already committed.
        self.subtract_immediate(length, 1);
        self.slot_address(stack, length);
        self.load_from(8, 1, 0);
        self.load_from(9, 1, PAYLOAD_OFFSET);
        self.store(8, destination.slot);
        self.store(9, destination.slot + PAYLOAD_OFFSET);
        self.set_stack_length(pins.length, stack.length, length);
        if let Some(register) = destination.pin {
            self.move_register(register as u8, 9);
        }
        self.slow_path(checks, slow);
    }

    fn slow_path(&mut self, checks: Vec<PendingBranch>, emit: impl FnOnce(&mut Self)) {
        if checks.is_empty() {
            return;
        }
        let done = self.forward(&[0xE9]); // jmp done
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
        // add/sub of a base operand and an immediate that fits a sign-extended
        // imm32 fold into a single instruction, sparing the scratch load.
        if let Some((base, value)) = super::immediate_form(kind, lhs, rhs)
            && value <= i32::MAX as u64
        {
            self.operand_into(base, 0, &mut checks); // rax
            match kind {
                FastBinaryOp::Add => self.add_immediate(0, value as u32),
                FastBinaryOp::Subtract => self.subtract_immediate(0, value as u32),
                FastBinaryOp::Multiply => unreachable!(),
            }
        } else {
            self.operand_into(lhs, 0, &mut checks); // rax
            self.operand_into(rhs, 1, &mut checks); // rcx
            match kind {
                FastBinaryOp::Add => self.add_registers(0, 1), // rax += rcx
                FastBinaryOp::Subtract => self.subtract_registers(0, 1), // rax -= rcx
                FastBinaryOp::Multiply => self.multiply_registers(0, 1), // rax *= rcx
            }
        }
        // The result is in rax; a pinned destination also keeps it in its
        // register, spilled to the bank only at call boundaries.
        if let Some(register) = dst.pin {
            self.move_register(register as u8, 0);
        }
        self.store_through(&dst, write_tag);
        self.slow_path(checks, |assembler| {
            assembler.call(helper, &[op], Spill::All);
            assembler.reload_pinned();
        });
    }

    /// Copies a proven `Uint64` payload from `source` to `destination`, keeping
    /// it in the destination's pinned register when it has one and writing it
    /// through to the slot otherwise.
    pub(super) fn copy_register(
        &mut self,
        source: FastOperand,
        destination: FastDest,
        write_tag: bool,
    ) {
        let mut checks = Vec::new();
        self.operand_into(source, 0, &mut checks); // rax
        debug_assert!(checks.is_empty(), "a proven source never needs a tag check");
        if let Some(register) = destination.pin {
            self.move_register(register as u8, 0);
        }
        self.store_through(&destination, write_tag);
    }

    pub(super) fn copy_slot(&mut self, source: usize, destination: usize) {
        for offset in [0, PAYLOAD_OFFSET] {
            self.bytes(&[0x48, 0x8B]); // mov rax, [rbx + source]
            self.memory(0, (source + offset) as i32);
            self.bytes(&[0x48, 0x89]); // mov [rbx + destination], rax
            self.memory(0, (destination + offset) as i32);
        }
    }

    pub(super) fn copy_constant(&mut self, constant: usize, destination: usize) {
        self.move_immediate(0x48, 0, constant as u64); // mov rax, constant
        self.bytes(&[0x48, 0x8B, 0x08]); // mov rcx, [rax]
        self.bytes(&[0x48, 0x89]); // mov [rbx + destination], rcx
        self.memory(1, destination as i32);
        self.bytes(&[0x48, 0x8B, 0x48, 0x08]); // mov rcx, [rax + 8]
        self.bytes(&[0x48, 0x89]); // mov [rbx + destination + 8], rcx
        self.memory(1, (destination + PAYLOAD_OFFSET) as i32);
    }

    pub(super) fn jump_if_zero_fast(
        &mut self,
        source: FastOperand,
        target: usize,
        helper: *const (),
        op: u64,
    ) {
        let mut checks = Vec::new();
        self.operand_into(source, 0, &mut checks); // rax
        self.test_status();
        self.branch_fixup(&[0x0F, 0x84], FixupTarget::Op(target)); // jz
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
        self.operand_into(lhs, 0, &mut checks); // rax
        self.operand_into(rhs, 1, &mut checks); // rcx
        self.bytes(&[0x48, 0x39, 0xC8]); // cmp rax, rcx
        self.branch_fixup(&[0x0F, 0x84], FixupTarget::Op(target)); // je
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
            let delta = destination as i64 - (fixup.at + 4) as i64;
            let Ok(relative) = i32::try_from(delta) else {
                return Err(MachineError::InstructionOverflow);
            };
            let position = fixup.at;
            self.code[position..position + 4].copy_from_slice(&relative.to_le_bytes());
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.code
    }
}
