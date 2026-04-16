use crate::machine::error::{MachineError, Result};
use crate::machine::value::MachineValue;
use crate::op::{Op, OpArg};
use crate::program::Program;

pub mod error;
pub mod ops;
pub mod value;

const REGISTER_BANK_COUNT: usize = 9;

#[derive(PartialEq, Clone, Copy, Debug, Default)]
pub struct RegisterBank {
    registers: [MachineValue; REGISTER_BANK_COUNT],
}

impl RegisterBank {
    pub fn new() -> RegisterBank {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.registers = [MachineValue::None; REGISTER_BANK_COUNT];
    }

    pub fn load(&self, arg: OpArg) -> Option<MachineValue> {
        match arg {
            OpArg::Register1 => Some(self.registers[0]),
            OpArg::Register2 => Some(self.registers[1]),
            OpArg::Register3 => Some(self.registers[2]),
            OpArg::Register4 => Some(self.registers[3]),
            OpArg::Register5 => Some(self.registers[4]),
            OpArg::Register6 => Some(self.registers[5]),
            OpArg::Register7 => Some(self.registers[6]),
            OpArg::Register8 => Some(self.registers[7]),
            OpArg::Register9 => Some(self.registers[8]),
            _ => None,
        }
    }

    pub fn store(&mut self, arg: OpArg, value: MachineValue) -> Result<()> {
        match arg {
            OpArg::Register1 => self.registers[0] = value,
            OpArg::Register2 => self.registers[1] = value,
            OpArg::Register3 => self.registers[2] = value,
            OpArg::Register4 => self.registers[3] = value,
            OpArg::Register5 => self.registers[4] = value,
            OpArg::Register6 => self.registers[5] = value,
            OpArg::Register7 => self.registers[6] = value,
            OpArg::Register8 => self.registers[7] = value,
            OpArg::Register9 => self.registers[8] = value,
            _ => return Err(MachineError::RegisterExpected),
        }
        Ok(())
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct Machine<'program> {
    program: &'program Program<'program>,
    stack: Vec<MachineValue>,
    calls: Vec<MachineValue>,
    bank: RegisterBank,
    current: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineLoopState {
    Next,
    Stay,
    Break,
}

impl<'program> Machine<'program> {
    pub fn new(program: &'program Program) -> Machine<'program> {
        Self {
            program,
            stack: Vec::new(),
            calls: Vec::new(),
            bank: RegisterBank::new(),
            current: 0,
        }
    }

    fn jmp(&mut self, op: &Op) -> Result<()> {
        self.current = match op.arg {
            OpArg::Instruction(instruction) => instruction as usize,
            _ => return Err(MachineError::InstructionExpected),
        };
        Ok(())
    }

    fn call(&mut self, op: &Op) -> Result<()> {
        let current = self.current + 1;
        self.jmp(op)?;
        self.calls.push(MachineValue::ReturnAddress(current));
        Ok(())
    }

    fn ret(&mut self) -> Result<()> {
        let value = self.calls.pop().ok_or(MachineError::StackEmpty)?;
        self.current = match value {
            MachineValue::ReturnAddress(value) => value,
            _ => return Err(MachineError::InstructionExpected),
        };
        Ok(())
    }

    pub fn step(&mut self) -> Result<MachineLoopState> {
        let op = self
            .program
            .ops()
            .get(self.current)
            .ok_or(MachineError::InstructionOverflow)?;
        let state = ops::perform(self, op)?;
        if state == MachineLoopState::Next {
            self.current += 1;
        }
        Ok(state)
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            let state = self.step()?;
            if state == MachineLoopState::Break {
                break;
            }
        }
        Ok(())
    }

    pub fn push(&mut self, value: MachineValue) {
        self.stack.push(value);
    }

    pub fn pop(&mut self) -> Result<MachineValue> {
        self.stack.pop().ok_or(MachineError::StackEmpty)
    }

    pub fn reset(&mut self) {
        if !self.stack.is_empty() {
            self.stack.clear();
        }

        if !self.calls.is_empty() {
            self.calls.clear();
        }

        self.current = 0;
        self.bank.reset();
    }

    pub fn bank(&self) -> &RegisterBank {
        &self.bank
    }
}
