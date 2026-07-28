use crate::op::Op;
use crate::op::textual::TextualParseError;
use crate::program::RawProgram;
use std::str::FromStr;

impl RawProgram<'_> {
    /// Parses a textual program, also reporting the 1-based text line every op
    /// was written on. Blank and comment lines are skipped, so op indices on
    /// their own cannot be traced back to the text; the recorded lines let a
    /// [`SourceMap`](crate::machine::trace::SourceMap) reach past the raw ops
    /// to the program as it was written.
    pub fn parse_with_lines(string: &str) -> Result<(Self, Vec<usize>), TextualParseError> {
        let mut ops = Vec::new();
        let mut lines = Vec::new();
        for (index, line) in string.lines().enumerate() {
            if line.is_empty() {
                continue;
            }

            if line.starts_with("#") {
                continue;
            }

            let op = Op::from_str(line)?;
            ops.push(op);
            lines.push(index + 1);
        }
        Ok((RawProgram::new_owned(ops), lines))
    }
}

impl FromStr for RawProgram<'_> {
    type Err = TextualParseError;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        Self::parse_with_lines(string).map(|(program, _)| program)
    }
}
