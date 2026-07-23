use super::{FastBinaryOp, FastOperand, PAYLOAD_OFFSET};
use crate::machine::error::{MachineError, Result};

// Register conventions for generated code: x19 holds the machine state
// pointer, x20 the entry table base (both callee-saved), and x16 is the
// intra-procedure scratch register used for helper addresses.
const STATE: u32 = 19;
const TABLE: u32 = 20;
const SCRATCH: u32 = 16;
const ARGUMENTS: [u32; 3] = [0, 1, 2];

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

    fn compare_zero(&mut self) {
        self.word(0xF100_001F); // cmp x0, #0
    }

    fn branch_negative_epilogue(&mut self) {
        self.branch_fixup(0x5400_000B, FixupKind::Imm19, FixupTarget::Epilogue); // b.lt
    }

    /// Entered as `function(state: x0, entry: x1)`; jumps into the generated
    /// body at `entry` once the frame and pinned registers are established.
    pub(super) fn prologue(&mut self) {
        let table = self.table as u64;
        self.word(0xA9BF_7BFD); // stp x29, x30, [sp, #-16]!
        self.word(0x9100_03FD); // mov x29, sp
        self.word(0xA9BF_53F3); // stp x19, x20, [sp, #-16]!
        self.move_register(STATE, 0);
        self.load_immediate(TABLE, table);
        self.word(0xD61F_0020); // br x1
    }

    pub(super) fn epilogue(&mut self) {
        self.epilogue_offset = self.code.len();
        self.word(0xA8C1_53F3); // ldp x19, x20, [sp], #16
        self.word(0xA8C1_7BFD); // ldp x29, x30, [sp], #16
        self.word(0xD65F_03C0); // ret
    }

    pub(super) fn call(&mut self, function: *const (), args: &[u64]) {
        self.move_register(ARGUMENTS[0], STATE);
        for (index, argument) in args.iter().enumerate() {
            self.load_immediate(ARGUMENTS[index + 1], *argument);
        }
        self.load_immediate(SCRATCH, function as u64);
        self.word(0xD63F_0000 | (SCRATCH << 5)); // blr x16
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

    /// Loads an operand's payload into `register` (x0 or x1). Register
    /// operands are checked for the `Uint64` tag first — a failed check
    /// branches to the pending slow path — while trusted registers were
    /// proven `Uint64` by the optimizer and skip the check.
    fn load_operand(
        &mut self,
        operand: FastOperand,
        register: u32,
        checks: &mut Vec<PendingBranch>,
    ) {
        match operand {
            FastOperand::Register(slot) => {
                let tag = u32::from(super::uint64_tag());
                let payload = ((slot + PAYLOAD_OFFSET) / 8) as u32;
                self.word(0x3940_0000 | ((slot as u32) << 10) | (STATE << 5) | 2); // ldrb w2, [x19, #slot]
                self.word(0x7100_001F | (tag << 10) | (2 << 5)); // cmp w2, #tag
                checks.push(self.forward(0x5400_0001, FixupKind::Imm19)); // b.ne slow
                self.word(0xF940_0000 | (payload << 10) | (STATE << 5) | register); // ldr
            }
            FastOperand::TrustedRegister(slot) => {
                let payload = ((slot + PAYLOAD_OFFSET) / 8) as u32;
                self.word(0xF940_0000 | (payload << 10) | (STATE << 5) | register); // ldr
            }
            FastOperand::Immediate(value) => self.load_immediate(register, value),
        }
    }

    /// Stores the `Uint64` in x0 to a register slot: the payload word, then a
    /// whole tag word so the slot's padding stays defined.
    fn store_result(&mut self, destination: usize) {
        let payload = ((destination + PAYLOAD_OFFSET) / 8) as u32;
        self.word(0xF900_0000 | (payload << 10) | (STATE << 5)); // str x0, [x19, #payload]
        self.load_immediate(1, u64::from(super::uint64_tag()));
        self.word(0xF900_0000 | (((destination / 8) as u32) << 10) | (STATE << 5) | 1); // str x1, [x19, #slot]
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
        self.word(match kind {
            FastBinaryOp::Add => 0x8B01_0000,      // add x0, x0, x1
            FastBinaryOp::Subtract => 0xCB01_0000, // sub x0, x0, x1
            FastBinaryOp::Multiply => 0x9B01_7C00, // mul x0, x0, x1
        });
        self.store_result(destination);
        self.slow_path(checks, |assembler| assembler.call(helper, &[op]));
    }

    pub(super) fn copy_slot(&mut self, source: usize, destination: usize) {
        self.word(0xA940_0400 | (((source / 8) as u32) << 15) | (STATE << 5)); // ldp x0, x1, [x19, #source]
        self.word(0xA900_0400 | (((destination / 8) as u32) << 15) | (STATE << 5)); // stp x0, x1, [x19, #destination]
    }

    pub(super) fn copy_constant(&mut self, constant: usize, destination: usize) {
        self.load_immediate(SCRATCH, constant as u64);
        self.word(0xA940_0400 | (SCRATCH << 5)); // ldp x0, x1, [x16]
        self.word(0xA900_0400 | (((destination / 8) as u32) << 15) | (STATE << 5)); // stp x0, x1, [x19, #destination]
    }

    pub(super) fn jump_if_zero_fast(
        &mut self,
        source: FastOperand,
        target: usize,
        helper: *const (),
        op: u64,
    ) {
        let mut checks = Vec::new();
        self.load_operand(source, 0, &mut checks);
        self.branch_fixup(0xB400_0000, FixupKind::Imm19, FixupTarget::Op(target)); // cbz x0
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
        self.word(0xEB01_001F); // cmp x0, x1
        self.branch_fixup(0x5400_0000, FixupKind::Imm19, FixupTarget::Op(target)); // b.eq
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
