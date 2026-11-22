//! Acceptance and unit tests for `ECO.CI.LIC` (`fid_xtask::licence_check`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

use fid_xtask::licence_check::{
    eval_spdx, glob_match, parse_spdx, run, Allowlist, EvalOutcome, FindingCode, MAX_SPDX_DEPTH, MAX_SPDX_TOKENS,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("crates/").parent().expect("repo root").to_path_buf()
}

fn lic(rel: &str) -> PathBuf {
    repo_root().join("fixtures/lic").join(rel)
}

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fid-xtask"))
}

/// AC-1: `metadata-ok.json` (MIT, Apache-2.0, MIT OR Apache-2.0) + `allow.toml` + `tree-ok` exits 0.
#[test]
fn ac1_ok_metadata_and_tree_pass() {
    let findings = run(&lic("metadata-ok.json"), &lic("allow.toml"), &lic("tree-ok"), None).expect("run ok");
    assert!(findings.is_empty(), "expected 0 findings, got: {findings:?}");

    let out = Command::new(bin_path())
        .args([
            "licence-check",
            "--metadata",
            lic("metadata-ok.json").to_str().unwrap(),
            "--allow",
            lic("allow.toml").to_str().unwrap(),
            "--root",
            lic("tree-ok").to_str().unwrap(),
        ])
        .output()
        .expect("run fid-xtask");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "licence-check: ok");
}

/// AC-2: `metadata-gpl.json` exits 1 with `LIC-DENIED <name> <version> GPL-3.0-only`.
#[test]
fn ac2_gpl_denied() {
    let findings = run(&lic("metadata-gpl.json"), &lic("allow.toml"), &lic("tree-ok"), None).expect("run gpl");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, FindingCode::LicDenied);
    assert_eq!(findings[0].to_string(), "LIC-DENIED bad-gpl 3.0.1 GPL-3.0-only");

    let out = Command::new(bin_path())
        .args([
            "licence-check",
            "--metadata",
            lic("metadata-gpl.json").to_str().unwrap(),
            "--allow",
            lic("allow.toml").to_str().unwrap(),
            "--root",
            lic("tree-ok").to_str().unwrap(),
        ])
        .output()
        .expect("run fid-xtask");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "LIC-DENIED bad-gpl 3.0.1 GPL-3.0-only");
}

/// AC-3: `GPL-3.0-only OR MIT` passes; `MIT AND GPL-3.0-only` exits 1.
#[test]
fn ac3_or_passes_and_fails() {
    let ok = run(&lic("metadata-or.json"), &lic("allow.toml"), &lic("tree-ok"), None).expect("run or");
    assert!(ok.is_empty(), "OR with allowed branch must pass: {ok:?}");

    let bad = run(&lic("metadata-and.json"), &lic("allow.toml"), &lic("tree-ok"), None).expect("run and");
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].code, FindingCode::LicDenied);
    assert_eq!(bad[0].to_string(), "LIC-DENIED both-mit-gpl 1.2.3 MIT AND GPL-3.0-only");
}

/// AC-4: `metadata-nolicence.json` without clarification -> `LIC-MISSING`; with matching
/// `clarify.toml` -> pass; with mismatching `clarify-stale.toml` -> `LIC-CLARIFY-STALE`.
#[test]
fn ac4_missing_and_clarification_digest() {
    let missing =
        run(&lic("metadata-nolicence.json"), &lic("allow.toml"), &lic("tree-ok"), None).expect("run no clarify");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].code, FindingCode::LicMissing);
    assert_eq!(missing[0].to_string(), "LIC-MISSING unlabelled 0.2.0");

    let ok = run(&lic("metadata-nolicence.json"), &lic("allow.toml"), &lic("tree-ok"), Some(&lic("clarify.toml")))
        .expect("run matching clarify");
    assert!(ok.is_empty(), "matching clarification must pass: {ok:?}");

    let stale =
        run(&lic("metadata-nolicence.json"), &lic("allow.toml"), &lic("tree-ok"), Some(&lic("clarify-stale.toml")))
            .expect("run stale clarify");
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].code, FindingCode::LicClarifyStale);
    assert!(stale[0].to_string().starts_with("LIC-CLARIFY-STALE unlabelled 0.2.0 "));
}

/// AC-5: `tree-missing-attr/` exits 1 with `ATTR-MISSING assets/icons/x.svg`; `tree-orphan-attr/`
/// exits 1 with `ATTR-ORPHAN assets/icons/deleted.svg`.
#[test]
fn ac5_asset_attribution_missing_and_orphan() {
    let missing =
        run(&lic("metadata-ok.json"), &lic("allow.toml"), &lic("tree-missing-attr"), None).expect("run missing attr");
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].code, FindingCode::AttrMissing);
    assert_eq!(missing[0].to_string(), "ATTR-MISSING assets/icons/x.svg");

    let orphan =
        run(&lic("metadata-ok.json"), &lic("allow.toml"), &lic("tree-orphan-attr"), None).expect("run orphan attr");
    assert_eq!(orphan.len(), 1);
    assert_eq!(orphan[0].code, FindingCode::AttrOrphan);
    assert_eq!(orphan[0].to_string(), "ATTR-ORPHAN assets/icons/deleted.svg");
}

/// AC-6: `metadata-with.json` (`Apache-2.0 WITH LLVM-exception`) exits 1 with `LIC-EXCEPTION`
/// when `LLVM-exception` is not allowlisted, and passes when it is allowlisted.
#[test]
fn ac6_with_exception_allowlist() {
    let bad = run(&lic("metadata-with.json"), &lic("allow.toml"), &lic("tree-ok"), None).expect("run with");
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].code, FindingCode::LicException);
    assert_eq!(bad[0].to_string(), "LIC-EXCEPTION llvm-dep 0.9.0 Apache-2.0 WITH LLVM-exception");

    let ok =
        run(&lic("metadata-with.json"), &lic("allow-with-llvm.toml"), &lic("tree-ok"), None).expect("run with llvm");
    assert!(ok.is_empty(), "allowlisted exception must pass: {ok:?}");
}

/// Bounded SPDX parser enforces max depth (16), max tokens (256), precedence (`WITH` > `AND` >
/// `OR`), parentheses, and `+` suffix handling.
#[test]
fn spdx_parser_bounds_and_semantics() {
    let allow = Allowlist::parse(
        r#"
licences = ["MIT", "Apache-2.0"]
exceptions = ["LLVM-exception"]
asset_licences = ["CC-BY-4.0"]
asset_paths = ["assets/**"]
"#,
    )
    .expect("parse allow");

    // Precedence: GPL-3.0-only AND Apache-2.0 OR MIT == (GPL-3.0-only AND Apache-2.0) OR MIT => Allowed.
    let ast = parse_spdx("GPL-3.0-only AND Apache-2.0 OR MIT").expect("parse precedence");
    assert_eq!(eval_spdx(&ast, &allow), EvalOutcome::Allowed);

    // Parentheses override precedence: GPL-3.0-only AND (Apache-2.0 OR MIT) => DeniedLicence.
    let ast = parse_spdx("GPL-3.0-only AND (Apache-2.0 OR MIT)").expect("parse parens");
    assert_eq!(eval_spdx(&ast, &allow), EvalOutcome::DeniedLicence);

    // `+` suffix allowed when base ID is allowlisted, denied otherwise.
    assert_eq!(eval_spdx(&parse_spdx("MIT+").expect("mit+"), &allow), EvalOutcome::Allowed);
    assert_eq!(eval_spdx(&parse_spdx("GPL-2.0+").expect("gpl+"), &allow), EvalOutcome::DeniedLicence);

    // Exact depth limit passes; depth + 1 fails cleanly without recursing further.
    let ok_deep = format!("{}MIT{}", "(".repeat(MAX_SPDX_DEPTH), ")".repeat(MAX_SPDX_DEPTH));
    assert!(parse_spdx(&ok_deep).is_ok());
    let too_deep = format!("{}MIT{}", "(".repeat(MAX_SPDX_DEPTH + 1), ")".repeat(MAX_SPDX_DEPTH + 1));
    assert!(parse_spdx(&too_deep).is_err());

    // Token limit enforced.
    let too_many = std::iter::repeat_n("MIT", MAX_SPDX_TOKENS).collect::<Vec<_>>().join(" OR ");
    assert!(parse_spdx(&too_many).is_err());

    // Malformed syntax fails cleanly.
    assert!(parse_spdx("").is_err());
    assert!(parse_spdx("MIT OR").is_err());
    assert!(parse_spdx("(MIT").is_err());
    assert!(parse_spdx("Apache-2.0 WITH").is_err());
    assert!(parse_spdx("MIT/Apache-2.0").is_err());
}

/// Glob matching supports `*`, `?`, and `**` including zero-segment matches.
#[test]
fn glob_matching_rules() {
    assert!(glob_match("assets/**", "assets/icons/x.svg"));
    assert!(glob_match("assets/**", "assets/x.svg"));
    assert!(!glob_match("assets/**", "other/x.svg"));
    assert!(glob_match("crates/*/assets/**", "crates/fid-report-pdf/assets/fonts/a.ttf"));
    assert!(!glob_match("crates/*/assets/**", "crates/a/b/assets/x.svg"));
    assert!(glob_match("assets/icons/?.svg", "assets/icons/x.svg"));
    assert!(!glob_match("assets/icons/?.svg", "assets/icons/xy.svg"));
}

/// Real house-rules/spdx-allowlist.toml parses cleanly with `Allowlist::parse`.
#[test]
fn parses_production_spdx_allowlist() {
    let text = std::fs::read_to_string(repo_root().join("house-rules/spdx-allowlist.toml")).expect("read allowlist");
    let allow = Allowlist::parse(&text).expect("parse production allowlist");
    assert!(allow.licences.contains("MIT"));
    assert!(allow.licences.contains("Apache-2.0"));
    assert!(allow.asset_licences.contains("CC-BY-4.0"));
}
