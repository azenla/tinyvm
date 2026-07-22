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
