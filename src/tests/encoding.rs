use crate::op::{Op, OpArg, OpCode};
use crate::program::RawProgram;
use std::str::FromStr;

/// One representative value per `OpArg` variant, including boundary values for
/// the numeric ones.
fn sample_args() -> Vec<OpArg> {
    vec![
        OpArg::Register1,
        OpArg::Register2,
        OpArg::Register3,
        OpArg::Register4,
        OpArg::Register5,
        OpArg::Register6,
        OpArg::Register7,
        OpArg::Register8,
        OpArg::Register9,
        OpArg::None,
        OpArg::Uint8(0),
        OpArg::Uint8(u8::MAX),
        OpArg::Uint16(0),
        OpArg::Uint16(u16::MAX),
        OpArg::Uint32(0),
        OpArg::Uint32(u32::MAX),
        OpArg::Uint64(0),
        OpArg::Uint64(u64::MAX),
        OpArg::Int8(i8::MIN),
        OpArg::Int8(i8::MAX),
        OpArg::Int16(i16::MIN),
        OpArg::Int16(i16::MAX),
        OpArg::Int32(i32::MIN),
        OpArg::Int32(i32::MAX),
        OpArg::Int64(i64::MIN),
        OpArg::Int64(i64::MAX),
        OpArg::Instruction(0),
        OpArg::Instruction(u64::MAX),
    ]
}

/// Reads the raw `#[repr(u8)]` discriminant byte of an `OpArg`.
///
/// SAFETY: `OpArg` is declared `#[repr(u8)]`, so (per RFC 2195) its
/// discriminant is stored in the first byte regardless of any payload. Reading
/// that byte through the value's pointer is well-defined.
fn raw_discriminant(arg: &OpArg) -> u8 {
    unsafe { *(arg as *const OpArg).cast::<u8>() }
}

#[test]
fn oparg_ids_are_stable() {
    let expected: &[(OpArg, u8)] = &[
        (OpArg::Register1, 0),
        (OpArg::Register2, 1),
        (OpArg::Register3, 2),
        (OpArg::Register4, 3),
        (OpArg::Register5, 4),
        (OpArg::Register6, 5),
        (OpArg::Register7, 6),
        (OpArg::Register8, 7),
        (OpArg::Register9, 8),
        (OpArg::None, 9),
        (OpArg::Uint8(0), 10),
        (OpArg::Uint16(0), 11),
        (OpArg::Uint32(0), 12),
        (OpArg::Uint64(0), 13),
        (OpArg::Int8(0), 14),
        (OpArg::Int16(0), 15),
        (OpArg::Int32(0), 16),
        (OpArg::Int64(0), 17),
        (OpArg::Instruction(0), 18),
    ];
    for (arg, id) in expected {
        assert_eq!(arg.id(), *id, "unexpected id for {arg:?}");
    }
}

#[test]
fn oparg_discriminant_matches_encoded_id() {
    // The in-memory discriminant and the on-the-wire id must agree so that a
    // future `as u8` cast can never silently disagree with encode/decode.
    for arg in sample_args() {
        assert_eq!(
            raw_discriminant(&arg),
            arg.id(),
            "discriminant/id mismatch for {arg:?}"
        );
    }
}

#[test]
fn op_binary_round_trip_every_code_and_arg() {
    for &code in OpCode::ALL {
        for arg in sample_args() {
            let op = Op::new(code, arg);
            let mut buffer = vec![0u8; Op::encoded_len()];
            op.encode(&mut buffer);
            let decoded = Op::decode(&buffer).expect("op should decode");
            assert_eq!(op, decoded, "binary round trip failed for {op:?}");
        }
    }
}

#[test]
fn op_decode_rejects_wrong_length() {
    assert!(Op::decode(&[0u8; 3]).is_none());
    assert!(Op::decode(&[]).is_none());
}

#[test]
fn op_decode_rejects_invalid_opcode() {
    let mut buffer = vec![0u8; Op::encoded_len()];
    buffer[0] = 0xFF;
    assert!(Op::decode(&buffer).is_none());
}

#[test]
fn op_textual_round_trip_every_code_and_arg() {
    for &code in OpCode::ALL {
        for arg in sample_args() {
            let op = Op::new(code, arg);
            let text = op.to_string();
            let parsed =
                Op::from_str(&text).unwrap_or_else(|e| panic!("parse {text:?} failed: {e:?}"));
            assert_eq!(
                op, parsed,
                "textual round trip failed for {op:?} via {text:?}"
            );
        }
    }
}

#[test]
fn textual_parsing_is_case_insensitive() {
    assert_eq!(
        Op::from_str("PUSH 5U64").unwrap(),
        Op::from_str("push 5u64").unwrap()
    );
    assert_eq!(
        Op::from_str("POP R3").unwrap(),
        Op::from_str("pop r3").unwrap()
    );
    assert_eq!(
        Op::from_str("JMP 7P").unwrap(),
        Op::from_str("jmp 7p").unwrap()
    );
}

#[test]
fn textual_parsing_rejects_garbage() {
    assert!(Op::from_str("nope").is_err());
    assert!(Op::from_str("push r99").is_err());
    assert!(Op::from_str("push 5x64").is_err());
    assert!(Op::from_str("push 999u8").is_err());
}

#[test]
fn program_binary_round_trip_all_codes() {
    let ops: Vec<Op> = OpCode::ALL
        .iter()
        .map(|&code| Op::new(code, OpArg::Uint64(0xDEAD_BEEF)))
        .collect();
    let program = RawProgram::new_owned(ops);
    let encoded = program.encode();
    let decoded = RawProgram::decode(&encoded).expect("program should decode");
    assert_eq!(program, decoded);
}

#[test]
fn program_textual_round_trip() {
    let ops = vec![
        Op::new(OpCode::Push, OpArg::Uint64(5)),
        Op::new(OpCode::Pop, OpArg::Register1),
        Op::new(OpCode::Push, OpArg::Register1),
        Op::new(OpCode::Jump, OpArg::Instruction(0)),
        Op::new(OpCode::Exit, OpArg::None),
    ];
    let program = RawProgram::new_owned(ops.clone());
    let text = ops.iter().map(Op::to_string).collect::<Vec<_>>().join("\n");
    let parsed = RawProgram::from_str(&text).expect("program text should parse");
    assert_eq!(program, parsed);
}
