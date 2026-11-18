//! Acceptance criteria of spec node TRUST.CHAIN.CANON: AC-1 (golden vectors),
//! AC-3 (invalid inputs), AC-4 (convention errors) and the entry-hash rule
//! (REQ-TRUST.CHAIN.CANON-003). Expected values come from
//! `fixtures/canon/make_vectors.py`, an independent Python reference.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_trust_canon::{
    canonicalize, entry_hash, parse, parse_with_limits, serialize, CanonError, ConventionRule, InvalidKind, Limits,
    Value, MAX_DEPTH, MAX_INPUT_BYTES,
};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/canon")
}

fn files(dir: &str, ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(fixtures().join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect();
    out.sort();
    out
}

/// AC-1: every `vectors/*.json` canonicalises to its `*.canon` byte for byte.
#[test]
fn ac1_golden_vectors() {
    let inputs = files("vectors", "json");
    assert!(inputs.len() >= 30, "expected the generated vector set, found {}", inputs.len());
    for input in inputs {
        let text = std::fs::read(&input).unwrap();
        let want = std::fs::read(input.with_extension("canon")).unwrap();
        let got = canonicalize(&text).unwrap_or_else(|e| panic!("{}: {e}", input.display()));
        assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(&want), "{}", input.display());
        assert_eq!(got, want, "{}", input.display());
        // Canonical output is a fixed point.
        assert_eq!(canonicalize(&got).unwrap(), want, "{} not idempotent", input.display());
    }
}

/// AC-3: every `bad/*.json` fails with `canon.invalid` and none panics.
#[test]
fn ac3_bad_inputs() {
    let inputs = files("bad", "json");
    assert!(inputs.len() >= 20);
    for input in inputs {
        let text = std::fs::read(&input).unwrap();
        match canonicalize(&text) {
            Err(e) => assert_eq!(e.code(), "canon.invalid", "{}: {e}", input.display()),
            Ok(out) => panic!("{} accepted: {}", input.display(), String::from_utf8_lossy(&out)),
        }
    }
}

fn kind(input: &[u8]) -> InvalidKind {
    match parse(input) {
        Err(CanonError::Invalid { kind, .. }) => kind,
        other => panic!("{:?}: {other:?}", String::from_utf8_lossy(input)),
    }
}

/// AC-3 named cases from REQ-TRUST.CHAIN.CANON-002, with their detail kinds.
#[test]
fn ac3_named_kinds() {
    let bad = fixtures().join("bad");
    let read = |n: &str| std::fs::read(bad.join(n)).unwrap();
    assert_eq!(kind(&read("duplicate_key.json")), InvalidKind::DuplicateKey);
    assert_eq!(kind(&read("duplicate_key_by_escape.json")), InvalidKind::DuplicateKey);
    assert_eq!(kind(&read("lone_high_surrogate.json")), InvalidKind::LoneSurrogate);
    assert_eq!(kind(&read("lone_low_surrogate.json")), InvalidKind::LoneSurrogate);
    assert_eq!(kind(&read("depth_129_arrays.json")), InvalidKind::TooDeep);
    assert_eq!(kind(&read("depth_129_objects.json")), InvalidKind::TooDeep);
    assert_eq!(kind(&read("number_1e400.json")), InvalidKind::NumberRange);
    assert_eq!(kind(&read("number_huge_mantissa.json")), InvalidKind::NumberRange);
    assert_eq!(kind(&read("invalid_utf8.json")), InvalidKind::Utf8);
    assert_eq!(kind(&read("utf8_bom.json")), InvalidKind::Syntax);
}

/// Hostile-input limits: size (64 MiB) and depth (128), without stack overflow.
#[test]
fn hostile_limits() {
    // Exactly at the depth limit parses; one more fails.
    let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(parse(ok.as_bytes()).is_ok());
    // Very deep nesting fails fast at depth 129 instead of recursing.
    let deep = "[".repeat(1_000_000);
    assert_eq!(kind(deep.as_bytes()), InvalidKind::TooDeep);
    let deep_obj = "{\"a\":".repeat(500_000);
    assert_eq!(kind(deep_obj.as_bytes()), InvalidKind::TooDeep);

    // Size: one byte over 64 MiB is refused before any parsing.
    let mut big = vec![b' '; MAX_INPUT_BYTES + 1];
    big[0] = b'0';
    assert_eq!(kind(&big), InvalidKind::TooLarge);
    big.pop();
    assert_eq!(parse(&big), Ok(Value::Number(0.0)));

    // Caller limits are tighter only.
    let tight = Limits { max_bytes: 4, max_depth: 2 };
    assert!(matches!(
        parse_with_limits(b"[1,2]", tight),
        Err(CanonError::Invalid { kind: InvalidKind::TooLarge, .. })
    ));
    assert!(matches!(
        parse_with_limits(b"[[[]]]", Limits { max_bytes: 100, max_depth: 2 }),
        Err(CanonError::Invalid { kind: InvalidKind::TooDeep, .. })
    ));
    let loose = Limits { max_bytes: usize::MAX, max_depth: usize::MAX };
    assert_eq!(parse_with_limits(deep.as_bytes(), loose).map(|_| ()).unwrap_err().code(), "canon.invalid");

    // Many members with a duplicate far apart (sort-based check, not quadratic).
    let mut wide = String::from("{");
    for i in 0..200_000 {
        wide.push_str(&format!("\"k{i}\":{i},"));
    }
    wide.push_str("\"k7\":0}");
    assert_eq!(kind(wide.as_bytes()), InvalidKind::DuplicateKey);
}

/// Constructed values are checked by the serialiser too.
#[test]
fn serialize_rejects_bad_constructed_values() {
    let mut v = Value::Null;
    for _ in 0..=MAX_DEPTH {
        v = Value::Array(vec![v]);
    }
    assert_eq!(serialize(&v).unwrap_err().code(), "canon.invalid");
    let nan = Value::Array(vec![Value::Number(f64::NAN)]);
    assert!(matches!(serialize(&nan), Err(CanonError::Invalid { kind: InvalidKind::NumberRange, .. })));
    let dup = Value::Object(vec![("a".into(), Value::Null), ("a".into(), Value::Bool(true))]);
    assert!(matches!(serialize(&dup), Err(CanonError::Invalid { kind: InvalidKind::DuplicateKey, .. })));
}

fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, x)| x),
        _ => None,
    }
}

fn get_str<'a>(v: &'a Value, key: &str) -> &'a str {
    match get(v, key) {
        Some(Value::String(s)) => s,
        other => panic!("{key}: {other:?}"),
    }
}

fn entry_vectors() -> Vec<Value> {
    let text = std::fs::read(fixtures().join("entry/vectors.json")).unwrap();
    match parse(&text).unwrap() {
        Value::Array(items) => items,
        other => panic!("{other:?}"),
    }
}

/// REQ-TRUST.CHAIN.CANON-003 and AC-4 (with REQ-006): entry hashes match the
/// reference, convention errors name their path.
#[test]
fn entry_hash_vectors() {
    let vectors = entry_vectors();
    assert!(vectors.len() >= 15);
    let mut conventions = 0;
    for v in &vectors {
        let name = get_str(v, "name");
        let header = get_str(v, "header");
        let expect = get(v, "expect").unwrap();
        let got = entry_hash(header.as_bytes());
        match get(expect, "hash") {
            Some(Value::String(h)) => {
                assert_eq!(got.as_ref().map(|d| d.as_str()), Ok(h.as_str()), "{name}");
            }
            _ => {
                let err = got.expect_err(name);
                assert_eq!(err.code(), get_str(expect, "error"), "{name}: {err}");
                if let Some(Value::String(path)) = get(expect, "path") {
                    conventions += 1;
                    match err {
                        CanonError::Convention { path: p, .. } => assert_eq!(&p, path, "{name}"),
                        other => panic!("{name}: {other:?}"),
                    }
                }
            }
        }
    }
    assert!(conventions >= 8);
}

/// AC-4: a raw 64-bit integer in a header names its path.
#[test]
fn ac4_raw_u64_names_path() {
    let h = br#"{"seq":"1","time":{"utc":"t","mono_ns":18446744073709551615,"sync":"x"}}"#;
    assert_eq!(
        entry_hash(h),
        Err(CanonError::Convention { path: "/time/mono_ns".into(), rule: ConventionRule::DecimalString })
    );
    let h = br#"{"seq":"1","counter":18446744073709551615}"#;
    assert_eq!(entry_hash(h), Err(CanonError::Convention { path: "/counter".into(), rule: ConventionRule::RawNumber }));
}
