use super::{FastBinary, FastBinaryOp, FastDest, FastOperand, PAYLOAD_OFFSET};
use crate::machine::error::{MachineError, Result};

// Register conventions for generated code: rbx holds the machine state pointer,
// r12 the entry table base (both callee-saved), rax/rcx are operand scratch,
// and dl the tag scratch. r13/r14/r15/rbp are handed to the register allocator
// to pin hot VM registers (callee-saved, so they survive helper calls). Helper
// argument registers differ between the System V and Windows x64 conventions.
//
// The frame keeps rsp 16-byte aligned at every call site; Windows additionally
// requires 32 bytes of shadow space above rsp. Each pinned register is pushed
// (8 bytes), so an odd count is padded with 8 extra frame bytes to preserve
// alignment.
const STATE: u8 = 3; // rbx (r12 holds the entry table, addressed inline)

#[cfg(windows)]
const FRAME_BASE: u8 = 40;
#[cfg(not(windows))]
const FRAME_BASE: u8 = 8;

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

    /// `mov register, [rbx + byte_offset]` for any register.
    fn load(&mut self, register: u8, byte_offset: usize) {
        self.rex(register, STATE);
        self.bytes(&[0x8B, 0x80 | ((register & 7) << 3) | (STATE & 7)]);
        self.bytes(&(byte_offset as i32).to_le_bytes());
    }

    /// `mov [rbx + byte_offset], register` for any register.
    fn store(&mut self, register: u8, byte_offset: usize) {
        self.rex(register, STATE);
        self.bytes(&[0x89, 0x80 | ((register & 7) << 3) | (STATE & 7)]);
        self.bytes(&(byte_offset as i32).to_le_bytes());
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
        #[cfg(windows)]
        self.bytes(&[0xFF, 0xE2]); // jmp rdx
        #[cfg(not(windows))]
        self.bytes(&[0xFF, 0xE6]); // jmp rsi
    }

    pub(super) fn epilogue(&mut self) {
        self.epilogue_offset = self.code.len();
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

    /// Writes every pinned register's payload back to its bank slot, making the
    /// bank coherent for the helper a call is about to enter.
    fn spill_pinned(&mut self) {
        let pinned = self.pinned.clone();
        for (register, slot) in &pinned {
            self.store(*register as u8, slot + PAYLOAD_OFFSET);
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

    pub(super) fn call(&mut self, function: *const (), args: &[u64]) {
        #[cfg(windows)]
        const ARGUMENTS: [(u8, u8); 2] = [(0x48, 2), (0x49, 0)]; // rdx, r8
        #[cfg(not(windows))]
        const ARGUMENTS: [(u8, u8); 2] = [(0x48, 6), (0x48, 2)]; // rsi, rdx

        // The helper reads and writes the bank directly, so flush the pinned
        // registers' payloads to it first; callers reload afterward if the
        // helper may have written the bank.
        self.spill_pinned();
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
            assembler.call(helper, &[op]);
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
            assembler.call(helper, &[op]);
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
            assembler.call(helper, &[op]);
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
