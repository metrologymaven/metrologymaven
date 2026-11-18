//! Acceptance criteria of spec node AUTO.SURF.ENV (AC-1 .. AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{to_canonical_bytes, CValue};
use fid_surface::{envelope_schema, error_codes, Envelope, EnvelopeError, ErrorCode, Outcome};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures(sub: &str) -> Vec<(String, Vec<u8>)> {
    let dir = root().join("fixtures/envelope").join(sub);
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect();
    out.sort();
    out
}

/// AC-1: every valid fixture round-trips byte for byte, one per outcome.
#[test]
fn ac1_valid_round_trip() {
    let valid = fixtures("valid");
    let mut seen = Vec::new();
    for (name, bytes) in &valid {
        let env = Envelope::decode(bytes).unwrap_or_else(|e| panic!("{name}: decode failed: {e}"));
        env.validate().unwrap_or_else(|e| panic!("{name}: invalid: {e}"));
        let again = env.encode().unwrap();
        assert_eq!(String::from_utf8(again).unwrap(), String::from_utf8(bytes.clone()).unwrap(), "{name}");
        seen.push(env.outcome.as_str().to_string());
    }
    for o in Outcome::KNOWN {
        assert!(seen.iter().any(|s| s == o.as_str()), "no valid fixture for outcome {o}");
    }
}

/// AC-2, AC-3 and the other named validation failures.
#[test]
fn ac2_ac3_invalid_fixtures() {
    let want = [
        ("accepted-no-job", "missing_job_id"),
        ("committed-no-hash", "missing_journal_entry_hash"),
        ("failed-no-error", "missing_error"),
        ("hash-on-failed", "unexpected_journal_entry_hash"),
    ];
    let invalid = fixtures("invalid");
    assert_eq!(invalid.len(), want.len());
    for ((name, bytes), (want_name, want_code)) in invalid.iter().zip(want) {
        assert_eq!(name, want_name);
        let env = Envelope::decode(bytes).unwrap();
        assert_eq!(env.validate().unwrap_err().code(), want_code, "{name}");
        assert_eq!(env.encode().unwrap_err().code(), want_code, "{name}: encode must refuse too");
    }
}

/// AC-4: unknown outcome and error-code strings decode without failing.
#[test]
fn ac4_unknown_strings_decode() {
    let env = Envelope::decode(br#"{"command_id":"x.y@1","envelope_version":1,"outcome":"teleported"}"#).unwrap();
    assert_eq!(env.outcome, Outcome::Unknown("teleported".into()));
    assert_eq!(env.validate(), Err(EnvelopeError::UnknownOutcome("teleported".into())));

    let env = Envelope::decode(
        br#"{"command_id":"x.y@1","envelope_version":1,"error":{"code":"warp_core_breach","message":"m"},"outcome":"failed"}"#,
    )
    .unwrap();
    assert_eq!(env.error.as_ref().unwrap().code, ErrorCode::Unknown("warp_core_breach".into()));
}

fn check_generated(rel: &str, value: &CValue) {
    let mut want = to_canonical_bytes(value).unwrap();
    want.push(b'\n');
    let path = root().join(rel);
    let have = std::fs::read(&path).unwrap_or_default();
    assert!(have == want, "{rel} is out of date. Replace its contents with:\n{}", String::from_utf8_lossy(&want));
}

/// REQ-005 / AC-1: the checked-in schema is generated from the Rust types.
#[test]
fn schema_is_generated() {
    check_generated("schema/envelope-v1.json", &envelope_schema());
}

/// AC-5: the error-code list is generated, complete, unique and in byte order.
#[test]
fn ac5_error_codes_generated() {
    check_generated("schema/error-codes.json", &error_codes());
    let mut codes: Vec<&str> = ErrorCode::KNOWN.iter().map(ErrorCode::as_str).collect();
    let n = codes.len();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), n, "duplicate error code");
}

#[test]
fn decode_rejects_bad_shapes_and_limits() {
    let cases: [(&[u8], &str); 6] = [
        (br#"[]"#, "bad_shape"),
        (br#"{"command_id":"x@1","envelope_version":2,"outcome":"computed"}"#, "schema_version_unsupported"),
        (br#"{"command_id":"x@1","envelope_version":1,"outcome":"computed","extra":1}"#, "bad_shape"),
        (br#"{"command_id":"x@1","envelope_version":1}"#, "bad_shape"),
        (br#"{"command_id":1,"envelope_version":1,"outcome":"computed"}"#, "bad_shape"),
        (br#"{"a":1,"a":2}"#, "duplicate_key"),
    ];
    for (bytes, code) in cases {
        assert_eq!(Envelope::decode(bytes).unwrap_err().code(), code, "{}", String::from_utf8_lossy(bytes));
    }
    let deep = format!(
        r#"{{"command_id":"x@1","envelope_version":1,"outcome":"computed","result":{}{}}}"#,
        "[".repeat(70),
        "]".repeat(70)
    );
    assert_eq!(Envelope::decode(deep.as_bytes()).unwrap_err().code(), "input_limit");
}

#[test]
fn command_id_rules() {
    for ok in ["align.run_321@1", "a@1", "gdt.evaluate_flatness@12"] {
        assert!(fid_surface::valid_command_id(ok), "{ok}");
    }
    for bad in ["", "@1", "Align.run@1", "align.run", "align.run@", "align.run@x", ".a@1", "a.@1", "a@123456"] {
        assert!(!fid_surface::valid_command_id(bad), "{bad}");
    }
}

#[test]
fn constructors_are_valid() {
    let ok = Envelope::computed("canon.digest@1", CValue::Str("sha256:00".into()));
    assert!(ok.encode().is_ok());
    let failed = Envelope::failed("canon.digest@1", ErrorCode::ValidationError, "bad", None);
    assert!(failed.encode().is_ok());
}
