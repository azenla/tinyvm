use crate::machine::error::MachineError;
use crate::machine::value::MachineValue;
use crate::op::OpArg;

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

    pub fn store(&mut self, arg: OpArg, value: MachineValue) -> crate::machine::error::Result<()> {
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
