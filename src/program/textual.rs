use crate::op::Op;
use crate::op::textual::TextualParseError;
use crate::program::RawProgram;
use std::str::FromStr;

impl FromStr for RawProgram<'_> {
    type Err = TextualParseError;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let mut ops = Vec::new();
        for line in string.lines() {
            if line.is_empty() {
                continue;
            }

            if line.starts_with("#") {
                continue;
            }

            let op = Op::from_str(line)?;
            ops.push(op);
        }
        Ok(RawProgram::new_owned(ops))
    }
}
