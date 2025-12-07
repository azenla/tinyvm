use tinyvm::op::OpArg::{Instruction, Register1, Register2, Register3, Register4, Uint64};
use tinyvm::op::OpCode::{Add, Exit, Jump, JumpIfZero, Pop, Push, Subtract};
use tinyvm::program::Program;
use tinyvm::{op, program};

pub fn fib() -> Program {
    program!(
        // Pop the input value into r3.
        op!(Pop, Register3),
        // fib(0) = 0: store in r1.
        op!(Push, Uint64(0)),
        op!(Pop, Register1),
        // fib(1) = 1: store in r2.
        op!(Push, Uint64(1)),
        op!(Pop, Register2),
        // Pushes the counter-value. (loop start: instruction 5)
        op!(Push, Register3),
        // Exit loop if counter-value == 0.
        op!(JumpIfZero, Instruction(20)),
        // Calculate next fibonacci: next = r1 + r2
        op!(Push, Register1),
        op!(Push, Register2),
        op!(Add),
        // Store result in r4.
        op!(Pop, Register4),
        // Shift values in registers: r1 => R2, r2 => next
        op!(Push, Register2),
        op!(Pop, Register1),
        op!(Push, Register4),
        op!(Pop, Register2),
        // Decrement the counter-value: r3 = r3 - 1
        op!(Push, Register3),
        op!(Push, Uint64(1)),
        op!(Subtract),
        op!(Pop, Register3),
        // Jump back to the loop start.
        op!(Jump, Instruction(5)),
        // Push the result to the stack.
        op!(Push, Register2),
        // Exit.
        op!(Exit)
    )
}
