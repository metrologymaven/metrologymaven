//! Acceptance criteria of spec node ECO.DATA.CAT (AC-1 .. AC-5), plus the requirements
//! the criteria do not exercise directly. Expected findings come from
//! `fixtures/datasets/expected.txt`, labelled by hand and confirmed by the independent
//! Python checker in `fixtures/datasets/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_data::cat::{check, check_files, parse_domains, parse_licence, Finding, MAX_INPUT_BYTES};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/datasets")
}

fn domains() -> BTreeSet<String> {
    parse_domains(&std::fs::read_to_string(fixtures().join("domains.txt")).unwrap()).unwrap()
}

/// `<CODE> <subject>` lines expected for one fixture (empty for `ok`).
fn expected(name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(fixtures().join("expected.txt")).unwrap();
    let rows: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(f, _)| *f == name)
        .map(|(_, v)| v)
        .collect();
    assert!(!rows.is_empty(), "{name} is not in expected.txt");
    rows.into_iter().filter(|v| *v != "ok").map(String::from).collect()
}

fn short(f: &[Finding]) -> Vec<String> {
    f.iter().map(|f| format!("{} {}", f.code, f.subject)).collect()
}

fn check_fixture(name: &str) -> Vec<Finding> {
    let got = check_files(&fixtures().join(name), &fixtures().join("domains.txt"));
    assert_eq!(short(&got), expected(name), "{name}: {got:#?}");
    // REQ-005 / D0: same output on every run.
    assert_eq!(check_files(&fixtures().join(name), &fixtures().join("domains.txt")), got, "{name}");
    got
}

/// Runs the `datasets-check` binary on a fixture; returns (exit code, stdout lines).
fn run_cli(name: &str) -> (i32, Vec<String>) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-data"))
        .arg("datasets-check")
        .arg("--catalogue")
        .arg(fixtures().join(name))
        .arg("--domains")
        .arg(fixtures().join("domains.txt"))
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    (out.status.code().unwrap(), stdout.lines().map(String::from).collect())
}

fn assert_cli(name: &str) {
    let want = expected(name);
    let (code, lines) = run_cli(name);
    if want.is_empty() {
        assert_eq!((code, lines), (0, vec!["datasets-check: ok".to_string()]), "{name}");
    } else {
        assert_eq!(code, 1, "{name}: {lines:?}");
        assert_eq!(lines.len(), want.len(), "{name}: {lines:?}");
        for (line, w) in lines.iter().zip(&want) {
            assert!(line.starts_with(&format!("{w}: ")), "{name}: {line:?} does not start with {w:?}");
        }
    }
}

/// AC-1: three admitted entries (CC-BY-4.0 mirror, LicenseRef fetch-only, CC0-1.0 mirror).
#[test]
fn ac1_valid_catalogue_exits_0() {
    assert!(check_fixture("valid.toml").is_empty());
    assert_cli("valid.toml");
}

/// AC-2: a LicenseRef-Vendor-EULA entry marked mirror.
#[test]
fn ac2_mirror_without_redistributable_licence() {
    check_fixture("mirror-noredist.toml");
    assert_cli("mirror-noredist.toml");
}

/// AC-3: an entry declaring standards text.
#[test]
fn ac3_standards_text() {
    check_fixture("standards.toml");
    assert_cli("standards.toml");
}

/// AC-4: a 63-character digest, and a duplicate ID.
#[test]
fn ac4_bad_digest_and_duplicate() {
    let f = check_fixture("bad-digest.toml");
    assert!(f[0].detail.contains("sha256"), "{f:?}");
    assert_cli("bad-digest.toml");
    let f = check_fixture("dup.toml");
    assert!(f[0].detail.contains("lines"), "{f:?}");
    assert_cli("dup.toml");
}

/// AC-5: owner `XYZ` is not a level-1 domain.
#[test]
fn ac5_unknown_owner() {
    check_fixture("bad-owner.toml");
    assert_cli("bad-owner.toml");
}

/// REQ-001: each schema rule on its own entry.
#[test]
fn req1_schema_rules() {
    let f = check_fixture("schema.toml");
    let detail = |s: &str| f.iter().find(|x| x.subject == s).map(|x| x.detail.clone()).unwrap();
    for (subject, field) in [
        ("Bad_ID", "id"),
        ("dataset#2", "id"),
        ("no-urls", "urls"),
        ("empty-urls", "urls"),
        ("ref-no-terms", "terms_url"),
        ("bad-redist", "redistribution"),
        ("unknown-field", "colour"),
        ("negative-size", "size_bytes"),
        ("bad-expression", "licence"),
        ("upper-digest", "sha256"),
        ("bad-url", "urls"),
    ] {
        assert!(detail(subject).contains(field), "{subject}: {}", detail(subject));
    }
}

/// REQ-002: OR needs any side, AND needs both, LicenseRef only if listed, WITH is one term.
#[test]
fn req2_licence_expressions() {
    check_fixture("expressions.toml");
    let allowed: BTreeSet<String> = ["MIT", "CC0-1.0", "LicenseRef-Ok"].iter().map(|s| s.to_string()).collect();
    for (expr, ok) in [
        ("MIT", true),
        ("GPL-3.0-only", false),
        ("MIT OR GPL-3.0-only", true),
        ("MIT AND GPL-3.0-only", false),
        ("GPL-3.0-only OR MIT AND CC0-1.0", true),
        ("(GPL-3.0-only OR MIT) AND CC0-1.0", true),
        ("(GPL-3.0-only OR MIT) AND GPL-2.0-only", false),
        ("LicenseRef-Ok", true),
        ("LicenseRef-Other", false),
        ("MIT WITH Some-exception", false),
    ] {
        assert_eq!(parse_licence(expr).unwrap().satisfiable(&allowed), ok, "{expr}");
    }
    for bad in ["", "MIT OR", "AND MIT", "(MIT", "MIT)", "MIT or CC0-1.0", "MIT WITH", "M/T", "MIT CC0-1.0"] {
        assert!(parse_licence(bad).is_none(), "{bad:?} should not parse");
    }
    // Bounded nesting: no stack overflow on hostile input.
    let deep = format!("{}MIT{}", "(".repeat(100_000), ")".repeat(100_000));
    assert!(parse_licence(&deep).is_none());
    let ok_depth = format!("{}MIT{}", "(".repeat(32), ")".repeat(32));
    assert!(parse_licence(&ok_depth).is_some());
}

/// REQ-003 (second half): customer data needs a release_ref.
#[test]
fn req3_customer_data() {
    check_fixture("customer.toml");
}

/// REQ-005: several findings, entries out of file order, printed by subject then code.
#[test]
fn req5_sorted_findings() {
    let f = check_fixture("multi.toml");
    let mut sorted = f.clone();
    sorted.sort();
    assert_eq!(f, sorted);
    assert_cli("multi.toml");
}

/// A file outside the supported TOML subset, or without a policy, is one file-level finding.
#[test]
fn file_level_schema_errors() {
    check_fixture("syntax.toml");
    check_fixture("no-policy.toml");
    let d = domains();
    let policy = "[policy]\nredistributable = [\"MIT\"]\n";
    for (text, why) in [
        (format!("{policy}[[dataset]]\nsize_bytes = 1.5\n"), "float"),
        (format!("{policy}[[dataset]]\nid = \"a\"\nid = \"b\"\n"), "duplicate key"),
        (format!("{policy}[other]\n"), "unknown table"),
        (format!("{policy}[policy]\n"), "policy twice"),
        (format!("x = 1\n{policy}"), "key outside a table"),
        (format!("{policy}[[dataset]]\nurls = [[\"a\"]]\n"), "nested array"),
        (format!("{policy}[[dataset]]\nnotes = \"\"\"x\"\"\"\n"), "multi-line string"),
        (format!("{policy}[[dataset]]\nnotes = \"\\q\"\n"), "bad escape"),
        (format!("{policy}[[dataset]]\nsize_bytes = 99999999999999999999\n"), "integer overflow"),
        (format!("{policy}[[dataset]]\nsize_bytes = 01\n"), "leading zero"),
        (format!("{policy}[[dataset]]\na.b = 1\n"), "dotted key"),
        ("[policy]\nredistributable = \"MIT\"\n".to_string(), "policy not an array"),
        ("[policy]\nredistributable = []\nextra = 1\n".to_string(), "unknown policy key"),
    ] {
        let f = check(&text, &d);
        assert_eq!(short(&f), vec!["DATA-SCHEMA -".to_string()], "{why}: {f:?}");
    }
}

/// The subset reader handles the TOML forms the catalogue uses.
#[test]
fn toml_subset_forms() {
    let d = domains();
    let entry = |extra: &str| {
        format!(
            "[policy] # comment\r\nredistributable = [ \"MIT\" , ]\r\n\r\n[[ dataset ]]\r\nid = 'abc'\r\n\
             title = \"T \\u00e9 \\U0001F600 \\\\ \\\"\"\r\nsource = \"https://e.org/x\"\r\nlicence = \"MIT\"\r\n\
             size_bytes = +1_024\r\nsha256 = \"{}\"\r\nowner = \"KRN\"\r\nredistribution = \"mirror\"\r\n\
             urls = [\r\n  # first\r\n  \"https://e.org/a\", 'https://e.org/b'\r\n]\r\n{extra}",
            "a".repeat(64)
        )
    };
    assert_eq!(check(&entry(""), &d), Vec::<Finding>::new());
    assert_eq!(short(&check(&entry("contains_customer_data = \"yes\"\n"), &d)), vec!["DATA-SCHEMA abc"]);
    assert_eq!(short(&check(&entry("release_ref = 1\n"), &d)), vec!["DATA-SCHEMA abc"]);
    assert_eq!(short(&check(&entry("size_bytes = 2\n"), &d)), vec!["DATA-SCHEMA -"]);
}

/// DATA-IO: missing files, oversized input, bad domain list; and usage errors exit 2.
#[test]
fn io_errors() {
    let dir = std::env::temp_dir().join(format!("fid-data-cat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let valid = fixtures().join("valid.toml");
    let doms = fixtures().join("domains.txt");

    let f = check_files(&dir.join("missing.toml"), &doms);
    assert_eq!(short(&f), vec!["DATA-IO -"]);

    let big = dir.join("big.toml");
    let mut text = std::fs::read_to_string(&valid).unwrap();
    while (text.len() as u64) <= MAX_INPUT_BYTES {
        text.push_str("# padding padding padding padding padding padding padding padding\n");
    }
    std::fs::write(&big, &text).unwrap();
    assert_eq!(short(&check_files(&big, &doms)), vec!["DATA-IO -"]);
    assert_eq!(short(&check(&text, &domains())), vec!["DATA-IO -"]);

    let bad_domains = dir.join("domains.txt");
    std::fs::write(&bad_domains, "KRN\nKRN.NUM\n").unwrap();
    assert_eq!(short(&check_files(&valid, &bad_domains)), vec!["DATA-IO -"]);

    let not_utf8 = dir.join("latin1.toml");
    std::fs::write(&not_utf8, b"[policy]\nredistributable = [\"\xe9\"]\n").unwrap();
    assert_eq!(short(&check_files(&not_utf8, &doms)), vec!["DATA-IO -"]);

    let status = Command::new(env!("CARGO_BIN_EXE_fid-data")).arg("datasets-check").status().unwrap();
    assert_eq!(status.code(), Some(2));
    let status = Command::new(env!("CARGO_BIN_EXE_fid-data")).arg("nope").status().unwrap();
    assert_eq!(status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every fixture listed in expected.txt is exercised (none forgotten).
#[test]
fn all_fixtures_checked() {
    let text = std::fs::read_to_string(fixtures().join("expected.txt")).unwrap();
    let names: BTreeSet<&str> =
        text.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once('\t')).map(|(f, _)| f).collect();
    assert_eq!(names.len(), 12);
    for n in names {
        check_fixture(n);
    }
}
