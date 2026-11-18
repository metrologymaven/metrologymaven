//! Acceptance criteria of spec node HAL.IDL.STAMP (AC-1 .. AC-9). Expected digests and
//! canonical bytes come from `fixtures/hal/idl/stamp/make_fixtures.py` (an independent
//! stdlib Python reference); expected error codes and pointers are hand-labelled there from
//! the leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::stamp::{
    json_schema, validate, validate_value, CompatStatus, GapReason, Record, RecordKind, StampErrorKind, Trust,
};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/stamp")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn expect_err(name: &str, code: &str, pointer: &str) {
    let e = validate(&read(name)).err().unwrap_or_else(|| panic!("{name} must be rejected"));
    assert_eq!((e.code(), e.pointer.as_str()), (code, pointer), "{name}");
}

/// AC-1: the line-scan batch validates and its canonical digest is the reference digest.
#[test]
fn ac1_valid_linescan_digest() {
    let record = validate(&read("valid_linescan.json")).unwrap();
    let Record::Batch(batch) = &record else { panic!("expected a batch envelope") };
    assert_eq!(batch.stamp.trust, Trust::Declared);
    assert!(!batch.stamp.simulated);
    assert_eq!(batch.gaps[0].reason, GapReason::DeviceDrop);
    assert_eq!(batch.stamp.driver.compat_status, CompatStatus::Pass);
    assert_eq!(batch.stamp.device.model.as_display(), "LS-7 \u{b5}Line \u{3a9}");

    let expected = String::from_utf8(read("valid_linescan.sha256")).unwrap();
    assert_eq!(record.digest().unwrap().as_str(), expected.trim_end());
    assert_eq!(batch.digest().unwrap().as_str(), expected.trim_end());
}

/// AC-2 (D0): the canonical bytes equal the reference bytes byte for byte; this test runs on
/// the Linux, Windows and macOS runners, so the three digests agree.
#[test]
fn ac2_canonical_bytes_match_reference() {
    let record = validate(&read("valid_linescan.json")).unwrap();
    let canonical = record.canonical_bytes().unwrap();
    assert_eq!(canonical, read("valid_linescan.canonical.json"));
    // fid-canon's own canonicalisation of the input agrees, so the typed round trip loses
    // nothing.
    assert_eq!(canonical, fid_canon::canonicalize(&read("valid_linescan.json")).unwrap());
    // The canonical form validates to the same record.
    assert_eq!(validate(&canonical).unwrap(), record);
}

/// AC-3: missing `simulated` is `stamp_field_missing` at `/simulated` (REQ-002).
#[test]
fn ac3_missing_simulated() {
    expect_err("missing_simulated.json", "stamp_field_missing", "/simulated");
}

/// AC-4: `trust: "verified"` is `stamp_enum_invalid` at `/trust`.
#[test]
fn ac4_bad_trust() {
    expect_err("bad_trust.json", "stamp_enum_invalid", "/trust");
}

/// AC-5: an unknown field is `stamp_field_unknown`.
#[test]
fn ac5_unknown_field() {
    expect_err("unknown_field.json", "stamp_field_unknown", "/vendor_note");
}

/// AC-6: `seq: [200, 100]` is `stamp_seq_invalid`.
#[test]
fn ac6_seq_reversed() {
    expect_err("seq_reversed.json", "stamp_seq_invalid", "/seq");
}

/// AC-7: the claim round-trips to identical canonical bytes, and no value-of-record flag
/// exists on the type, in its schema or on the wire.
#[test]
fn ac7_claim_round_trip_without_value_of_record() {
    let input = read("claim_diameter.json");
    let record = validate(&input).unwrap();
    let Record::Claim(claim) = &record else { panic!("expected a claim") };
    assert_eq!(claim.value.value(), 0.0125);
    assert_eq!(claim.canonical_bytes().unwrap(), fid_canon::canonicalize(&input).unwrap());
    assert_eq!(claim.canonical_bytes().unwrap(), read("claim_diameter.canonical.json"));

    // The struct's fields, exhaustively: adding a field breaks this pattern at compile time.
    let fid_hal_idl::ExternalClaim { quantity, value: _, algorithm_source: _, method: _, batch: _ } = claim;
    assert_eq!(quantity, "diameter");

    let schema = json_schema(RecordKind::Claim).unwrap();
    assert!(!schema.contains("\"value_of_record\"") && !schema.contains("of_record\":"));
    let CValue::Obj(wire) = claim.to_value() else { panic!("object expected") };
    assert!(wire.keys().all(|k| !k.contains("record")));
    let mut forged = wire.clone();
    forged.insert("value_of_record".into(), CValue::Bool(true));
    let e = validate_value(CValue::Obj(forged)).unwrap_err();
    assert_eq!((e.kind, e.pointer.as_str()), (StampErrorKind::FieldUnknown, "/value_of_record"));
}

/// AC-8: a v1 record is `schema_version_unsupported`, even when it carries v1-only fields.
#[test]
fn ac8_v1_major() {
    expect_err("v1_major.json", "schema_version_unsupported", "/schema");
}

/// AC-9: the generated JSON Schemas are byte-identical to `schemas/hal/`. Set
/// `FID_UPDATE_SCHEMAS=1` to rewrite the committed files after a deliberate change.
#[test]
fn ac9_schema_drift() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/hal");
    let update = std::env::var_os("FID_UPDATE_SCHEMAS").is_some();
    for kind in RecordKind::ALL {
        let generated = json_schema(kind).unwrap();
        let path = root.join(kind.schema_file());
        if update {
            std::fs::write(&path, &generated).unwrap();
            continue;
        }
        let committed = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
        if committed != generated {
            let line = committed.lines().zip(generated.lines()).position(|(a, b)| a != b);
            panic!(
                "{} drifted at line {:?}; regenerate with FID_UPDATE_SCHEMAS=1.\n--- generated ---\n{generated}",
                kind.schema_file(),
                line.map(|l| l + 1)
            );
        }
    }
}

/// Further rejected records, each with the code and pointer labelled by the reference script.
#[test]
fn invalid_cases_fail_with_labelled_code_and_pointer() {
    let CValue::Arr(cases) = fid_canon::parse(&read("invalid_cases.json")).unwrap() else { panic!("array") };
    assert!(cases.len() >= 60);
    for case in cases {
        let CValue::Obj(mut c) = case else { panic!("object") };
        let text = |c: &mut std::collections::BTreeMap<String, CValue>, k: &str| match c.remove(k) {
            Some(CValue::Str(s)) => s,
            other => panic!("{k}: {other:?}"),
        };
        let name = text(&mut c, "name");
        let code = text(&mut c, "code");
        let pointer = text(&mut c, "pointer");
        let record = c.remove("record").unwrap();
        // Through the byte path too, so the size and depth limits are in play.
        let bytes = fid_canon::to_canonical_bytes(&record).unwrap();
        for result in [validate_value(record), validate(&bytes)] {
            let e = result.err().unwrap_or_else(|| panic!("{name} must be rejected"));
            assert_eq!((e.code(), e.pointer.as_str()), (code.as_str(), pointer.as_str()), "{name}");
        }
    }
}

/// Records built in code fail closed: `canonical_bytes` validates before writing.
#[test]
fn typed_records_are_checked_before_digesting() {
    let Record::Batch(mut batch) = validate(&read("valid_linescan.json")).unwrap() else { panic!("batch") };
    batch.stamp.simulated = true;
    assert!(batch.canonical_bytes().is_ok());
    assert_ne!(batch.digest().unwrap().as_str(), String::from_utf8(read("valid_linescan.sha256")).unwrap().trim_end());

    let mut stamp = batch.stamp.clone();
    stamp.gaps[0].from = stamp.gaps[0].to + 1;
    let e = stamp.canonical_bytes().unwrap_err();
    assert_eq!((e.code(), e.pointer.as_str()), ("stamp_seq_invalid", "/gaps/0"));

    let mut stamp = batch.stamp.clone();
    stamp.capability.features = vec!["b".into(), "a".into()];
    assert_eq!(stamp.digest().unwrap_err().pointer, "/capability/features/1");

    batch.seq.last += 1;
    let e = batch.canonical_bytes().unwrap_err();
    assert_eq!((e.code(), e.pointer.as_str()), ("stamp_batch_mismatch", "/stamp/seq"));

    let mut stamp = batch.stamp.clone();
    stamp.device.make = fid_hal_idl::UntrustedText::new("");
    assert_eq!(stamp.canonical_bytes().unwrap_err().pointer, "/device/make");
}

/// Malformed input is a typed error, never a panic.
#[test]
fn malformed_json_is_typed() {
    let cases: [&[u8]; 5] = [
        b"",
        b"{",
        b"{\"schema\":\"stamp/0\",\"schema\":\"stamp/0\"}",
        b"\xff",
        b"[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]",
    ];
    for bad in cases {
        let e = validate(bad).unwrap_err();
        assert_eq!((e.kind, e.pointer.as_str()), (StampErrorKind::JsonInvalid, ""));
    }
}
