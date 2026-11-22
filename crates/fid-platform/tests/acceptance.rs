//! Acceptance criteria of spec node PLAT.MATRIX.FILE (AC-1 .. AC-7). Expected diagnostics
//! and lookups come from the independent Python reference
//! `fixtures/plat/matrix/make_expected.py` (recorded in `expected.toml`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::matrix::{
    parse, validate, Arch, EffectiveTier, Limit, MatrixError, OsFamily, Phase, Problem, Tier, Version, MAX_BYTES,
    MAX_ROWS,
};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/plat/matrix").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

/// Runs `fid-platform plat-matrix validate <fixture>` (what `cargo xtask plat-matrix validate`
/// runs; the wrapper itself is exercised by the rust.yml step for AC-1); returns (exit code, stdout).
fn cli(name: &str) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .args(["plat-matrix", "validate"])
        .arg(fixture(name))
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn expected() -> toml::Table {
    std::fs::read_to_string(fixture("expected.toml")).unwrap().parse().unwrap()
}

fn tables<'a>(t: &'a toml::Table, key: &str) -> Vec<&'a toml::Table> {
    t[key].as_array().unwrap().iter().map(|v| v.as_table().unwrap()).collect()
}

fn s<'a>(t: &'a toml::Table, key: &str) -> &'a str {
    t[key].as_str().unwrap()
}

#[test]
fn ac1_valid_fixture_cli_exits_0() {
    let (code, out) = cli("valid.toml");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "6 rows, 0 errors\n");
    let m = parse(&read("valid.toml")).unwrap();
    assert_eq!(m.rows.len(), 6);
    let oses: Vec<OsFamily> = m.rows.iter().map(|r| r.os).collect();
    for os in [OsFamily::Windows, OsFamily::Ubuntu, OsFamily::Macos] {
        assert!(oses.contains(&os));
    }
}

#[test]
fn ac2_overlap_names_rows_2_and_3() {
    let (code, out) = cli("overlap.toml");
    assert_eq!(code, 1);
    assert_eq!(out, "OverlappingRange: rows 2 and 3: ubuntu x86_64 version ranges overlap\n3 rows, 1 errors\n");
    let v = validate(&read("overlap.toml"));
    assert_eq!(v.errors, vec![MatrixError::OverlappingRange { a: 2, b: 3, os: OsFamily::Ubuntu, arch: Arch::X86_64 }]);
    assert!(v.matrix.is_none());
}

#[test]
fn ac3_bad_phase_names_the_row() {
    let (code, out) = cli("bad-phase.toml");
    assert_eq!(code, 1);
    assert!(out.starts_with("BadPhase: row 2:"), "{out}");
    let v = validate(&read("bad-phase.toml"));
    assert_eq!(v.errors, vec![MatrixError::BadPhase { row: 2, problem: Problem::Invalid }]);
}

#[test]
fn ac4_lookup() {
    let m = parse(&read("valid.toml")).unwrap();
    assert_eq!(m.lookup("macos", "15.1", "aarch64", Phase::P0), EffectiveTier::Planned(Tier::Tier1, Phase::P1a));
    assert_eq!(m.lookup("macos", "15.1", "aarch64", Phase::P1a), EffectiveTier::Tier(Tier::Tier1));
    assert_eq!(m.lookup("freebsd", "14", "x86_64", Phase::P1a), EffectiveTier::NotListed);
}

#[test]
fn ac4_lookups_match_python_reference() {
    let m = parse(&read("valid.toml")).unwrap();
    let exp = expected();
    let cases = tables(&exp, "lookup");
    assert!(cases.len() >= 20);
    for c in cases {
        let phase = Phase::from_name(s(c, "phase")).unwrap();
        let got = m.lookup(s(c, "os"), s(c, "version"), s(c, "arch"), phase).to_string();
        assert_eq!(got, s(c, "expect"), "{c:?}");
    }
}

/// Deterministic generator (64-bit LCG, Knuth MMIX constants) for the oversized file.
fn generated_rows(seed: u64, rows: usize) -> String {
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        x >> 33
    };
    let mut out = String::from("schema_version = 1\n");
    for _ in 0..rows {
        let os = ["windows", "ubuntu", "macos"][(next() % 3) as usize];
        let v = next() % 1000;
        out.push_str(&format!(
            "[[row]]\nos=\"{os}\"\nversion_min=\"{v}\"\narch=\"x86_64\"\ntier=\"tier2\"\nsince_phase=\"P0\"\nnotes=\"\"\n"
        ));
    }
    out
}

#[test]
fn ac5_ten_thousand_and_one_rows_is_too_large() {
    let text = generated_rows(1, MAX_ROWS + 1);
    assert!(text.len() <= MAX_BYTES, "generator must exercise the row limit, not the byte limit");
    let start = Instant::now();
    let r = parse(text.as_bytes());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(r, Err(MatrixError::TooLarge { limit: Limit::Rows, found: MAX_ROWS + 1, max: MAX_ROWS }));
}

#[test]
fn ac5_more_than_one_mib_is_too_large() {
    let mut text = String::from("schema_version = 1\n");
    while text.len() <= MAX_BYTES {
        text.push_str("# padding padding padding padding padding padding padding padding\n");
    }
    let r = parse(text.as_bytes());
    assert_eq!(r, Err(MatrixError::TooLarge { limit: Limit::Bytes, found: text.len(), max: MAX_BYTES }));
}

#[test]
fn ac6_future_schema_is_unsupported() {
    assert_eq!(parse(&read("future-schema.toml")), Err(MatrixError::UnsupportedSchema { found: 99 }));
    let v = validate(&read("future-schema.toml"));
    assert!(v.matrix.is_none());
    assert_eq!(v.errors.len(), 1);
}

#[test]
fn ac7_overlap_diagnostics_are_byte_identical() {
    let a = validate(&read("overlap.toml")).render();
    let b = validate(&read("overlap.toml")).render();
    assert_eq!(a.as_bytes(), b.as_bytes());
    assert_eq!(cli("overlap.toml"), cli("overlap.toml"));
}

#[test]
fn all_fixtures_match_python_reference() {
    let exp = expected();
    let cases = tables(&exp, "validate");
    assert_eq!(cases.len(), 5);
    for c in cases {
        let file = s(c, "file");
        let errors: Vec<&str> = c["errors"].as_array().unwrap().iter().map(|e| e.as_str().unwrap()).collect();
        let rows = c["rows"].as_integer().unwrap();
        let mut want = String::new();
        for e in &errors {
            want.push_str(e);
            want.push('\n');
        }
        want.push_str(&format!("{rows} rows, {} errors\n", errors.len()));
        assert_eq!(validate(&read(file)).render(), want, "{file}");
        let (code, out) = cli(file);
        assert_eq!(out, want, "{file}");
        assert_eq!(code, i32::from(!errors.is_empty()), "{file}");
    }
}

#[test]
fn repository_support_matrix_is_valid_per_d010() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../platform/support-matrix.toml");
    let m = parse(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(m.rows.len(), 9, "D-010 matrix: 6 Tier 1, 2 Tier 2 (Ubuntu aarch64), 1 Tier 3 (Win 11 aarch64)");
    for r in &m.rows {
        assert!(r.version_max.is_some(), "every row carries a bounded version range");
        assert!(r.notes.contains("[unverified]"), "exact P1a OS release versions are marked [unverified]");
        assert_eq!(r.since_phase, Phase::P1a, "no row may claim a tier before P1a");
    }
    assert_eq!(m.lookup("ubuntu", "26.04.1", "x86_64", Phase::P0), EffectiveTier::Planned(Tier::Tier1, Phase::P1a));
    assert_eq!(m.lookup("windows", "10.0.26100", "x86_64", Phase::P0), EffectiveTier::Planned(Tier::Tier1, Phase::P1a));
    assert_eq!(m.lookup("macos", "26.7.2", "aarch64", Phase::P0), EffectiveTier::Planned(Tier::Tier1, Phase::P1a));
    assert_eq!(m.lookup("ubuntu", "24.04", "aarch64", Phase::P1a), EffectiveTier::Tier(Tier::Tier2));
    assert_eq!(m.lookup("windows", "10.0.26100", "aarch64", Phase::P1a), EffectiveTier::Tier(Tier::Tier3));
}

#[test]
fn versions_compare_numerically_with_zero_padding() {
    let v = |s: &str| Version::parse(s).unwrap();
    assert_eq!(v("22.04"), v("22.4"));
    assert_eq!(v("26"), v("26.0.0.0"));
    assert!(v("10.0.19041") < v("10.0.22000"));
    assert!(v("9.10") > v("9.9"));
    assert_eq!(v("4294967295").components(), &[u32::MAX]);
    assert_eq!(v("10.0.19041").to_string(), "10.0.19041");
    for bad in ["", ".", "1.", ".1", "1..2", "1.2.3.4.5", "-1", "+1", " 1", "1 ", "4294967296", "1e3", "١"] {
        assert!(Version::parse(bad).is_none(), "{bad:?}");
    }
}

#[test]
fn names_round_trip() {
    for p in Phase::ALL {
        assert_eq!(Phase::from_name(p.as_str()), Some(*p));
    }
    assert!(Phase::P0 < Phase::P1a && Phase::P4 < Phase::Later);
    for os in OsFamily::ALL {
        assert_eq!(OsFamily::from_name(os.as_str()), Some(*os));
    }
    assert_eq!(Arch::from_name("arm64"), None);
}

#[test]
fn hostile_input_fails_closed() {
    let syntax = validate(b"schema_version = 1\n[[row]\nos = 'x'\n");
    assert!(matches!(syntax.errors.as_slice(), [MatrixError::Parse { line: Some(_), .. }]), "{:?}", syntax.errors);
    let utf8 = validate(&[0xff, 0xfe, 0x00]);
    assert!(matches!(utf8.errors.as_slice(), [MatrixError::Parse { line: None, .. }]));
    let dup = validate(b"schema_version = 1\nschema_version = 1\n");
    assert!(matches!(dup.errors.as_slice(), [MatrixError::Parse { .. }]));
    let empty = validate(b"");
    assert_eq!(
        empty.errors,
        vec![MatrixError::BadField { row: None, field: "schema_version".into(), msg: "is missing" }]
    );
    let huge = validate(b"schema_version = 9223372036854775807\n");
    assert_eq!(huge.errors, vec![MatrixError::UnsupportedSchema { found: 9_223_372_036_854_775_807 }]);
    let not_tables = validate(b"schema_version = 1\nrow = [1, 2]\n");
    assert_eq!(not_tables.rows, 2);
    assert_eq!(not_tables.errors.len(), 2);
    // Spaced `[[ row ]]` headers count towards the pre-parse row limit too.
    let spaced = "schema_version = 1\n".to_string() + &"[[ row ]]\n".repeat(MAX_ROWS + 1);
    assert!(matches!(parse(spaced.as_bytes()), Err(MatrixError::TooLarge { limit: Limit::Rows, .. })));
}

#[test]
fn many_overlaps_are_capped() {
    let row =
        "[[row]]\nos=\"ubuntu\"\nversion_min=\"1\"\narch=\"x86_64\"\ntier=\"tier1\"\nsince_phase=\"P0\"\nnotes=\"\"\n";
    let text = "schema_version = 1\n".to_string() + &row.repeat(100);
    let v = validate(text.as_bytes());
    assert_eq!(v.errors.len(), fid_platform::matrix::MAX_ERRORS + 1);
    assert_eq!(v.errors.last(), Some(&MatrixError::TooManyErrors { limit: fid_platform::matrix::MAX_ERRORS }));
    assert_eq!(validate(text.as_bytes()).render(), v.render());
}
