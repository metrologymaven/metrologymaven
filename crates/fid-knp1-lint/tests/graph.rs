//! Acceptance criteria of spec node KRN.NUM.LINT.GRAPH (AC-1 .. AC-5) and its error paths.
//! Fixtures and expected results under `fixtures/knp1/graph/` are hand-written and
//! hand-labelled from the spec's rules (see its README), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::digest_bytes;
use fid_knp1_lint::find::{Finding, Rule};
use fid_knp1_lint::graph::{
    check, compute_closure, glob, load_config, parse_allowlist, parse_config, parse_spdx_allowlist, run, semver, spdx,
    GraphErrorCode, SourceKind, SpdxAllowlist,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/knp1/graph").join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fix(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// One line per finding: `RULE crate version message`.
fn lines(findings: &[Finding]) -> Vec<String> {
    findings.iter().map(|f| format!("{} {} {} {}", f.rule, f.crate_name, f.crate_version, f.message)).collect()
}

fn expected_lines(rel: &str) -> Vec<String> {
    String::from_utf8(read(rel)).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn ac1_closure_lists_four_crates_with_files() {
    let r = run(&fix("ok/knp1-lint.toml")).unwrap();
    let bytes = r.closure.to_canonical_bytes().unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), String::from_utf8(read("ok/expected-closure.json")).unwrap());
    let names: Vec<&str> = r.closure.crates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["ext-a", "ext-b", "ext-m", "fid-num"]);
    // The dev-only, GPL-licensed ext-dev is not in the closure.
    assert!(!names.contains(&"ext-dev"));
    let kinds: Vec<SourceKind> = r.closure.crates.iter().map(|c| c.source_kind).collect();
    assert_eq!(kinds, [SourceKind::Vendored, SourceKind::Vendored, SourceKind::Vendored, SourceKind::Workspace]);
    assert_eq!(lines(&r.findings), expected_lines("ok/expected-findings.txt"));
    assert!(r.lockfile_digest.starts_with("sha256:"));
}

#[test]
fn ac1_closure_is_deterministic() {
    let a = run(&fix("ok/knp1-lint.toml")).unwrap();
    let b = run(&fix("ok/knp1-lint.toml")).unwrap();
    assert_eq!(a.closure.to_canonical_bytes().unwrap(), b.closure.to_canonical_bytes().unwrap());
    assert_eq!(a.findings, b.findings);
}

#[test]
fn ac2_unlisted_crate_reported_with_path() {
    let r = run(&fix("unlisted/knp1-lint.toml")).unwrap();
    assert_eq!(lines(&r.findings), expected_lines("unlisted/expected-findings.txt"));
    let f = &r.findings[0];
    assert_eq!(format!("{} {}", f.rule, f.message), "KNP1-ALLOW-UNLISTED ext-b via fid-num -> ext-a -> ext-b");
    assert_eq!(f.rule, Rule::Allow("UNLISTED".to_string()));
    f.validate().unwrap();
    assert_eq!(f.excerpt_digest, digest_bytes(f.message.as_bytes()).to_string());
}

#[test]
fn req003_version_mismatch_reported() {
    let r = run(&fix("version/knp1-lint.toml")).unwrap();
    assert_eq!(lines(&r.findings), expected_lines("version/expected-findings.txt"));
    r.findings[0].validate().unwrap();
}

#[test]
fn ac3_licence_expressions() {
    let cfg = load_config(&fix("ok/knp1-lint.toml")).unwrap();
    let closure = compute_closure(&cfg).unwrap();
    let allow = parse_allowlist(&String::from_utf8(read("ok/kernel-allowlist.toml")).unwrap()).unwrap();
    // MIT allowed, GPL not.
    let spdx_list = SpdxAllowlist { licences: set(&["MIT"]), exceptions: set(&["LLVM-exception"]) };
    let with_licence = |expr: &str| {
        let mut c = closure.clone();
        for e in c.crates.iter_mut() {
            e.licence = Some(if e.name == "ext-a" { expr.to_string() } else { "MIT".to_string() });
        }
        lines(&check(&c, &allow, &spdx_list))
    };
    assert_eq!(with_licence("MIT OR GPL-3.0-only"), Vec::<String>::new());
    assert_eq!(
        with_licence("GPL-3.0-only"),
        ["KNP1-ALLOW-LICENCE ext-a 1.2.0 ext-a 1.2.0 licence `GPL-3.0-only` is not allowed"]
    );
    assert_eq!(
        with_licence("MIT AND GPL-3.0-only"),
        ["KNP1-ALLOW-LICENCE ext-a 1.2.0 ext-a 1.2.0 licence `MIT AND GPL-3.0-only` is not allowed"]
    );
    // Missing or unparseable expressions fail closed.
    let mut c = closure.clone();
    for e in c.crates.iter_mut() {
        e.licence = if e.name == "ext-m" { None } else { Some("MIT".to_string()) };
    }
    let want = "KNP1-ALLOW-LICENCE ext-m 0.1.0 ext-m 0.1.0 has no licence expression";
    assert_eq!(lines(&check(&c, &allow, &spdx_list)), [want]);
    for e in c.crates.iter_mut() {
        e.licence = Some("MIT OR".to_string());
    }
    let out = check(&c, &allow, &spdx_list);
    assert_eq!(out.len(), 3, "every third-party crate, not the workspace root: {out:?}");
    assert!(out.iter().all(|f| f.message.contains("does not parse")));
}

#[test]
fn ac4_missing_local_source_fails_without_report() {
    let err = run(&fix("unresolved/knp1-lint.toml")).unwrap_err();
    assert_eq!(err.code, GraphErrorCode::Unresolved);
    assert_eq!(err.code.as_str(), "KNP1-GRAPH-UNRESOLVED");
    assert!(err.message.contains("ext-r 1.0.0"), "{err}");
}

#[test]
fn ac5_excluding_src_path_rejected() {
    let err = run(&fix("exclude-src/knp1-lint.toml")).unwrap_err();
    assert_eq!(err.code, GraphErrorCode::ConfigExclude);
    assert_eq!(err.code.as_str(), "KNP1-CONFIG-EXCLUDE");
    assert!(err.message.contains("src/solver.rs"), "{err}");
}

#[test]
fn req005_exclusion_globs() {
    for ok in ["tests/**", "benches/*.rs", "examples/**", "src/bin/**", "src/bin/tool.rs", "tests/a/**/b.rs"] {
        assert!(glob::exclusion_allowed(ok), "{ok}");
    }
    for bad in [
        "src/solver.rs",
        "src/**",
        "**/tests/**",
        "*/tests/**",
        "tests",
        "src/bin",
        "src/binary/**",
        "tests/../src/x.rs",
        "/tests/**",
        "tests\\x.rs",
        "build.rs",
        "",
    ] {
        assert!(!glob::exclusion_allowed(bad), "{bad}");
        let cfg = format!("[graph]\nroots = [\"a\"]\nexclude = [{bad:?}]\n");
        assert_eq!(parse_config(&cfg, Path::new(".")).unwrap_err().code, GraphErrorCode::ConfigExclude, "{bad}");
    }
    assert!(glob::matches("tests/**", "tests/a/b.rs"));
    assert!(glob::matches("src/bin/**", "src/bin/tool.rs"));
    assert!(glob::matches("benches/*.rs", "benches/b.rs"));
    assert!(!glob::matches("benches/*.rs", "benches/x/b.rs"));
    assert!(!glob::matches("tests/**", "src/tests/a.rs"));
    assert!(glob::matches("tests/a/**/b.rs", "tests/a/b.rs"));
    assert!(glob::matches("tests/a?.rs", "tests/ab.rs"));
}

#[test]
fn config_errors_fail_closed() {
    let dir = Path::new(".");
    let code = |t: &str| parse_config(t, dir).unwrap_err().code;
    assert_eq!(code("roots = [\"a\"]\n"), GraphErrorCode::ConfigInvalid);
    assert_eq!(code("[graph]\nroots = []\n"), GraphErrorCode::ConfigInvalid);
    assert_eq!(code("[graph]\nroots = [\"a\"]\nunknown = 1\n"), GraphErrorCode::ConfigInvalid);
    assert_eq!(code("[graph]\nroots = [\"a\"]\nthird_party_kind = \"git\"\n"), GraphErrorCode::ConfigInvalid);
    // Other modules' tables are not this module's business.
    let cfg = parse_config("[graph]\nroots = [\"b\", \"a\", \"a\"]\n[flags]\nanything = true\n", dir).unwrap();
    assert_eq!(cfg.roots, ["a", "b"]);
    assert_eq!(cfg.third_party_kind, SourceKind::Vendored);

    assert!(parse_allowlist("").unwrap().is_empty());
    for bad in [
        "[[allow]]\nname = \"a\"\nversion = \"1\"\nreason = \"r\"\n",
        "[[allow]]\nname = \"a\"\nversion = \"1\"\nreason = \" \"\nreviewer = \"x\"\n",
        "[[allow]]\nname = \"a\"\nversion = \"not-a-req\"\nreason = \"r\"\nreviewer = \"x\"\n",
        "[[allow]]\nname = \"a\"\nversion = \"1\"\nreason = \"r\"\nreviewer = \"x\"\nextra = 1\n",
        "[other]\n",
    ] {
        assert_eq!(parse_allowlist(bad).unwrap_err().code, GraphErrorCode::ConfigInvalid, "{bad}");
    }
    assert_eq!(parse_spdx_allowlist("licences = [\"MIT\"]\n").unwrap_err().code, GraphErrorCode::ConfigInvalid);
    let s = parse_spdx_allowlist(&String::from_utf8(read("ok/spdx-allowlist.toml")).unwrap()).unwrap();
    assert_eq!(s.licences, set(&["Apache-2.0", "MIT"]));
}

#[test]
fn spdx_evaluation_and_bounds() {
    let lic = set(&["MIT", "Apache-2.0"]);
    let exc = set(&["LLVM-exception"]);
    let ev = |e: &str| spdx::evaluate(e, &lic, &exc);
    assert_eq!(ev("MIT"), Ok(true));
    assert_eq!(ev("MIT OR GPL-3.0-only"), Ok(true));
    assert_eq!(ev("GPL-3.0-only OR MIT"), Ok(true));
    assert_eq!(ev("GPL-3.0-only"), Ok(false));
    assert_eq!(ev("MIT AND GPL-3.0-only"), Ok(false));
    assert_eq!(ev("MIT AND Apache-2.0"), Ok(true));
    assert_eq!(ev("MIT/Apache-2.0"), Ok(true));
    assert_eq!(ev("Apache-2.0 WITH LLVM-exception"), Ok(true));
    assert_eq!(ev("Apache-2.0 WITH Classpath-exception-2.0"), Ok(false));
    // AND binds tighter than OR: GPL OR (MIT AND Apache) is allowed; (GPL OR MIT) AND GPL is not.
    assert_eq!(ev("GPL-3.0-only OR MIT AND Apache-2.0"), Ok(true));
    assert_eq!(ev("(GPL-3.0-only OR MIT) AND GPL-3.0-only"), Ok(false));
    assert_eq!(ev("MIT AND GPL-3.0-only OR Apache-2.0"), Ok(true));
    // Lower-case operators and `+` suffixes are not the allowed IDs.
    assert_eq!(ev("mit or GPL-3.0-only"), Err("unexpected token after position 1".to_string()));
    assert_eq!(ev("MIT+"), Ok(false));
    for bad in ["", "MIT OR", "(MIT", "MIT)", "MIT WITH", "(MIT OR Apache-2.0) WITH LLVM-exception", "MIT;"] {
        assert!(ev(bad).is_err(), "{bad:?}");
    }
    let deep = format!("{}MIT{}", "(".repeat(40), ")".repeat(40));
    assert!(ev(&deep).is_err());
    let long = vec!["MIT"; 300].join(" OR ");
    assert!(ev(&long).is_err());
}

#[test]
fn semver_requirements() {
    let m = |req: &str, v: &str| {
        let r = semver::VersionReq::parse(req).unwrap_or_else(|| panic!("req {req}"));
        r.matches(&semver::Version::parse(v).unwrap_or_else(|| panic!("version {v}")))
    };
    assert!(m("^1.2", "1.2.0") && m("^1.2", "1.9.3") && !m("^1.2", "2.0.0") && !m("^1.2", "1.1.9"));
    assert!(m("1.2", "1.5.0") && !m("1.2", "2.0.0"));
    assert!(m("^0.3", "0.3.7") && !m("^0.3", "0.4.0"));
    assert!(m("^0.0.3", "0.0.3") && !m("^0.0.3", "0.0.4"));
    assert!(m("~0.3", "0.3.1") && !m("~0.3", "0.4.0"));
    assert!(m("~1.2.3", "1.2.9") && !m("~1.2.3", "1.3.0") && !m("~1.2.3", "1.2.2"));
    assert!(m("=0.1.0", "0.1.0") && !m("=0.1.0", "0.1.1"));
    assert!(m(">=0.3.0, <0.4.0", "0.3.1") && !m(">=0.3.0, <0.4.0", "0.4.0"));
    assert!(m(">1.2", "1.3.0") && !m(">1.2", "1.2.9"));
    assert!(m("<=1.2", "1.2.9") && !m("<=1.2", "1.3.0"));
    assert!(m("1.*", "1.9.0") && !m("1.*", "2.0.0"));
    assert!(m("1.2.*", "1.2.5") && !m("1.2.*", "1.3.0"));
    assert!(m("*", "7.0.0") && !m("*", "7.0.0-rc.1"));
    assert!(!m("^1.2", "1.3.0-alpha"));
    assert!(m(">=1.3.0-alpha", "1.3.0-beta") && !m(">=1.3.0-alpha", "1.4.0-beta"));
    assert!(m("^2", "2.0.0+build.5"));
    for bad in ["", "abc", "^1.x.2", "1.2.3.4", ">=*", "01.2"] {
        assert!(semver::VersionReq::parse(bad).is_none(), "{bad:?}");
    }
    assert!(semver::Version::parse("1.2").is_none());
}

#[test]
fn findings_are_sorted() {
    let cfg = load_config(&fix("ok/knp1-lint.toml")).unwrap();
    let closure = compute_closure(&cfg).unwrap();
    let out = check(&closure, &[], &SpdxAllowlist::default());
    let got = lines(&out);
    assert_eq!(
        got,
        [
            "KNP1-ALLOW-LICENCE ext-a 1.2.0 ext-a 1.2.0 licence `MIT OR GPL-3.0-only` is not allowed",
            "KNP1-ALLOW-UNLISTED ext-a 1.2.0 ext-a via fid-num -> ext-a",
            "KNP1-ALLOW-LICENCE ext-b 0.3.1 ext-b 0.3.1 licence `Apache-2.0 WITH LLVM-exception` is not allowed",
            "KNP1-ALLOW-UNLISTED ext-b 0.3.1 ext-b via fid-num -> ext-a -> ext-b",
            "KNP1-ALLOW-LICENCE ext-m 0.1.0 ext-m 0.1.0 licence `MIT` is not allowed",
            "KNP1-ALLOW-UNLISTED ext-m 0.1.0 ext-m via fid-num -> ext-a -> ext-m",
        ]
    );
    for f in &out {
        f.validate().unwrap();
    }
}

#[test]
fn wide_renames_inheritance_targets_git_registry_and_two_versions() {
    let r = run(&fix("wide/knp1-lint.toml")).unwrap();
    let bytes = r.closure.to_canonical_bytes().unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), String::from_utf8(read("wide/expected-closure.json")).unwrap());
    assert_eq!(lines(&r.findings), expected_lines("wide/expected-findings.txt"));
    // fid-k's file list skips the nested closure crate ext-win.
    let fid_k = r.closure.crates.iter().find(|c| c.name == "fid-k").unwrap();
    assert_eq!(fid_k.files, ["src/lib.rs"]);
}

#[test]
fn non_member_cannot_inherit_version() {
    let err = run(&fix("inherit-version/knp1-lint.toml")).unwrap_err();
    assert_eq!(err.code, GraphErrorCode::Unresolved);
    assert!(err.message.contains("not a member"), "{err}");
}

#[test]
fn nested_package_reached_by_path_attribute_is_scanned() {
    let r = run(&fix("ok/knp1-lint.toml")).unwrap();
    let ext_a = r.closure.crates.iter().find(|c| c.name == "ext-a").unwrap();
    assert!(ext_a.files.iter().any(|f| f == "src/nested/helper.rs"), "{:?}", ext_a.files);
}

#[test]
fn spdx_slash_only_in_plain_lists() {
    let lic = set(&["MIT", "Apache-2.0"]);
    let exc = set(&["LLVM-exception"]);
    let ev = |e: &str| spdx::evaluate(e, &lic, &exc);
    assert_eq!(ev("MIT/Apache-2.0"), Ok(true));
    assert_eq!(ev("GPL-3.0-only/MIT"), Ok(true));
    assert_eq!(ev("GPL-3.0-only/LGPL-2.1-only/MIT"), Ok(true));
    assert_eq!(ev("GPL-3.0-only/AGPL-3.0-only"), Ok(false));
    for bad in [
        "MIT/Apache-2.0 AND GPL-3.0-only",
        "GPL-3.0-only AND MIT/Apache-2.0",
        "MIT OR Apache-2.0/GPL-3.0-only",
        "Apache-2.0 WITH LLVM-exception/MIT",
        "(MIT/Apache-2.0)",
        "MIT/",
        "/MIT",
        "MIT//Apache-2.0",
    ] {
        assert!(ev(bad).is_err(), "{bad:?}");
    }
}

/// The real workspace's kernel (layers `num` and `kernel` in layers.toml) is all first-party today.
#[test]
fn real_workspace_kernel_closure_is_first_party() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let layers = std::fs::read_to_string(repo.join("layers.toml")).unwrap();
    let roots: Vec<String> = layers
        .lines()
        .filter(|l| l.ends_with("= \"num\"") || l.ends_with("= \"kernel\""))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect();
    assert!(roots.iter().any(|r| r == "fid-num"), "{roots:?}");
    let list: Vec<String> = roots.iter().map(|r| format!("{r:?}")).collect();
    let cfg = parse_config(&format!("[graph]\nroots = [{}]\n", list.join(", ")), &repo).unwrap();
    let closure = compute_closure(&cfg).unwrap();
    assert!(closure.crates.iter().all(|c| c.source_kind == SourceKind::Workspace), "{closure:?}");
    let got: BTreeSet<&str> = closure.crates.iter().map(|c| c.name.as_str()).collect();
    let want: BTreeSet<&str> = roots.iter().map(String::as_str).collect();
    assert_eq!(got, want);
    assert!(check(&closure, &[], &SpdxAllowlist::default()).is_empty());
}
