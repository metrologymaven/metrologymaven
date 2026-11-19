//! Acceptance criteria of spec node HAL.IDL.BASE.ERR (AC-1 .. AC-5). The records, the
//! behaviour table and the refused inputs under `fixtures/hal/idl/base/err/` are
//! hand-labelled in `make_fixtures.py` from the leaf's rules; nothing there comes from this
//! crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_hal_idl::base::error::{MAX_INPUT_BYTES, MAX_MESSAGE_BYTES, SCHEMA_TAG};
use fid_hal_idl::{
    Behaviour, ConfigErrorCode, DeviceError, DeviceText, ErrorCode, OperatorHintId, Severity, UntrustedText,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hal/idl/base/err")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn objects(name: &str) -> Vec<BTreeMap<String, CValue>> {
    let CValue::Arr(items) = fid_canon::parse(&read(name)).unwrap() else { panic!("{name}: not an array") };
    items
        .into_iter()
        .map(|c| match c {
            CValue::Obj(o) => o,
            _ => panic!("{name}: item is not an object"),
        })
        .collect()
}

fn str_of<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

/// The record lines of `all_codes.json`: every line between `[` and `]`, without its comma.
fn record_lines() -> Vec<Vec<u8>> {
    let text = String::from_utf8(read("all_codes.json")).unwrap();
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines.first(), Some(&"["));
    assert_eq!(&lines[lines.len() - 2..], &["]", ""]);
    lines[1..lines.len() - 2].iter().map(|l| l.strip_suffix(',').unwrap_or(*l).as_bytes().to_vec()).collect()
}

#[test]
fn ac1_all_codes_parse_and_round_trip_byte_identically() {
    let lines = record_lines();
    assert_eq!(lines.len(), 11);
    // The file as a whole is also valid JSON.
    let CValue::Arr(items) = fid_canon::parse(&read("all_codes.json")).unwrap() else { panic!() };
    assert_eq!(items.len(), 11);

    let mut codes = Vec::new();
    for line in &lines {
        let e = DeviceError::from_json(line).unwrap_or_else(|err| panic!("{err}: {}", String::from_utf8_lossy(line)));
        assert_eq!(&e.to_canonical_json().unwrap(), line, "{}", String::from_utf8_lossy(line));
        // Parse again from our own output: still the same value.
        assert_eq!(DeviceError::from_json(&e.to_canonical_json().unwrap()).unwrap(), e);
        codes.push(e.code);
    }
    // One record per code, in table order.
    assert_eq!(codes, ErrorCode::ALL.to_vec());
}

#[test]
fn ac1_in_memory_value_round_trips() {
    // `to_value` writes the device code as an in-memory `I64`; `from_value` must take it back.
    for line in record_lines() {
        let e = DeviceError::from_json(&line).unwrap();
        assert_eq!(DeviceError::from_value(e.to_value()).unwrap(), e, "{}", String::from_utf8_lossy(&line));
    }
}

#[test]
fn ac1_hand_labelled_field_values() {
    let lines = record_lines();
    let parse = |i: usize| DeviceError::from_json(&lines[i]).unwrap();
    let t = parse(0);
    assert_eq!(t.device_code, Some(-17));
    assert_eq!(t.hint.as_ref().map(OperatorHintId::as_str), Some("hal.transient.001"));
    assert!(!t.message.truncated());
    assert_eq!(parse(4).message.text().len(), 0);
    assert_eq!(parse(5).device_code, Some(i64::MAX));
    assert_eq!(parse(6).device_code, Some(i64::MIN));
    assert_eq!(parse(6).message.text().as_display(), "X+ soft limit\\u{a}reached");
    // A short message marked truncated by the device is kept as sent.
    let u = parse(9);
    assert!(u.message.truncated());
    assert_eq!(u.message.text().as_display(), "C:\\\\probe\\u{9}mode \\u{1f} off");
    assert_eq!(parse(2).message.text().len(), "Ungültiger Parameter: Abtastrate".len());
}

#[test]
fn ac2_unknown_code_fails() {
    assert_eq!(ErrorCode::parse("OVERHEAT").unwrap_err().code(), "error_code_unknown");
    let doc = br#"{"code":"OVERHEAT","device_code":null,"hint":null,"message":{"bytes":"","truncated":false},"schema":"device-error/1"}"#;
    let err = DeviceError::from_json(doc).unwrap_err();
    assert_eq!((err.code(), &err.field), ("error_code_unknown", &UntrustedText::new("code")));
}

/// Exhaustive on purpose: adding a variant to `ErrorCode` without a row here fails the build.
fn design_row(c: ErrorCode) -> (&'static str, Severity, bool, Behaviour) {
    match c {
        ErrorCode::Transient => ("TRANSIENT", Severity::Warning, true, Behaviour::Retry),
        ErrorCode::CommLost => ("COMM_LOST", Severity::Error, false, Behaviour::Interrupt),
        ErrorCode::ConfigInvalid => ("CONFIG_INVALID", Severity::Error, false, Behaviour::Fail),
        ErrorCode::OutOfRange => ("OUT_OF_RANGE", Severity::Error, false, Behaviour::Fail),
        ErrorCode::NotQualified => ("NOT_QUALIFIED", Severity::Error, false, Behaviour::Fail),
        ErrorCode::CalibrationExpired => ("CALIBRATION_EXPIRED", Severity::Warning, false, Behaviour::Fail),
        ErrorCode::Limit => ("LIMIT", Severity::Error, false, Behaviour::StopStreams),
        ErrorCode::SafetyStopObserved => ("SAFETY_STOP_OBSERVED", Severity::Critical, false, Behaviour::StopStreams),
        ErrorCode::VendorLicense => ("VENDOR_LICENSE", Severity::Error, false, Behaviour::Fail),
        ErrorCode::Unsupported => ("UNSUPPORTED", Severity::Error, false, Behaviour::Fail),
        ErrorCode::Internal => ("INTERNAL", Severity::Critical, false, Behaviour::Fault),
    }
}

#[test]
fn ac3_behaviour_table_is_exhaustive_and_matches_design() {
    for c in ErrorCode::ALL {
        let (name, severity, retryable, behaviour) = design_row(c);
        assert_eq!(c.as_str(), name);
        assert_eq!(c.severity(), severity, "{name}");
        assert_eq!(c.retryable(), retryable, "{name}");
        assert_eq!(c.behaviour(), behaviour, "{name}");
        assert!(!c.semantics().is_empty());
        let e = DeviceError::new(c, None, "", None);
        assert_eq!((e.severity(), e.retryable(), e.behaviour()), (severity, retryable, behaviour));
    }
    // The hand-transcribed table file agrees row for row, in order.
    let rows = objects("table.json");
    assert_eq!(rows.len(), ErrorCode::ALL.len());
    for (row, c) in rows.iter().zip(ErrorCode::ALL) {
        assert_eq!(str_of(row, "code"), c.as_str());
        assert_eq!(str_of(row, "severity"), c.severity().as_str());
        assert_eq!(row.get("retryable"), Some(&CValue::Bool(c.retryable())));
        assert_eq!(str_of(row, "behaviour"), c.behaviour().as_str());
    }
}

#[test]
fn ac4_long_message_is_truncated_on_a_utf8_boundary() {
    // 2 + 3332 * 3 + 2 = 10 000 bytes. Cut: 2 + 1364 * 3 = 4094 bytes (4095 and 4096 fall
    // inside a three-byte char).
    let long = format!("ab{}cc", "€".repeat(3332));
    assert_eq!(long.len(), 10_000);
    let t = DeviceText::new(&long);
    assert!(t.truncated());
    assert_eq!(t.text().len(), 4094);
    assert_eq!(t.text(), &UntrustedText::new(&format!("ab{}", "€".repeat(1364))));

    // The same through the wire: refused never, cut and marked.
    let mut doc = BTreeMap::new();
    doc.insert("code".to_string(), CValue::Str("TRANSIENT".into()));
    doc.insert("device_code".to_string(), CValue::Null);
    doc.insert("hint".to_string(), CValue::Null);
    let mut msg = BTreeMap::new();
    msg.insert("bytes".to_string(), CValue::Str(long.clone()));
    msg.insert("truncated".to_string(), CValue::Bool(false));
    doc.insert("message".to_string(), CValue::Obj(msg));
    doc.insert("schema".to_string(), CValue::Str(SCHEMA_TAG.into()));
    let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(doc)).unwrap();
    assert!(bytes.len() <= MAX_INPUT_BYTES);
    let e = DeviceError::from_json(&bytes).unwrap();
    assert!(e.message.truncated());
    assert_eq!(e.message.text().len(), 4094);
    // After the cut the record round-trips byte-identically.
    let out = e.to_canonical_json().unwrap();
    assert_eq!(DeviceError::from_json(&out).unwrap().to_canonical_json().unwrap(), out);

    // Exactly the limit is kept whole.
    let edge = DeviceText::new(&"a".repeat(MAX_MESSAGE_BYTES));
    assert!(!edge.truncated());
    assert_eq!(edge.text().len(), MAX_MESSAGE_BYTES);
}

/// A document of exactly `len` bytes whose message is `len - overhead` ASCII bytes.
fn document_of_len(len: usize) -> Vec<u8> {
    let head = br#"{"code":"TRANSIENT","device_code":null,"hint":null,"message":{"bytes":""#;
    let tail = br#"","truncated":false},"schema":"device-error/1"}"#;
    let mut doc = head.to_vec();
    doc.resize(len - tail.len(), b'a');
    doc.extend_from_slice(tail);
    assert_eq!(doc.len(), len);
    doc
}

#[test]
fn whole_document_over_the_input_limit_is_refused() {
    // At the limit: parsed, message cut and marked.
    let e = DeviceError::from_json(&document_of_len(MAX_INPUT_BYTES)).unwrap();
    assert!(e.message.truncated());
    assert_eq!(e.message.text().len(), MAX_MESSAGE_BYTES);
    // One byte over: refused before any field is read.
    let err = DeviceError::from_json(&document_of_len(MAX_INPUT_BYTES + 1)).unwrap_err();
    assert_eq!((err.code(), &err.field), ("device_error_invalid", &UntrustedText::new("")));
}

#[test]
fn ac5_free_text_hint_fails() {
    assert_eq!(OperatorHintId::parse("see manual page 4").unwrap_err().code(), "hint_id_invalid");
    assert_eq!(OperatorHintId::parse("hal.limit.010").unwrap().as_str(), "hal.limit.010");
}

#[test]
fn refused_inputs_fail_with_the_labelled_code_and_field() {
    let cases = objects("cases.json");
    assert!(cases.len() >= 30);
    for case in &cases {
        let name = str_of(case, "name");
        let err = DeviceError::from_json(str_of(case, "input").as_bytes())
            .err()
            .unwrap_or_else(|| panic!("{name}: accepted"));
        assert_eq!(err.code(), str_of(case, "error"), "{name}");
        assert_eq!(err.field, UntrustedText::new(str_of(case, "field")), "{name}");
    }
}

#[test]
fn ordering_is_deterministic() {
    let mut all: Vec<DeviceError> = record_lines().iter().map(|l| DeviceError::from_json(l).unwrap()).collect();
    all.reverse();
    all.push(DeviceError::new(ErrorCode::Transient, None, "z", None));
    all.push(DeviceError::new(ErrorCode::Transient, Some(-18), "z", None));
    all.sort();
    let order: Vec<(ErrorCode, Option<i64>)> = all.iter().map(|e| (e.code, e.device_code)).collect();
    let first = [(ErrorCode::Transient, None), (ErrorCode::Transient, Some(-18)), (ErrorCode::Transient, Some(-17))];
    assert_eq!(&order[..3], &first);
    assert_eq!(all.last().map(|e| e.code), Some(ErrorCode::Internal));
    assert!(Severity::Info < Severity::Warning && Severity::Error < Severity::Critical);
}

#[test]
fn config_codes_map_into_the_closed_set() {
    assert_eq!(ErrorCode::from(ConfigErrorCode::ConfigInvalid), ErrorCode::ConfigInvalid);
    assert_eq!(ErrorCode::from(ConfigErrorCode::OutOfRange), ErrorCode::OutOfRange);
    assert_eq!(ConfigErrorCode::OutOfRange.as_str(), ErrorCode::OutOfRange.as_str());
}

#[test]
fn hostile_inputs_do_not_panic() {
    let big = vec![b' '; MAX_INPUT_BYTES + 1];
    assert_eq!(DeviceError::from_json(&big).unwrap_err().code(), "device_error_invalid");
    let deep = b"{\"code\":[[[[[[]]]]]]}";
    assert_eq!(DeviceError::from_json(deep).unwrap_err().code(), "device_error_invalid");
    for bad in [&b"\xff"[..], &b""[..], &b"null"[..], &b"\"TRANSIENT\""[..]] {
        assert_eq!(DeviceError::from_json(bad).unwrap_err().code(), "device_error_invalid");
    }
}
