use crate::machine::error;
use crate::machine::error::MachineError;
use crate::machine::registers::RegisterBank;
use crate::machine::value::MachineValue;
use crate::op::{Op, OpArg};

#[derive(Clone, Default)]
pub struct MachineState {
    stack: Vec<MachineValue>,
    calls: Vec<MachineValue>,
    bank: RegisterBank,
    instruction: usize,
}

impl MachineState {
    pub fn jmp(&mut self, op: &Op) -> error::Result<()> {
        self.instruction = match op.arg {
            OpArg::Instruction(instruction) => instruction as usize,
            _ => return Err(MachineError::InstructionExpected),
        };
        Ok(())
    }

    pub fn call(&mut self, op: &Op) -> error::Result<()> {
        let current = self.instruction + 1;
        self.jmp(op)?;
        self.calls.push(MachineValue::ReturnAddress(current));
        Ok(())
    }

    pub fn ret(&mut self) -> error::Result<()> {
        let value = self.calls.pop().ok_or(MachineError::StackEmpty)?;
        self.instruction = match value {
            MachineValue::ReturnAddress(value) => value,
            _ => return Err(MachineError::InstructionExpected),
        };
        Ok(())
    }

    pub fn push(&mut self, value: MachineValue) {
        self.stack.push(value);
    }

    pub fn pop(&mut self) -> error::Result<MachineValue> {
        self.stack.pop().ok_or(MachineError::StackEmpty)
    }

    pub fn reset(&mut self) {
        if !self.stack.is_empty() {
            self.stack.clear();
        }

        if !self.calls.is_empty() {
            self.calls.clear();
        }

        self.instruction = 0;
        self.bank.reset();
    }

    pub fn bank(&self) -> &RegisterBank {
        &self.bank
    }

    pub fn bank_mut(&mut self) -> &mut RegisterBank {
        &mut self.bank
    }

    pub fn instruction(&self) -> usize {
        self.instruction
    }

    pub fn set_instruction(&mut self, index: usize) {
        self.instruction = index;
    }
}
