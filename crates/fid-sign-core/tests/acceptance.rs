#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! TRUST.SIGN.DSSE acceptance criteria AC-1 to AC-5 against `fixtures/dsse/`. Every
//! expected value comes from `fixtures/dsse/make_fixtures.py` (an independent stdlib
//! Python implementation) or from RFC 8032 / RFC 6979 test data, never from this crate.

use std::path::PathBuf;

use fid_sign_core::{
    pae, sign_envelope, verify, verify_json, Envelope, PublicKey, SigError, Signer, SoftwareSigner,
    IN_TOTO_PAYLOAD_TYPE,
};
use fid_trust_canon::Value;

fn path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/dsse").join(rel)
}

fn bytes(rel: &str) -> Vec<u8> {
    std::fs::read(path(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn text(rel: &str) -> String {
    String::from_utf8(bytes(rel)).unwrap()
}

fn hex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0);
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn records(rel: &str) -> Vec<Vec<(String, Value)>> {
    match fid_trust_canon::parse(&bytes(rel)).unwrap() {
        Value::Array(items) => items
            .into_iter()
            .map(|v| match v {
                Value::Object(m) => m,
                other => panic!("{rel}: not an object: {other:?}"),
            })
            .collect(),
        other => panic!("{rel}: not an array: {other:?}"),
    }
}

fn field<'a>(rec: &'a [(String, Value)], name: &str) -> &'a str {
    match rec.iter().find(|(k, _)| k == name) {
        Some((_, Value::String(s))) => s,
        _ => panic!("missing string field {name}"),
    }
}

const GOLDEN: [(&str, &str); 2] =
    [("golden/ed25519.dsse.json", "ed25519-test"), ("golden/p256.dsse.json", "p256-test")];

// AC-1 ---------------------------------------------------------------------------------

#[test]
fn ac1_pae_vectors_match_byte_for_byte() {
    let recs = records("vectors/pae.json");
    assert!(recs.len() >= 9);
    for rec in &recs {
        let got = pae(&hex(field(rec, "payloadType_hex")), &hex(field(rec, "payload_hex")));
        assert_eq!(got, hex(field(rec, "pae_hex")), "{}", field(rec, "name"));
    }
}

#[test]
fn ac1_pae_worked_example() {
    // Hand-labelled from the PAE rule.
    assert_eq!(
        pae(b"http://example.com/HelloWorld", b"hello world"),
        b"DSSEv1 29 http://example.com/HelloWorld 11 hello world".to_vec()
    );
}

// AC-2 ---------------------------------------------------------------------------------

#[test]
fn ac2_ed25519_golden_is_byte_exact() {
    let signer = SoftwareSigner::from_pkcs8_pem(&text("keys/ed25519-test.pem")).unwrap();
    let statement = bytes("statement.json");
    let env = sign_envelope(&statement, IN_TOTO_PAYLOAD_TYPE, &[&signer]).unwrap();
    assert_eq!(env.payload_type, "application/vnd.in-toto+json");
    let json = env.to_json().unwrap();
    assert_eq!(String::from_utf8(json).unwrap(), text("golden/ed25519.dsse.json"));
}

#[test]
fn ac2_goldens_round_trip_through_the_parser() {
    for name in ["golden/ed25519.dsse.json", "golden/p256.dsse.json", "golden/two-signatures.dsse.json"] {
        let golden = bytes(name);
        let env = Envelope::from_json(&golden).unwrap();
        assert_eq!(env.payload, bytes("statement.json"), "{name}");
        assert_eq!(env.to_json().unwrap(), golden, "{name}");
    }
}

#[test]
fn p256_test_signer_matches_rfc6979_golden() {
    // RFC 6979 nonces make ECDSA deterministic, so the independent Python signer and
    // the RustCrypto signer must agree byte for byte after low-S normalisation.
    let signer = SoftwareSigner::from_pkcs8_pem(&text("keys/p256-test.pem")).unwrap();
    let env = sign_envelope(&bytes("statement.json"), IN_TOTO_PAYLOAD_TYPE, &[&signer]).unwrap();
    assert_eq!(env.to_json().unwrap(), bytes("golden/p256.dsse.json"));
}

#[test]
fn two_signers_in_one_envelope() {
    let p = SoftwareSigner::from_pkcs8_pem(&text("keys/p256-test.pem")).unwrap();
    let e = SoftwareSigner::from_pkcs8_pem(&text("keys/ed25519-test.pem")).unwrap();
    let env = sign_envelope(&bytes("statement.json"), IN_TOTO_PAYLOAD_TYPE, &[&p, &e]).unwrap();
    assert_eq!(env.to_json().unwrap(), bytes("golden/two-signatures.dsse.json"));
}

// AC-3 ---------------------------------------------------------------------------------

#[test]
fn ac3_goldens_verify_and_report_fingerprints() {
    for (golden, key) in GOLDEN {
        let pem = text(&format!("keys/{key}.pub.pem"));
        let expected = text(&format!("keys/{key}.fingerprint"));
        let fp = verify_json(&bytes(golden), &pem).unwrap();
        assert_eq!(fp.as_str(), expected.trim_end(), "{golden}");
        let env = Envelope::from_json(&bytes(golden)).unwrap();
        assert_eq!(env.signatures[0].keyid, expected.trim_end(), "{golden}: keyid");
        let public = PublicKey::from_pem(&pem).unwrap();
        assert_eq!(public.to_pem(), pem, "{key}: PEM round trip");
        let signer = SoftwareSigner::from_pkcs8_pem(&text(&format!("keys/{key}.pem"))).unwrap();
        assert_eq!(signer.public_key(), public, "{key}: private and public fixtures agree");
    }
    let two = bytes("golden/two-signatures.dsse.json");
    for (_, key) in GOLDEN {
        let fp = verify_json(&two, &text(&format!("keys/{key}.pub.pem"))).unwrap();
        assert_eq!(fp.as_str(), text(&format!("keys/{key}.fingerprint")).trim_end());
    }
}

#[test]
fn ac3_signature_from_another_key_is_invalid() {
    let ed = text("keys/ed25519-test.pub.pem");
    let p = text("keys/p256-test.pub.pem");
    assert_eq!(verify_json(&bytes("golden/p256.dsse.json"), &ed), Err(SigError::Invalid));
    assert_eq!(verify_json(&bytes("golden/ed25519.dsse.json"), &p), Err(SigError::Invalid));
}

// AC-4 ---------------------------------------------------------------------------------

#[test]
fn ac4_rsa_is_unsupported_algorithm() {
    let err = verify_json(&bytes("golden/rsa.dsse.json"), &text("keys/rsa-test.pub.pem")).unwrap_err();
    assert_eq!(err, SigError::UnsupportedAlgorithm);
    assert_eq!(err.code(), "sig.unsupported_algorithm");
}

#[test]
fn ac4_high_s_and_non_canonical_der_are_invalid() {
    let pem = text("keys/p256-test.pub.pem");
    for name in
        ["golden/p256-high-s.dsse.json", "golden/p256-nonminimal-int.dsse.json", "golden/p256-longform-len.dsse.json"]
    {
        let err = verify_json(&bytes(name), &pem).unwrap_err();
        assert_eq!(err, SigError::Invalid, "{name}");
        assert_eq!(err.code(), "sig.invalid");
    }
}

#[test]
fn ac4_rfc6979_sample_high_s_rejected_low_s_accepted() {
    for rec in records("vectors/p256-rfc6979.json") {
        let key = PublicKey::p256_from_sec1(&hex(field(&rec, "public_sec1"))).unwrap();
        let msg = hex(field(&rec, "message"));
        assert_eq!(key.verify_raw(&msg, &hex(field(&rec, "low_s_der"))), Ok(()));
        assert_eq!(key.verify_raw(&msg, &hex(field(&rec, "high_s_der"))), Err(SigError::Invalid));
    }
}

#[test]
fn rfc8032_vectors_verify_strictly() {
    for rec in records("vectors/ed25519-rfc8032.json") {
        let key = PublicKey::ed25519_from_bytes(&hex(field(&rec, "public"))).unwrap();
        let msg = hex(field(&rec, "message"));
        let sig = hex(field(&rec, "signature"));
        assert_eq!(key.verify_raw(&msg, &sig), Ok(()), "{}", field(&rec, "name"));
        assert_eq!(key.verify_raw(b"other", &sig), Err(SigError::Invalid));
        let signer = SoftwareSigner::ed25519_from_seed(&hex(field(&rec, "secret")).try_into().unwrap());
        assert_eq!(signer.public_key(), key);
        assert_eq!(signer.sign_pae(&msg).unwrap(), sig);
    }
}

#[test]
fn small_order_ed25519_key_is_rejected() {
    // Hand-labelled: A = identity (y = 1), R = identity, S = 0 satisfies the cofactorless
    // equation [S]B = R + [k]A for every message; strict verification must refuse it.
    let mut identity = [0u8; 32];
    identity[0] = 1;
    let key = PublicKey::ed25519_from_bytes(&identity).unwrap();
    let mut sig = [0u8; 64];
    sig[0] = 1;
    assert_eq!(key.verify_raw(b"anything", &sig), Err(SigError::Invalid));
}

// AC-5 ---------------------------------------------------------------------------------

#[test]
fn ac5_bit_flips_in_decoded_fields_never_verify() {
    let key = PublicKey::from_pem(&text("keys/ed25519-test.pub.pem")).unwrap();
    let golden = Envelope::from_json(&bytes("golden/ed25519.dsse.json")).unwrap();
    assert!(verify(&golden, &key).is_ok());
    let mut checked = 0usize;
    for i in 0..golden.payload.len() {
        for bit in 0..8 {
            let mut env = golden.clone();
            env.payload[i] ^= 1 << bit;
            assert_eq!(verify(&env, &key), Err(SigError::Invalid), "payload byte {i} bit {bit}");
            checked += 1;
        }
    }
    for i in 0..golden.payload_type.len() {
        for bit in 0..8 {
            let mut raw = golden.payload_type.clone().into_bytes();
            raw[i] ^= 1 << bit;
            // A flip that breaks UTF-8 cannot be a payloadType at all.
            if let Ok(payload_type) = String::from_utf8(raw) {
                let env = Envelope { payload_type, ..golden.clone() };
                assert_eq!(verify(&env, &key), Err(SigError::Invalid), "payloadType byte {i} bit {bit}");
                checked += 1;
            }
        }
    }
    for i in 0..golden.signatures[0].sig.len() {
        for bit in 0..8 {
            let mut env = golden.clone();
            env.signatures[0].sig[i] ^= 1 << bit;
            assert_eq!(verify(&env, &key), Err(SigError::Invalid), "sig byte {i} bit {bit}");
            checked += 1;
        }
    }
    assert!(checked > 8 * (300 + 64), "{checked}");
}

#[test]
fn ac5_bit_flips_in_envelope_json_never_verify() {
    let pem = text("keys/ed25519-test.pub.pem");
    let golden = bytes("golden/ed25519.dsse.json");
    let env = Envelope::from_json(&golden).unwrap();
    // Byte ranges of the three string values inside the golden JSON text.
    let mut ranges = Vec::new();
    for value in [
        fid_sign_core::base64_encode(&env.payload),
        env.payload_type.clone(),
        fid_sign_core::base64_encode(&env.signatures[0].sig),
    ] {
        let needle = format!("\"{value}\"");
        let at = golden.windows(needle.len()).position(|w| w == needle.as_bytes()).unwrap();
        ranges.push(at + 1..at + 1 + value.len());
    }
    let (mut invalid, mut malformed) = (0usize, 0usize);
    for range in ranges {
        for i in range {
            for bit in 0..8 {
                let mut json = golden.clone();
                json[i] ^= 1 << bit;
                match verify_json(&json, &pem) {
                    Err(SigError::Invalid) => invalid += 1,
                    Err(SigError::Malformed(_)) => malformed += 1,
                    other => panic!("byte {i} bit {bit}: {other:?}"),
                }
            }
        }
    }
    assert!(invalid > 0 && malformed > 0, "{invalid} {malformed}");
}

// REQ-TRUST.SIGN.DSSE-006 limits ---------------------------------------------------------

fn golden_value() -> Vec<(String, Value)> {
    match fid_trust_canon::parse(&bytes("golden/ed25519.dsse.json")).unwrap() {
        Value::Object(m) => m,
        _ => panic!(),
    }
}

fn with_member(name: &str, value: Option<Value>) -> Vec<u8> {
    let mut m: Vec<(String, Value)> = golden_value().into_iter().filter(|(k, _)| k != name).collect();
    if let Some(v) = value {
        m.push((name.to_owned(), v));
    }
    fid_trust_canon::serialize(&Value::Object(m)).unwrap()
}

fn malformed(json: &[u8]) -> bool {
    matches!(Envelope::from_json(json), Err(SigError::Malformed(_)))
}

#[test]
fn signature_count_limit() {
    let sig = match golden_value().into_iter().find(|(k, _)| k == "signatures") {
        Some((_, Value::Array(a))) => a[0].clone(),
        _ => panic!(),
    };
    let at_limit = with_member("signatures", Some(Value::Array(vec![sig.clone(); 64])));
    let env = Envelope::from_json(&at_limit).unwrap();
    assert_eq!(env.signatures.len(), 64);
    assert!(verify_json(&at_limit, &text("keys/ed25519-test.pub.pem")).is_ok());
    assert!(malformed(&with_member("signatures", Some(Value::Array(vec![sig; 65])))));
    assert!(malformed(&with_member("signatures", Some(Value::Array(Vec::new())))));
    let mut too_many = env.clone();
    too_many.signatures = vec![env.signatures[0].clone(); 65];
    assert!(matches!(too_many.to_json(), Err(SigError::Malformed(_))));
}

#[test]
fn size_limit() {
    let mut big = vec![b' '; fid_sign_core::MAX_ENVELOPE_BYTES + 1];
    big[0] = b'{';
    assert!(malformed(&big));
}

#[test]
fn invalid_base64_and_structure_are_malformed() {
    let s = |v: &str| Some(Value::String(v.to_owned()));
    for bad in ["eyJ", "eyJ=", "e$J9", "eyJ9\n", "eyJ9-_==", "Zh=="] {
        assert!(malformed(&with_member("payload", s(bad))), "payload {bad:?}");
    }
    assert!(malformed(&with_member("payload", None)));
    assert!(malformed(&with_member("payloadType", None)));
    assert!(malformed(&with_member("payloadType", Some(Value::Number(1.0)))));
    assert!(malformed(&with_member("signatures", None)));
    assert!(malformed(&with_member("signatures", s("x"))));
    let bad_inputs: [&[u8]; 6] = [b"", b"[]", b"{", b"null", b"{\"payload\":\"\",\"payload\":\"\"}", b"\xff"];
    for bad in bad_inputs {
        assert!(malformed(bad), "{bad:?}");
    }
    let bad_sig = with_member(
        "signatures",
        Some(Value::Array(vec![Value::Object(vec![("sig".to_owned(), Value::String("AA=A".to_owned()))])])),
    );
    assert!(malformed(&bad_sig));
}

#[test]
fn missing_keyid_is_tried_foreign_keyid_is_skipped() {
    let pem = text("keys/ed25519-test.pub.pem");
    let golden = Envelope::from_json(&bytes("golden/ed25519.dsse.json")).unwrap();
    let mut env = golden.clone();
    env.signatures[0].keyid = String::new();
    let json = env.to_json().unwrap();
    assert!(verify_json(&json, &pem).is_ok());
    env.signatures[0].keyid = "0".repeat(64);
    assert_eq!(verify_json(&env.to_json().unwrap(), &pem), Err(SigError::Invalid));
}

#[test]
fn malformed_and_unsupported_keys() {
    assert!(matches!(PublicKey::from_pem("not pem"), Err(SigError::Malformed(_))));
    assert!(matches!(PublicKey::from_spki_der(b"\x30\x00"), Err(SigError::Malformed(_))));
    // P-256 SPKI with a point that is not on the curve.
    let mut der = PublicKey::from_pem(&text("keys/p256-test.pub.pem")).unwrap().spki_der();
    let last = der.len() - 1;
    der[last] ^= 1;
    assert!(matches!(PublicKey::from_spki_der(&der), Err(SigError::Malformed(_))));
    // id-ecPublicKey on secp384r1 (1.3.132.0.34): unsupported curve.
    let mut p384 =
        b"\x30\x76\x30\x10\x06\x07\x2a\x86\x48\xce\x3d\x02\x01\x06\x05\x2b\x81\x04\x00\x22\x03\x62\x00\x04".to_vec();
    p384.extend_from_slice(&[0u8; 96]);
    assert_eq!(PublicKey::from_spki_der(&p384), Err(SigError::UnsupportedAlgorithm));
}
