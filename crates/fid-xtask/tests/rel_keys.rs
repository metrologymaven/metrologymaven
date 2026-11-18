//! Acceptance criteria AC-1 and AC-2 of spec node ECO.REL.SIGN.GUIDE (`cargo xtask rel
//! keys-check`). Fingerprints in the fixture `KEYS.md` files come from
//! `fixtures/rel/guide/make_fixtures.py` (hashlib over the SPKI DER, not the Rust code). The
//! fixture keys are FAKE: well-formed Ed25519 SPKI structures around ASCII text, with no
//! private key.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::rel::keys::{check, fingerprint, parse_doc, spki_der, KeysError};
use std::path::{Path, PathBuf};
use std::process::Command;

const DATE: &str = "2026-11-18";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/rel/guide").join(name)
}

/// Runs `fid-xtask rel keys-check <args>` from the repository root; returns (exit, stdout).
fn cli(args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(root())
        .args(["rel", "keys-check"])
        .args(args)
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8(o.stdout).unwrap())
}

fn doc_args(doc: &str) -> Vec<String> {
    vec![
        "--keys".into(),
        "fixtures/rel/guide/keys".into(),
        "--doc".into(),
        format!("fixtures/rel/guide/docs/{doc}"),
        "--date".into(),
        DATE.into(),
    ]
}

fn run(doc: &str) -> (i32, String) {
    let a = doc_args(doc);
    cli(&a.iter().map(String::as_str).collect::<Vec<_>>())
}

/// AC-1: the fixture keys agree with the fixture KEYS.md (a retired key may be expired).
#[test]
fn ac1_fixture_keys_check_ok() {
    assert_eq!(run("KEYS.md"), (0, format!("keys-check: ok 4 keys ({DATE})\n")));
    assert_eq!(check(&fixture("keys"), &fixture("docs/KEYS.md"), DATE), Ok(4));
}

/// AC-1: the repository's own keys/ directory and KEYS.md agree (no keys are published yet).
#[test]
fn ac1_repository_keys_check_ok() {
    let (code, out) = cli(&["--date", DATE]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("keys-check: ok "), "{out}");
}

/// AC-2: a key file not listed in KEYS.md exits 1 with KEYS-UNLISTED.
#[test]
fn ac2_unlisted() {
    assert_eq!(run("KEYS-unlisted.md"), (1, "KEYS-UNLISTED packaging-test.pem\n".to_string()));
}

/// AC-2: a wrong listed fingerprint exits 1 with KEYS-FINGERPRINT naming both values.
#[test]
fn ac2_wrong_fingerprint() {
    let (code, out) = run("KEYS-fingerprint.md");
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "KEYS-FINGERPRINT nightly-test.pem (listed \
         a22949fdaf44b3436f3889dc37e418e45965593175c74b5bc414c5ddce95acb0, actual \
         a22949fdaf44b3436f3889dc37e418e45965593175c74b5bc414c5ddce95acb7)\n"
    );
}

/// An active key past its validity is KEYS-EXPIRED-IN-USE; before that date it is fine.
#[test]
fn expired_in_use() {
    assert_eq!(
        run("KEYS-expired.md"),
        (1, format!("KEYS-EXPIRED-IN-USE release-old-test.pem (valid until 2021-12-31, checked {DATE})\n"))
    );
    assert_eq!(check(&fixture("keys"), &fixture("docs/KEYS-expired.md"), "2021-12-31"), Ok(4));
}

/// An active key checked before its validity starts is KEYS-NOT-YET-VALID (hand-labelled:
/// the three active fixture keys are valid from 2026-01-01).
#[test]
fn not_yet_valid() {
    let errs = check(&fixture("keys"), &fixture("docs/KEYS.md"), "2025-12-31").unwrap_err();
    let lines: Vec<String> = errs.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "KEYS-NOT-YET-VALID nightly-test.pem (valid from 2026-01-01, checked 2025-12-31)",
            "KEYS-NOT-YET-VALID packaging-test.pem (valid from 2026-01-01, checked 2025-12-31)",
            "KEYS-NOT-YET-VALID release-test.pem (valid from 2026-01-01, checked 2025-12-31)",
        ]
    );
}

/// The fingerprint is the SHA-256 of the SPKI DER (value from make_fixtures.py).
#[test]
fn fingerprint_of_fixture_key() {
    let pem = std::fs::read_to_string(fixture("keys/release-test.pem")).unwrap();
    let der = spki_der(&pem).unwrap();
    assert_eq!(der.len(), 44);
    assert_eq!(fingerprint(&der), "00fd1e388595e3498fc8edfdbca1d4e2fc6d8f8d4b07c37f9e8c04a0928ea10a");
}

/// Malformed PEM files are refused (hand-labelled).
#[test]
fn pem_errors() {
    assert!(spki_der("no key here\n").is_err());
    assert!(spki_der("-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA\n").is_err(), "no END");
    assert!(spki_der("-----BEGIN PRIVATE KEY-----\nMCowBQ==\n-----END PRIVATE KEY-----\n").is_err());
    assert!(spki_der("-----BEGIN PUBLIC KEY-----\n!!!!\n-----END PUBLIC KEY-----\n").is_err());
    // Valid base64, but the DER SEQUENCE length (0x2a) does not match the 3 bytes present.
    assert!(spki_der("-----BEGIN PUBLIC KEY-----\nMCow\n-----END PUBLIC KEY-----\n").is_err());
    let one = std::fs::read_to_string(fixture("keys/nightly-test.pem")).unwrap();
    assert!(spki_der(&format!("{one}{one}")).is_err(), "two blocks");
}

/// A listed key that is absent is KEYS-MISSING; a malformed table is KEYS-DOC with exit 2.
#[test]
fn missing_and_doc_errors() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rel-keys");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("keys")).unwrap();
    let fp = "0".repeat(64);
    let doc = tmp.join("KEYS.md");
    std::fs::write(
        &doc,
        format!(
            "| File | Class | Fingerprint | Valid from | Valid until | Status |\n|---|---|---|---|---|---|\n\
             | `gone.pem` | release | `{fp}` | 2026-01-01 | 2027-01-01 | active |\n"
        ),
    )
    .unwrap();
    assert_eq!(check(&tmp.join("keys"), &doc, DATE), Err(vec![KeysError::Missing("gone.pem".into())]));

    std::fs::write(&doc, "# no table\n").unwrap();
    let (code, out) = cli(&["--keys", tmp.join("keys").to_str().unwrap(), "--doc", doc.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(out.starts_with("KEYS-DOC: no table"), "{out}");

    let bad = "| File | Class | Fingerprint | Valid from | Valid until | Status |\n|---|---|---|---|---|---|\n\
               | a.pem | master | abc | 2026-02-30 | 2026-01-01 | live |\n| a.pem | release |\n";
    let errs = parse_doc(bad).unwrap_err();
    let codes: Vec<String> = errs.iter().map(ToString::to_string).collect();
    assert_eq!(codes.len(), 5, "{codes:?}");
    assert!(codes.iter().all(|c| c.starts_with("KEYS-DOC: line ")), "{codes:?}");
    assert_eq!(cli(&["--date", "2026-02-30"]).0, 2);
    assert_eq!(cli(&["--bogus"]).0, 2);
}

/// The check reads files only.
#[test]
fn check_is_read_only() {
    let listing = |d: &Path| {
        let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap())
            })
            .collect();
        v.sort();
        v
    };
    let before = listing(&fixture("keys"));
    let _ = run("KEYS-fingerprint.md");
    assert_eq!(listing(&fixture("keys")), before);
}
