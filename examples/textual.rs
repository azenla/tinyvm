use std::str::FromStr;
use tinyvm::op::textual::TextualParseError;
use tinyvm::program::RawProgram;

const FIB_PROGRAM_TEXT: &str = "\
# Pop the input value into r3.
pop r3
# fib(0) = 0: store in r1.
push 0u64
pop r1
# fib(1) = 1: store in r2.
push 1u64
pop r2
# Pushes the counter-value. (loop start: instruction 5)
push r3
# Exit loop if counter-value == 0.
jiz 20p
# Calculate next fibonacci: next = r1 + r2
push r1
push r2
add
# Store result in r4.
pop r4
# Shift values in registers: r1 => r2, r2 => next
push r2
pop r1
push r4
pop r2
# Decrement the counter-value: r3 = r3 - 1
push r3
push 1u64
sub
pop r3
# Jump back to the loop start.
jmp 5p
# Push the result to the stack.
push r1
# Exit.
exit
";

fn main() -> Result<(), TextualParseError> {
    let program = RawProgram::from_str(FIB_PROGRAM_TEXT)?;
    println!("{:?}", program);
    Ok(())
}
