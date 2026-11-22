//! The TRUST.PKG.FIXTURE golden packages against the real signature checker.
//!
//! `fixtures/pkg/golden/two-signatures.zip` (written by the independent stdlib generator
//! `fixtures/pkg/make_golden.py` with the published RFC 8032 / RFC 6979 test keys of
//! `fixtures/dsse/keys`) must carry two signatures that both check as `VALID_TRUSTED` against
//! `fixtures/troot/roots/both.json`, bound to the golden's own `statement.json`; every other
//! golden is unsigned. The expected key fingerprints are the committed `*.fingerprint` files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_sign_core::troot::{
    check_package, Keyring, PackageStatus, Revocations, SignatureStatus, TrustInputs, TrustRoot,
};
use std::io::Cursor;
use std::path::PathBuf;

fn fixture(rel: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn fingerprint(name: &str) -> String {
    String::from_utf8(fixture(&format!("dsse/keys/{name}.fingerprint"))).unwrap().trim().to_owned()
}

fn check(golden: &str) -> fid_sign_core::troot::SignatureReport {
    let root = TrustRoot::parse(&fixture("troot/roots/both.json")).unwrap();
    let (rev, ring) = (Revocations::default(), Keyring::default());
    let inputs = TrustInputs { trust_root: &root, revocations: &rev, keyring: &ring, as_of: None };
    check_package(Cursor::new(fixture(&format!("pkg/golden/{golden}"))), &inputs).unwrap()
}

#[test]
fn two_signature_golden_has_two_valid_trusted_signatures() {
    let report = check("two-signatures.zip");
    assert_eq!(report.package_status, PackageStatus::Signed);
    assert!(report.all_trusted(), "{report:?}");
    let got: Vec<(&str, Option<&str>, SignatureStatus)> =
        report.signatures.iter().map(|s| (s.envelope.as_str(), s.fingerprint.as_deref(), s.status)).collect();
    let (p256, ed) = (fingerprint("p256-test"), fingerprint("ed25519-test"));
    assert_eq!(
        got,
        [
            ("signatures/0.dsse.json", Some(p256.as_str()), SignatureStatus::ValidTrusted),
            ("signatures/1.dsse.json", Some(ed.as_str()), SignatureStatus::ValidTrusted),
        ]
    );
    // The signatures bind the same statement as the unsigned points golden.
    assert_eq!(report.statement_sha256, check("points.zip").statement_sha256);
}

#[test]
fn other_goldens_are_unsigned() {
    for g in ["features.zip", "points.zip", "results.zip", "uec0.zip"] {
        let report = check(g);
        assert_eq!(report.package_status, PackageStatus::Unsigned, "{g}");
        assert!(report.signatures.is_empty(), "{g}");
    }
}
