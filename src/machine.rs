use crate::machine::error::{MachineError, Result};
use crate::machine::intermediate::{IntermediateOp, IntermediateProgram};
#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use crate::machine::jit::JitProgram;
use crate::machine::ops::{InlinedOpHandlers, OpHandlerSet};
use crate::machine::optimizer::{OptimizedProgram, ValueType};
use crate::machine::registers::RegisterBank;
use crate::machine::value::MachineValue;
use crate::op::{Op, OpArg};
use crate::program::RawProgram;

pub mod error;
pub mod intermediate;
#[cfg(all(
    any(unix, windows),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub mod jit;
pub mod ops;
pub mod optimizer;
pub mod registers;
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
    Intermediate(IntermediateProgram),
    Optimized(OptimizedProgram),
    #[cfg(all(
        any(unix, windows),
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    Jit(JitProgram),
}

#[derive(Clone, Default)]
pub struct MachineState {
    stack: Vec<MachineValue>,
    calls: Vec<MachineValue>,
    bank: RegisterBank,
    current: usize,
}

impl MachineState {
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
        let value = self.calls.pop().ok_or(MachineError::CallStackEmpty)?;
        self.current = match value {
            MachineValue::ReturnAddress(value) => value,
            _ => return Err(MachineError::InstructionExpected),
        };
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

    /// The op the machine will execute next.
    pub fn current(&self) -> usize {
        self.current
    }

    /// Optimized programs may declare the types the caller pushes before
    /// running; type-specialized code trusts the declaration, so entering at
    /// instruction zero verifies it against the actual stack. Resuming at
    /// any other program counter skips the check, since execution already
    /// passed it on the way in.
    pub(crate) fn check_inputs(&self, inputs: &[ValueType]) -> Result<()> {
        if self.current != 0 || inputs.is_empty() {
            return Ok(());
        }
        let Some(start) = self.stack.len().checked_sub(inputs.len()) else {
            return Err(MachineError::InputMismatch);
        };
        for (value, expected) in self.stack[start..].iter().zip(inputs) {
            if *expected != ValueType::Unknown && ValueType::of(value) != *expected {
                return Err(MachineError::InputMismatch);
            }
        }
        Ok(())
    }
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
            self.state.current += 1;
        }
        Ok(state)
    }

    pub fn step_uncompiled(&mut self, program: &RawProgram) -> Result<MachineLoopState> {
        let op = program
            .ops()
            .get(self.state.current)
            .ok_or(MachineError::InstructionOverflow)?;
        let handler = self
            .handlers
            .get(&op.code)
            .ok_or(MachineError::InvalidOpCode)?;
        let result = handler(&mut self.state, op);
        self.step_shared(result)
    }

    pub fn step_inlined(&mut self, program: &InlinedOpHandlers) -> Result<MachineLoopState> {
        let (op, handler) = program
            .ops()
            .get(self.state.current)
            .ok_or(MachineError::InstructionOverflow)?;
        let result = handler(&mut self.state, op);
        self.step_shared(result)
    }

    fn step_ops(&mut self, ops: &[IntermediateOp]) -> Result<MachineLoopState> {
        let pc = self.state.current;
        let op = ops.get(pc).ok_or(MachineError::InstructionOverflow)?;
        match op.perform(&mut self.state, pc)? {
            Some(next) => {
                self.state.current = next;
                if next == pc + 1 {
                    Ok(MachineLoopState::Next)
                } else {
                    Ok(MachineLoopState::Stay)
                }
            }
            None => Ok(MachineLoopState::Break),
        }
    }

    pub fn step_intermediate(&mut self, program: &IntermediateProgram) -> Result<MachineLoopState> {
        self.step_ops(program.ops())
    }

    pub fn step_optimized(&mut self, program: &OptimizedProgram) -> Result<MachineLoopState> {
        self.step_ops(program.ops())
    }

    pub fn step(&mut self, program: &MachineProgram) -> Result<MachineLoopState> {
        match program {
            MachineProgram::Uncompiled(program) => self.step_uncompiled(program),
            MachineProgram::Inlined(program) => self.step_inlined(program),
            MachineProgram::Intermediate(program) => self.step_intermediate(program),
            MachineProgram::Optimized(program) => self.step_optimized(program),
            #[cfg(all(
                any(unix, windows),
                any(target_arch = "x86_64", target_arch = "aarch64")
            ))]
            MachineProgram::Jit(_) => Err(MachineError::StepUnsupported),
        }
    }

    fn run_ops(&mut self, ops: &[IntermediateOp]) -> Result<()> {
        let mut pc = self.state.current;
        let result = loop {
            let Some(op) = ops.get(pc) else {
                break Err(MachineError::InstructionOverflow);
            };
            match op.perform(&mut self.state, pc) {
                Ok(Some(next)) => pc = next,
                Ok(None) => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        self.state.current = pc;
        result
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

            MachineProgram::Intermediate(program) => return self.run_ops(program.ops()),

            MachineProgram::Optimized(program) => {
                self.state.check_inputs(program.inputs())?;
                return self.run_ops(program.ops());
            }

            #[cfg(all(
                any(unix, windows),
                any(target_arch = "x86_64", target_arch = "aarch64")
            ))]
            MachineProgram::Jit(program) => return program.run(&mut self.state),
        }

        Ok(())
    }

    pub fn state(&mut self) -> &mut MachineState {
        &mut self.state
    }
}
