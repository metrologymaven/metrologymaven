//! Acceptance criteria of spec node KRN.SOLV.CONIC.VEND (AC-1 .. AC-5) and the other
//! requirements, over the synthetic trees in `fixtures/solv/vend/`. The trees, patches,
//! digests and reports come from `fixtures/solv/vend/make_fixtures.py` (an independent Python
//! applier and digest); the expected findings are hand-labelled from the leaf's rules and listed
//! at the top of that script. Cases that need a symbolic link, a `.gitattributes` or an extra
//! file copy `ok/` to a temporary directory and change it there.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::conic_audit::{
    audit, parse_record, read_tree, tree_digest, Code, Options, DEFAULT_ALLOWLIST, DEFAULT_DENY,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    root().join("fixtures/solv/vend").join(rel)
}

fn opts_at(dir: PathBuf) -> Options {
    let mut o = Options::new(dir);
    o.allowlist = root().join(DEFAULT_ALLOWLIST);
    o.deny = root().join(DEFAULT_DENY);
    o
}

fn opts(dir: &str) -> Options {
    opts_at(fix(dir))
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

fn none() -> Vec<String> {
    Vec::new()
}

/// A lint case on `ok/`: `--report lint/<report>.json --audit lint/AUDIT-<record>.toml`.
fn lint_case(record: &str, report: &str) -> Vec<String> {
    let mut o = opts("ok");
    o.audit = Some(fix(&format!("lint/AUDIT-{record}.toml")));
    o.report = Some(fix(&format!("lint/{report}.json")));
    run(&o)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// A fresh copy of `ok/` in a temporary directory named after the test.
fn ok_copy(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fid-conic-audit-{}-{name}", std::process::id()));
    if base.exists() {
        std::fs::remove_dir_all(&base).unwrap();
    }
    let dir = base.join("ok");
    copy_dir(&fix("ok"), &dir);
    dir
}

/// AC-1: tiny upstream tree, two patches, matching crate, clean report, MIT: exit 0.
#[test]
fn ac1_ok_passes() {
    assert_eq!(run(&opts("ok")), none());
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
    assert_eq!(run(&o), none());
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
    assert_eq!(run(&o), none());
    assert_eq!(run(&opts("update")), none());
}

/// AC-5: an unwaived KNP1-POW finding in crate/src/x.rs: AUDIT-LINT.
#[test]
fn ac5_unwaived_finding_is_lint() {
    assert_eq!(lint_case("unwaived-pow", "unwaived-pow"), vec!["AUDIT-LINT crate/src/x.rs:6:14"]);
    let (code, out) = cli(&[
        "--dir",
        &format!("{F}/ok"),
        "--report",
        &format!("{F}/lint/unwaived-pow.json"),
        "--audit",
        &format!("{F}/lint/AUDIT-unwaived-pow.toml"),
    ]);
    assert_eq!(code, 1);
    assert_eq!(out, "AUDIT-LINT crate/src/x.rs:6:14: unwaived KNP1-POW finding\n");
}

/// REQ-004: findings in other crates do not count; a waived finding in the crate needs its
/// waiver (of the same crate) listed with a justification, and a listed waiver must be used.
#[test]
fn req4_waivers() {
    assert_eq!(lint_case("other-crate", "other-crate"), none());
    assert_eq!(lint_case("waived", "waived"), vec!["AUDIT-LINT crate/src/lib.rs:7:14"]);
    assert_eq!(lint_case("waived-listed", "waived"), none());
    assert_eq!(
        lint_case("waiver-crate", "waiver-crate"),
        vec!["AUDIT-LINT crate/src/lib.rs:7:14", "AUDIT-LINT waiver src/lib.rs:7-7:KNP1-POW"]
    );
}

/// REQ-004 (maintainer decision): a report that does not name the vendored crate and its
/// version under `linted` never linted it, so it is not clean.
#[test]
fn req4_report_must_name_the_crate() {
    assert_eq!(lint_case("not-linted", "not-linted"), vec!["AUDIT-LINT report"]);
    assert_eq!(lint_case("wrong-version", "wrong-version"), vec!["AUDIT-LINT report"]);
    let mut o = opts("ok");
    o.audit = Some(fix("lint/AUDIT-not-linted.toml"));
    o.report = Some(fix("lint/not-linted.json"));
    let found = audit(&o);
    assert!(found[0].detail.contains("does not name minisolv 0.1.0"), "{found:?}");
}

/// `lint_report_sha256` must be the SHA-256 of the report actually read.
#[test]
fn req4_report_digest() {
    let mut o = opts("ok");
    o.report = Some(fix("lint/other-crate.json"));
    assert_eq!(run(&o), vec!["AUDIT-LINT report"]);
    let found = audit(&o);
    assert!(found[0].detail.starts_with("SHA-256 is sha256:"), "{found:?}");
    // A listed waiver with the clean report: wrong digest, and the waiver is unused.
    assert_eq!(
        lint_case("waived-listed", "../ok/knp1-findings"),
        vec!["AUDIT-LINT report", "AUDIT-LINT waiver src/lib.rs:7-7:KNP1-POW"]
    );
}

/// REQ-003: the recorded licence must be allowed by the SPDX allowlist and deny.toml; a `WITH`
/// pair only when deny.toml lists exactly that pair.
#[test]
fn req3_licence() {
    let case = |name: &str| {
        let mut o = opts("ok");
        o.audit = Some(fix(&format!("licence/AUDIT-{name}.toml")));
        run(&o)
    };
    assert_eq!(case("gpl"), vec!["AUDIT-LICENCE upstream.licence"]);
    assert_eq!(case("or"), none());
    assert_eq!(case("mit-with"), vec!["AUDIT-LICENCE upstream.licence"]);
    assert_eq!(case("apache-with"), none());
}

/// REQ-006: every patch names a KNP-1 rule ID or `scope-reduction`.
#[test]
fn req6_patch_rules() {
    let mut o = opts("ok");
    o.audit = Some(fix("schema/AUDIT-norule.toml"));
    assert_eq!(run(&o), vec!["AUDIT-SCHEMA patch 0002"]);
    o.audit = Some(fix("schema/AUDIT-scope.toml"));
    assert_eq!(run(&o), none());
}

/// REQ-006: missing, unknown and duplicate fields are AUDIT-SCHEMA findings.
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

    // Duplicate NNNN.
    let extra = "\n[[patches]]\nid = \"0001\"\nfile = \"patches/0001-again.patch\"\nrules = [\"KNP1-POW\"]\n\
                 rationale = \"again\"\n";
    let (_, errs) = parse_record(&format!("{text}{extra}")).unwrap();
    let errs: Vec<String> = errs.iter().map(|f| format!("{} {}", f.subject, f.detail)).collect();
    assert_eq!(errs, vec!["patch 0001 duplicate patch `id`"]);
}

/// REQ-001: the digest recomputed here equals the one the Python reference recorded, over bytes
/// as committed (the CRLF file keeps its CR bytes on every platform).
#[test]
fn req1_tree_digest_matches_reference() {
    let text = std::fs::read_to_string(fix("ok/AUDIT.toml")).unwrap();
    let (rec, _) = parse_record(&text).unwrap();
    let tree = read_tree(&fix("ok/upstream")).unwrap();
    assert_eq!(tree.len(), 6);
    assert_eq!(tree["data/crlf.txt"], b"row 1\r\nrow 2\r\n".to_vec());
    assert_eq!(tree_digest(&tree).as_str(), rec.upstream.tree_sha256);
}

/// REQ-002: a patch that deletes one file and creates another, applied to the trees on disk.
#[test]
fn req2_create_and_delete_files() {
    assert_eq!(run(&opts("createdelete")), none());
    let (code, _) = cli(&["--dir", &format!("{F}/createdelete")]);
    assert_eq!(code, 0);
}

/// A file in patches/ that the record does not list.
#[test]
fn unlisted_patch_file() {
    let dir = ok_copy("unlisted");
    std::fs::write(dir.join("patches/0003-extra.patch"), "--- a/x\n+++ b/x\n").unwrap();
    assert_eq!(run(&opts_at(dir)), vec!["AUDIT-SCHEMA patches/0003-extra.patch"]);
}

/// `.gitattributes` anywhere in the vendored tree is refused.
#[test]
fn gitattributes_refused() {
    let dir = ok_copy("gitattr-up");
    std::fs::write(dir.join("upstream/.gitattributes"), "* text\n").unwrap();
    assert_eq!(run(&opts_at(dir)), vec!["AUDIT-DIGEST upstream/"]);

    let dir = ok_copy("gitattr-root");
    std::fs::write(dir.join(".gitattributes"), "* text\n").unwrap();
    assert_eq!(run(&opts_at(dir)), vec!["AUDIT-SCHEMA .gitattributes"]);

    let dir = ok_copy("gitattr-patches");
    std::fs::write(dir.join("patches/.gitattributes"), "* text\n").unwrap();
    assert_eq!(run(&opts_at(dir)), vec!["AUDIT-SCHEMA patches/.gitattributes"]);
}

/// Symbolic links are refused everywhere: inside upstream/, in patches/, as the crate/ root and
/// as the vendored directory itself (also when dangling).
#[cfg(unix)]
#[test]
fn symlinks_refused() {
    use std::os::unix::fs::symlink;

    let dir = ok_copy("link-in-upstream");
    symlink("lib.rs", dir.join("upstream/src/link.rs")).unwrap();
    assert_eq!(run(&opts_at(dir)), vec!["AUDIT-DIGEST upstream/"]);

    let dir = ok_copy("link-in-patches");
    symlink("0001-no-powi.patch", dir.join("patches/0003-link.patch")).unwrap();
    assert_eq!(run(&opts_at(dir)), vec!["AUDIT-SCHEMA patches/0003-link.patch"]);

    let dir = ok_copy("link-crate-root");
    std::fs::rename(dir.join("crate"), dir.join("crate-real")).unwrap();
    symlink("crate-real", dir.join("crate")).unwrap();
    let found = run(&opts_at(dir));
    assert!(found.contains(&"AUDIT-PATCH-DRIFT crate/".to_string()), "{found:?}");
    assert!(found.contains(&"AUDIT-SCHEMA crate".to_string()), "{found:?}");

    let dir = ok_copy("link-root");
    let link = dir.with_file_name("link");
    symlink(&dir, &link).unwrap();
    assert_eq!(run(&opts_at(link)), vec!["AUDIT-SCHEMA vendored directory"]);

    let dir = ok_copy("dangling");
    let link = dir.with_file_name("dangling-link");
    symlink(dir.join("does-not-exist"), &link).unwrap();
    assert_eq!(run(&opts_at(link)), vec!["AUDIT-SCHEMA vendored directory"]);
}

/// Windows: creating a symbolic link needs a privilege the CI runner may lack, but a directory
/// junction (`mklink /J`) does not. Junctions are name-surrogate reparse points, which the standard
/// library reports as symbolic links, so they are refused the same way.
#[cfg(windows)]
#[test]
fn junctions_refused() {
    fn junction(link: &Path, target: &Path) {
        let ok = Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).status().unwrap();
        assert!(ok.success(), "mklink /J failed");
    }

    let dir = ok_copy("junction-in-upstream");
    junction(&dir.join("upstream").join("srclink"), &dir.join("upstream").join("src"));
    assert_eq!(run(&opts_at(dir)), vec!["AUDIT-DIGEST upstream/"]);

    let dir = ok_copy("junction-root");
    let link = dir.with_file_name("link");
    junction(&link, &dir);
    assert_eq!(run(&opts_at(link)), vec!["AUDIT-SCHEMA vendored directory"]);
}

/// Bad usage exits 2.
#[test]
fn usage() {
    assert_eq!(cli(&["--nope"]).0, 2);
    assert_eq!(cli(&["--dir"]).0, 2);
}
