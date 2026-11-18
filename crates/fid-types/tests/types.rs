//! Integration test of the shared vocabulary. Expected values are hand-labelled from
//! CONSTITUTION §3 (side-effect table, command IDs) and §4 (determinism classes), and
//! the FIPS 180-2 SHA-256 test vector for "abc".
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_types::{CommandId, CommandName, Determinism, Digest, SideEffect, TypesError};
use std::collections::BTreeSet;

#[test]
fn constitution_wire_strings() {
    let det: Vec<String> = Determinism::ALL.iter().map(ToString::to_string).collect();
    assert_eq!(det, ["D0", "D1", "D2", "D3", "N"]);
    let side: Vec<String> = SideEffect::ALL.iter().map(ToString::to_string).collect();
    assert_eq!(side, ["R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"]);
    // Ord follows declaration order, so sets iterate in the constitution's order.
    let set: BTreeSet<SideEffect> = SideEffect::ALL.iter().rev().copied().collect();
    assert_eq!(set.into_iter().collect::<Vec<_>>(), SideEffect::ALL);
    assert_eq!("D3".parse::<Determinism>().unwrap(), Determinism::D3);
    assert_eq!("M1".parse::<SideEffect>().unwrap(), SideEffect::M1);
}

#[test]
fn errors_have_stable_codes() {
    let e = Determinism::parse("D9").unwrap_err();
    assert_eq!((e.code(), e.input()), ("bad_determinism", "D9"));
    assert_eq!(e.to_string(), "bad_determinism: \"D9\"");
    assert_eq!(SideEffect::parse("w2").unwrap_err().code(), "bad_side_effect");
    assert_eq!(Digest::parse("sha256:00").unwrap_err().code(), "bad_digest");
    assert_eq!(CommandName::parse("Align").unwrap_err().code(), "bad_command_name");
    assert_eq!(CommandId::parse("align").unwrap_err().code(), "bad_command_id");
    // Inputs kept in errors are bounded.
    let huge = "x".repeat(10_000);
    assert!(Digest::parse(&huge).unwrap_err().input().len() <= 128);
    let _: &dyn std::error::Error = &TypesError::BadDigest(String::new());
}

#[test]
fn sha256_abc_vector() {
    let bytes: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23, 0xb0, 0x03,
        0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
    ];
    let wire = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let d = Digest::from_bytes(bytes);
    assert_eq!(d.as_str(), wire);
    assert_eq!(Digest::parse(wire).unwrap(), d);
    assert_eq!(d.to_bytes(), bytes);
    assert_eq!(Digest::from(bytes).to_string(), wire);
    assert_eq!(format!("{d:?}"), format!("Digest({wire:?})"));
}

#[test]
fn tombstone() {
    let t = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    assert_eq!(Digest::TOMBSTONE.as_str(), t);
    assert_eq!(Digest::parse(t).unwrap(), Digest::TOMBSTONE);
    assert!(Digest::parse(t).unwrap().is_tombstone());
}

#[test]
fn command_ids_match_the_envelope_rule() {
    // The cases of fid-surface's `command_id_rules` test, plus a few more.
    for ok in ["align.run_321@1", "a@1", "gdt.evaluate_flatness@12", "x@0", "x@99999", "9.a_b@3"] {
        assert!(CommandId::is_valid(ok), "{ok}");
    }
    for bad in ["", "@1", "Align.run@1", "align.run", "align.run@", "align.run@x", ".a@1", "a.@1", "a@123456", "a-b@1"] {
        assert!(!CommandId::is_valid(bad), "{bad}");
    }
    let id: CommandId = "egress.send@1".parse().unwrap();
    assert_eq!((id.name(), id.major()), ("egress.send", 1));
    let name = CommandName::parse("egress.send").unwrap();
    assert_eq!(CommandId::new(&name, 1).unwrap(), id);
}

#[test]
fn schema_patterns() {
    assert_eq!(CommandName::PATTERN, "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?$");
    assert_eq!(CommandId::PATTERN, "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$");
    assert_eq!(Digest::PATTERN, "^sha256:[0-9a-f]{64}$");
}
