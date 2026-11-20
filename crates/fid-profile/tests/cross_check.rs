#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Cross-check: CORE's canonicaliser (`fid-canon`, a dev-dependency only) gives the same
//! bytes as `fid-trust-canon` for every expected profile document. `fid-profile` itself never
//! links `fid-canon` (tests/independence.rs).

use std::path::PathBuf;

use fid_profile::{instantiate, CanonicalProfile, ProfileName};

fn bytes(rel: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/profile").join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

const DOCUMENTS: [&str; 7] = [
    "builtin/lean.json",
    "builtin/regulated.json",
    "builtin/assistive_only.json",
    "expected/regulated-with-xqa.json",
    "expected/lean-two-meanings.json",
    "expected/assistive_only-two-meanings.json",
    "expected/lean-nonascii.json",
];

#[test]
fn expected_documents_canonicalise_identically_under_fid_canon() {
    for rel in DOCUMENTS {
        let b = bytes(rel);
        assert_eq!(fid_canon::canonicalize(&b).unwrap(), b, "{rel}");
        let ours = CanonicalProfile::read(&b).unwrap();
        assert_eq!(fid_canon::digest_bytes(&b).to_string(), ours.digest().as_str(), "{rel}");
    }
}

#[test]
fn instantiated_documents_are_fixed_points_of_fid_canon() {
    let nonascii = bytes("tenant/nonascii.json");
    let xqa = bytes("regulated-with-xqa.json");
    let cases: [(ProfileName, Option<&[u8]>); 5] = [
        (ProfileName::Lean, None),
        (ProfileName::Regulated, None),
        (ProfileName::AssistiveOnly, None),
        (ProfileName::Regulated, Some(&xqa)),
        (ProfileName::Lean, Some(&nonascii)),
    ];
    for (name, tenant) in cases {
        let got = instantiate(name, tenant).unwrap();
        assert_eq!(fid_canon::canonicalize(got.bytes()).unwrap(), got.bytes(), "{name}");
    }
}
