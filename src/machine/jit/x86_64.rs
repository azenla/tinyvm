use super::{FastBinaryOp, FastOperand, PAYLOAD_OFFSET};
use crate::machine::error::{MachineError, Result};

// Register conventions for generated code: rbx holds the machine state
// pointer, r12 the entry table base (both callee-saved), and rax is scratch
// for helper addresses. Argument registers differ between the System V and
// Windows x64 calling conventions.
//
// The frame keeps rsp 16-byte aligned at every call site; Windows
// additionally requires 32 bytes of shadow space above rsp.
#[cfg(windows)]
const FRAME: u8 = 40;
#[cfg(not(windows))]
const FRAME: u8 = 8;

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
}

impl Assembler {
    pub(super) fn new(table: usize) -> Self {
        Self {
            code: Vec::new(),
            fixups: Vec::new(),
            table,
            epilogue_offset: 0,
        }
    }

    pub(super) fn offset(&self) -> usize {
        self.code.len()
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    fn branch_fixup(&mut self, opcode: &[u8], target: FixupTarget) {
        self.bytes(opcode);
        self.fixups.push(Fixup {
            at: self.code.len(),
            target,
        });
        self.bytes(&[0; 4]);
    }

    fn move_immediate(&mut self, rex: u8, register: u8, value: u64) {
        self.bytes(&[rex, 0xB8 + register]); // mov r64, imm64
        self.bytes(&value.to_le_bytes());
    }

    fn test_status(&mut self) {
        self.bytes(&[0x48, 0x85, 0xC0]); // test rax, rax
    }

    fn branch_negative_epilogue(&mut self) {
        self.branch_fixup(&[0x0F, 0x88], FixupTarget::Epilogue); // js
    }

    /// Entered as `function(state, entry)`; jumps into the generated body at
    /// `entry` once the frame and pinned registers are established.
    pub(super) fn prologue(&mut self) {
        let table = self.table as u64;
        self.bytes(&[0x53]); // push rbx
        self.bytes(&[0x41, 0x54]); // push r12
        self.bytes(&[0x48, 0x83, 0xEC, FRAME]); // sub rsp, FRAME
        #[cfg(windows)]
        self.bytes(&[0x48, 0x89, 0xCB]); // mov rbx, rcx
        #[cfg(not(windows))]
        self.bytes(&[0x48, 0x89, 0xFB]); // mov rbx, rdi
        self.move_immediate(0x49, 4, table); // mov r12, table
        #[cfg(windows)]
        self.bytes(&[0xFF, 0xE2]); // jmp rdx
        #[cfg(not(windows))]
        self.bytes(&[0xFF, 0xE6]); // jmp rsi
    }

    pub(super) fn epilogue(&mut self) {
        self.epilogue_offset = self.code.len();
        self.bytes(&[0x48, 0x83, 0xC4, FRAME]); // add rsp, FRAME
        self.bytes(&[0x41, 0x5C]); // pop r12
        self.bytes(&[0x5B]); // pop rbx
        self.bytes(&[0xC3]); // ret
    }

    pub(super) fn call(&mut self, function: *const (), args: &[u64]) {
        #[cfg(windows)]
        const ARGUMENTS: [(u8, u8); 2] = [(0x48, 2), (0x49, 0)]; // rdx, r8
        #[cfg(not(windows))]
        const ARGUMENTS: [(u8, u8); 2] = [(0x48, 6), (0x48, 2)]; // rsi, rdx

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

    /// Emits a ModRM byte and 32-bit displacement addressing `[rbx + disp]`.
    fn memory(&mut self, register: u8, displacement: i32) {
        self.bytes(&[0x83 | (register << 3)]);
        self.bytes(&displacement.to_le_bytes());
    }

    /// Loads an operand's payload into `register` (0 = rax, 1 = rcx).
    /// Register operands are checked for the `Uint64` tag first; a failed
    /// check branches to the pending slow path.
    fn load_operand(
        &mut self,
        operand: FastOperand,
        register: u8,
        checks: &mut Vec<PendingBranch>,
    ) {
        match operand {
            FastOperand::Register(slot) => {
                self.bytes(&[0x0F, 0xB6]); // movzx edx, byte [rbx + slot]
                self.memory(2, slot as i32);
                self.bytes(&[0x80, 0xFA, super::uint64_tag()]); // cmp dl, tag
                checks.push(self.forward(&[0x0F, 0x85])); // jne slow
                self.bytes(&[0x48, 0x8B]); // mov register, [rbx + payload]
                self.memory(register, (slot + PAYLOAD_OFFSET) as i32);
            }
            FastOperand::Immediate(value) => self.move_immediate(0x48, register, value),
        }
    }

    /// Stores the `Uint64` in rax to a register slot: the payload word, then
    /// a whole tag word so the slot's padding stays defined.
    fn store_result(&mut self, destination: usize) {
        self.bytes(&[0x48, 0x89]); // mov [rbx + payload], rax
        self.memory(0, (destination + PAYLOAD_OFFSET) as i32);
        self.bytes(&[0x48, 0xC7]); // mov qword [rbx + slot], tag
        self.memory(0, destination as i32);
        self.bytes(&u32::from(super::uint64_tag()).to_le_bytes());
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

    pub(super) fn binary_fast(
        &mut self,
        kind: FastBinaryOp,
        lhs: FastOperand,
        rhs: FastOperand,
        destination: usize,
        helper: *const (),
        op: u64,
    ) {
        let mut checks = Vec::new();
        self.load_operand(lhs, 0, &mut checks);
        self.load_operand(rhs, 1, &mut checks);
        match kind {
            FastBinaryOp::Add => self.bytes(&[0x48, 0x01, 0xC8]), // add rax, rcx
            FastBinaryOp::Subtract => self.bytes(&[0x48, 0x29, 0xC8]), // sub rax, rcx
            FastBinaryOp::Multiply => self.bytes(&[0x48, 0x0F, 0xAF, 0xC1]), // imul rax, rcx
        }
        self.store_result(destination);
        self.slow_path(checks, |assembler| assembler.call(helper, &[op]));
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
        source: usize,
        target: usize,
        helper: *const (),
        op: u64,
    ) {
        let mut checks = Vec::new();
        self.load_operand(FastOperand::Register(source), 0, &mut checks);
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
        self.load_operand(lhs, 0, &mut checks);
        self.load_operand(rhs, 1, &mut checks);
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
