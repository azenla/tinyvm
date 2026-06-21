use crate::program::RawProgram;
use std::str::FromStr;

mod constants;

#[test]
fn textual_decode() {
    let decoded = RawProgram::from_str(constants::FIB_PROGRAM_TEXT).unwrap();
    assert_eq!(decoded, constants::FIB_PROGRAM);
}

#[test]
fn survives_encode_decode() {
    let encoded = constants::FIB_PROGRAM.encode();
    let decoded = RawProgram::decode(&encoded).unwrap();
    assert_eq!(decoded, constants::FIB_PROGRAM);
}
