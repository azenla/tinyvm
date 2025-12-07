use crate::error::{MachineError, Result};
use crate::machine::value::MachineValue;
use crate::op::{Op, OpArg, OpCode};
use crate::program::Program;

pub mod value;

#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub struct RegisterBank {
    registers: [MachineValue; 9],
}

impl RegisterBank {
    pub fn new() -> RegisterBank {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.registers = [MachineValue::None; 9];
    }

    #[inline]
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

    #[inline]
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

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Machine {
    program: Program,
    stack: Vec<MachineValue>,
    calls: Vec<MachineValue>,
    bank: RegisterBank,
    current: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineLoopState {
    Continue,
    Break,
}

impl Machine {
    pub fn new(program: Program) -> Self {
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
        let op = *self
            .program
            .ops()
            .get(self.current)
            .ok_or(MachineError::InstructionOverflow)?;
        match op.code {
            OpCode::Push => {
                let value =
                    MachineValue::of(op.arg, &self.bank).ok_or(MachineError::ValueExpected)?;
                self.stack.push(value);
            }

            OpCode::Pop => {
                let value = self.stack.pop().ok_or(MachineError::StackEmpty)?;
                self.bank.store(op.arg, value)?;
            }

            OpCode::Add | OpCode::Subtract | OpCode::Multiply | OpCode::Divide => {
                let value1 = self.pop()?;
                let value2 = self.pop()?;
                let result = match op.code {
                    OpCode::Add => value2 + value1,
                    OpCode::Subtract => value2 - value1,
                    OpCode::Multiply => value2 * value1,
                    OpCode::Divide => value2 / value1,
                    _ => unreachable!("operation invalid"),
                };
                self.stack.push(result);
            }

            OpCode::JumpIfEqual => {
                let value1 = self.pop()?;
                let value2 = self.pop()?;
                if value1 == value2 {
                    self.jmp(&op)?;
                    return Ok(MachineLoopState::Continue);
                }
            }

            OpCode::Jump => {
                self.jmp(&op)?;
                return Ok(MachineLoopState::Continue);
            }

            OpCode::JumpIfZero => {
                let value = self.stack.pop().ok_or(MachineError::StackEmpty)?;
                if value.as_u64() == 0 {
                    self.jmp(&op)?;
                    return Ok(MachineLoopState::Continue);
                }
            }

            OpCode::Exit => {
                return Ok(MachineLoopState::Break);
            }

            OpCode::Call => {
                self.call(&op)?;
                return Ok(MachineLoopState::Continue);
            }

            OpCode::Return => {
                self.ret()?;
                return Ok(MachineLoopState::Continue);
            }
        }
        self.current += 1;
        Ok(MachineLoopState::Continue)
    }

    pub fn run(&mut self) -> Result<()> {
        while let MachineLoopState::Continue = self.step()? {}
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
}
