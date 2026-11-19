//! Acceptance criteria of spec node KRN.SOLV.CONIC.VEND (AC-1 .. AC-5) and the other
//! requirements, over the synthetic trees in `fixtures/solv/vend/`. The trees, patches,
//! digests and reports come from `fixtures/solv/vend/make_fixtures.py` (an independent Python
//! applier and digest); the expected findings are hand-labelled from the leaf's rules and listed
//! at the top of that script.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::conic_audit::{audit, parse_record, read_tree, tree_digest, Code, Options, DEFAULT_ALLOWLIST};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    root().join("fixtures/solv/vend").join(rel)
}

fn opts(dir: &str) -> Options {
    let mut o = Options::new(fix(dir));
    o.allowlist = root().join(DEFAULT_ALLOWLIST);
    o
}

/// The findings as `<code> <subject>` strings.
fn run(o: &Options) -> Vec<String> {
    audit(o).iter().map(|f| format!("{} {}", f.code.as_str(), f.subject)).collect()
}

/// Runs `fid-xtask conic-audit <args>` from the workspace root (what `cargo xtask conic-audit`
/// runs); returns (exit code, stdout).
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(root())
        .arg("conic-audit")
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

const F: &str = "fixtures/solv/vend";

/// AC-1: tiny upstream tree, two patches, matching crate, clean report, MIT: exit 0.
#[test]
fn ac1_ok_passes() {
    assert_eq!(run(&opts("ok")), Vec::<String>::new());
    let (code, out) = cli(&["--dir", &format!("{F}/ok")]);
    assert_eq!((code, out.as_str()), (0, "conic-audit: ok\n"));
}

/// AC-2: one byte changed in crate/src/lib.rs: AUDIT-PATCH-DRIFT naming the file, exit 1.
#[test]
fn ac2_crate_byte_changed_is_drift() {
    assert_eq!(run(&opts("drift")), vec!["AUDIT-PATCH-DRIFT crate/src/lib.rs"]);
    let (code, out) = cli(&["--dir", &format!("{F}/drift")]);
    assert_eq!(code, 1);
    assert!(out.starts_with("AUDIT-PATCH-DRIFT crate/src/lib.rs: "), "{out}");
}

/// AC-3: one byte changed in upstream/ (README.md, matched in crate/): AUDIT-DIGEST.
#[test]
fn ac3_upstream_byte_changed_is_digest() {
    assert_eq!(run(&opts("digest")), vec!["AUDIT-DIGEST upstream/"]);
    let (code, out) = cli(&["--dir", &format!("{F}/digest")]);
    assert_eq!(code, 1);
    assert!(out.starts_with("AUDIT-DIGEST upstream/: tree digest is sha256:"), "{out}");
}

/// AC-4: an upstream update that carries only 0001 forward fails with AUDIT-PATCH-MISSING 0002;
/// with 0002 under `dropped` and a justification it passes.
#[test]
fn ac4_update_carries_patches_forward() {
    let mut o = opts("update");
    o.previous = Some(fix("ok/AUDIT.toml"));
    assert_eq!(run(&o), vec!["AUDIT-PATCH-MISSING 0002"]);
    let (code, out) = cli(&["--dir", &format!("{F}/update"), "--previous", &format!("{F}/ok/AUDIT.toml")]);
    assert_eq!(code, 1);
    assert!(out.starts_with("AUDIT-PATCH-MISSING 0002: "), "{out}");

    o.audit = Some(fix("update/AUDIT-dropped.toml"));
    assert_eq!(run(&o), Vec::<String>::new());
    let (code, _) = cli(&[
        "--dir",
        &format!("{F}/update"),
        "--audit",
        &format!("{F}/update/AUDIT-dropped.toml"),
        "--previous",
        &format!("{F}/ok/AUDIT.toml"),
    ]);
    assert_eq!(code, 0);
}

/// REQ-005: the carry-forward check only applies when the upstream commit changed, and without
/// `--previous` the update record passes on its own.
#[test]
fn req5_same_commit_or_no_previous() {
    let mut o = opts("ok");
    o.previous = Some(fix("ok/AUDIT.toml"));
    assert_eq!(run(&o), Vec::<String>::new());
    assert_eq!(run(&opts("update")), Vec::<String>::new());
}

/// AC-5: an unwaived KNP1-POW finding in crate/src/x.rs: AUDIT-LINT.
#[test]
fn ac5_unwaived_finding_is_lint() {
    let mut o = opts("ok");
    o.report = Some(fix("lint/unwaived-pow.json"));
    assert_eq!(run(&o), vec!["AUDIT-LINT crate/src/x.rs:6:14"]);
    assert_eq!(audit(&o)[0].detail, "unwaived KNP1-POW finding");
    let (code, out) = cli(&["--dir", &format!("{F}/ok"), "--report", &format!("{F}/lint/unwaived-pow.json")]);
    assert_eq!(code, 1);
    assert_eq!(out, "AUDIT-LINT crate/src/x.rs:6:14: unwaived KNP1-POW finding\n");
}

/// REQ-004: findings in other crates do not count; a waived finding in the crate needs its
/// waiver listed with a justification, and a listed waiver must be used.
#[test]
fn req4_waivers() {
    let mut o = opts("ok");
    o.report = Some(fix("lint/other-crate.json"));
    assert_eq!(run(&o), Vec::<String>::new());

    o.report = Some(fix("lint/waived.json"));
    assert_eq!(run(&o), vec!["AUDIT-LINT crate/src/lib.rs:7:14"]);

    o.audit = Some(fix("lint/AUDIT-waived.toml"));
    assert_eq!(run(&o), Vec::<String>::new());

    o.report = Some(fix("ok/knp1-findings.json"));
    assert_eq!(run(&o), vec!["AUDIT-LINT waiver src/lib.rs:7-7:KNP1-POW"]);
}

/// REQ-003: the recorded licence must be allowed by the SPDX allowlist.
#[test]
fn req3_licence() {
    let mut o = opts("ok");
    o.audit = Some(fix("licence/AUDIT-gpl.toml"));
    assert_eq!(run(&o), vec!["AUDIT-LICENCE upstream.licence"]);
    o.audit = Some(fix("licence/AUDIT-or.toml"));
    assert_eq!(run(&o), Vec::<String>::new());
}

/// REQ-006: every patch names a KNP-1 rule ID or `scope-reduction`.
#[test]
fn req6_patch_rules() {
    let mut o = opts("ok");
    o.audit = Some(fix("schema/AUDIT-norule.toml"));
    assert_eq!(run(&o), vec!["AUDIT-SCHEMA patch 0002"]);
    o.audit = Some(fix("schema/AUDIT-scope.toml"));
    assert_eq!(run(&o), Vec::<String>::new());
}

/// REQ-006: missing and unknown fields are AUDIT-SCHEMA findings.
#[test]
fn req6_schema_fields() {
    let (_, errs) = parse_record("schema = 2\nextra = 1\n").unwrap();
    let subjects: Vec<String> = errs.iter().map(|f| f.subject.clone()).collect();
    assert!(errs.iter().all(|f| f.code == Code::Schema));
    for s in ["schema", "AUDIT.toml", "upstream", "date"] {
        assert!(subjects.iter().any(|x| x == s), "{s}: {subjects:?}");
    }
    assert!(parse_record("not toml [").is_err());
    let text = std::fs::read_to_string(fix("ok/AUDIT.toml")).unwrap();
    let (rec, errs) = parse_record(&text).unwrap();
    assert!(errs.is_empty(), "{errs:?}");
    assert_eq!(rec.patches.len(), 2);
    assert_eq!(rec.upstream.licence, "MIT");
}

/// REQ-001: the digest recomputed here equals the one the Python reference recorded.
#[test]
fn req1_tree_digest_matches_reference() {
    let text = std::fs::read_to_string(fix("ok/AUDIT.toml")).unwrap();
    let (rec, _) = parse_record(&text).unwrap();
    let tree = read_tree(&fix("ok/upstream")).unwrap();
    assert_eq!(tree.len(), 5);
    assert_eq!(tree_digest(&tree).as_str(), rec.upstream.tree_sha256);
}

/// Bad usage exits 2.
#[test]
fn usage() {
    assert_eq!(cli(&["--nope"]).0, 2);
    assert_eq!(cli(&["--dir"]).0, 2);
}
