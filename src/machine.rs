use crate::machine::error::{MachineError, Result};
use crate::machine::ops::{InlinedOpHandlers, OpHandlerSet};
use crate::machine::registers::RegisterBank;
use crate::program::RawProgram;
use state::MachineState;

pub mod error;
pub mod ops;
pub mod registers;
pub mod state;
pub mod value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineLoopState {
    Next,
    Stay,
    Break,
}

#[derive(Clone)]
pub enum MachineProgram<'program> {
    Uncompiled(&'program RawProgram<'program>),
    Inlined(InlinedOpHandlers<'program>),
}

#[derive(Clone)]
pub struct Machine {
    handlers: OpHandlerSet,
    state: MachineState,
}

impl Machine {
    pub fn new(handlers: OpHandlerSet) -> Machine {
        Self {
            handlers,
            state: MachineState::default(),
        }
    }

    fn step_shared(&mut self, result: Result<MachineLoopState>) -> Result<MachineLoopState> {
        let state = result?;
        if state == MachineLoopState::Next {
            self.state.set_instruction(self.state.instruction() + 1)
        }
        Ok(state)
    }

    pub fn step_uncompiled(&mut self, program: &RawProgram) -> Result<MachineLoopState> {
        let op = program
            .ops()
            .get(self.state.instruction())
            .ok_or(MachineError::InstructionOverflow)?;
        let handler = self
            .handlers
            .get(&op.code)
            .ok_or(MachineError::InvalidOpCode)?;
        let result = handler.perform(&mut self.state, op);
        self.step_shared(result)
    }

    pub fn step_inlined(&mut self, program: &InlinedOpHandlers) -> Result<MachineLoopState> {
        let (op, handler) = program
            .ops()
            .get(self.state.instruction())
            .ok_or(MachineError::InstructionOverflow)?;
        let result = handler.perform(&mut self.state, op);
        self.step_shared(result)
    }

    pub fn step(&mut self, program: &MachineProgram) -> Result<MachineLoopState> {
        match program {
            MachineProgram::Uncompiled(program) => self.step_uncompiled(program),
            MachineProgram::Inlined(program) => self.step_inlined(program),
        }
    }

    pub fn run(&mut self, program: &MachineProgram) -> Result<()> {
        match program {
            MachineProgram::Uncompiled(program) => loop {
                let state = self.step_uncompiled(program)?;
                if state == MachineLoopState::Break {
                    break;
                }
            },

            MachineProgram::Inlined(program) => loop {
                let state = self.step_inlined(program)?;
                if state == MachineLoopState::Break {
                    break;
                }
            },
        }

        Ok(())
    }

    pub fn state(&mut self) -> &MachineState {
        &mut self.state
    }

    pub fn state_mut(&mut self) -> &mut MachineState {
        &mut self.state
    }
}
