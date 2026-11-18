//! Acceptance criteria of spec node CORE.JRN.CANON (AC-1, AC-3 .. AC-6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{canonicalize, digest, digest_bytes, parse, to_canonical_bytes, CValue, CanonError};
use std::collections::BTreeMap;

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/canon/vectors.json");

fn field<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("vector field {k} is {other:?}"),
    }
}

/// AC-1: every conformance vector (written by the independent JS runner) matches.
#[test]
fn ac1_conformance_vectors() {
    let text = std::fs::read(VECTORS).expect("read vectors.json");
    let CValue::Arr(vectors) = parse(&text).expect("parse vectors.json") else {
        panic!("vectors.json must be an array");
    };
    assert!(vectors.len() >= 200, "need at least 200 vectors, have {}", vectors.len());
    let mut failures = Vec::new();
    for v in &vectors {
        let CValue::Obj(o) = v else { panic!("vector must be an object") };
        let name = field(o, "name");
        let got = match canonicalize(field(o, "input").as_bytes()) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: error {e}"));
                continue;
            }
        };
        let got_s = String::from_utf8(got.clone()).unwrap();
        if got_s != field(o, "canonical") {
            failures.push(format!("{name}: got {got_s} want {}", field(o, "canonical")));
        } else if digest_bytes(&got).to_string() != field(o, "digest") {
            failures.push(format!("{name}: digest mismatch"));
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), vectors.len(), failures.join("\n"));
}

/// AC-3: bit-significant -0.0 and 0.0 differ.
#[test]
fn ac3_signed_zero_bits() {
    assert_eq!(to_canonical_bytes(&CValue::F64Bits(-0.0)).unwrap(), b"\"0x8000000000000000\"");
    assert_eq!(to_canonical_bytes(&CValue::F64Bits(0.0)).unwrap(), b"\"0x0000000000000000\"");
}

/// AC-4: NaN or infinity anywhere fails with non_finite_number.
#[test]
fn ac4_non_finite_rejected() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let v = CValue::Arr(vec![CValue::Null, CValue::Obj(BTreeMap::from([("x".to_string(), CValue::Num(bad))]))]);
        assert_eq!(to_canonical_bytes(&v), Err(CanonError::NonFiniteNumber));
        assert_eq!(to_canonical_bytes(&CValue::F64Bits(bad)), Err(CanonError::NonFiniteNumber));
    }
    assert_eq!(parse(b"[1e400]").map(|_| ()), Err(CanonError::NonFiniteNumber));
    assert_eq!(CanonError::NonFiniteNumber.code(), "non_finite_number");
}

/// AC-5: duplicate keys are rejected.
#[test]
fn ac5_duplicate_key() {
    assert_eq!(parse(br#"{"a":1,"a":2}"#).map(|_| ()), Err(CanonError::DuplicateKey("a".into())));
    assert_eq!(parse(br#"{"x":{"a":1,"b":2,"a":3}}"#).map(|_| ()).unwrap_err().code(), "duplicate_key");
}

/// AC-6: deep nesting fails closed without stack overflow.
#[test]
fn ac6_depth_limit() {
    let deep = "[".repeat(10_000) + &"]".repeat(10_000);
    assert_eq!(parse(deep.as_bytes()).map(|_| ()), Err(CanonError::InputLimit));
    let ok = "[".repeat(128) + &"]".repeat(128);
    assert!(parse(ok.as_bytes()).is_ok());
    let too_deep = "[".repeat(129) + &"]".repeat(129);
    assert_eq!(parse(too_deep.as_bytes()).map(|_| ()), Err(CanonError::InputLimit));
}

/// REQ-002: 64-bit integers as decimal strings.
#[test]
fn integers_as_strings() {
    let v = CValue::Arr(vec![CValue::I64(i64::MIN), CValue::U64(u64::MAX), CValue::I64(0), CValue::U64(1 << 53)]);
    assert_eq!(
        String::from_utf8(to_canonical_bytes(&v).unwrap()).unwrap(),
        r#"["-9223372036854775808","18446744073709551615","0","9007199254740992"]"#
    );
}

/// REQ-006: digest format.
#[test]
fn digest_format() {
    let d = digest(&CValue::Obj(BTreeMap::new())).unwrap().to_string();
    // SHA-256 of "{}"
    assert_eq!(d, "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
}

#[test]
fn syntax_errors() {
    for bad in [&b""[..], b"[1,]", b"{\"a\"}", b"01", b"1.", b"\"\\x\"", b"[1] 2", b"\"\\ud800\"", b"tru"] {
        assert_eq!(parse(bad).map(|_| ()).unwrap_err().code(), "syntax", "input {:?}", String::from_utf8_lossy(bad));
    }
    assert_eq!(parse(&[0x22, 0xff, 0x22]).map(|_| ()), Err(CanonError::InvalidUtf8));
}

/// The writer is iterative too: a value deeper than the parser allows still serialises.
#[test]
fn writer_handles_deep_values() {
    let mut v = CValue::Null;
    for _ in 0..100_000 {
        v = CValue::Arr(vec![v]);
    }
    let bytes = to_canonical_bytes(&v).unwrap();
    assert_eq!(bytes.len(), 200_004);
    // Dropping a 100k-deep Vec chain recursively could overflow; leak it in the test.
    std::mem::forget(v);
}
