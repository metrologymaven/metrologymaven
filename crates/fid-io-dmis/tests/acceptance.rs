//! Acceptance criteria of spec node IO.DMIS.CONF (AC-1 .. AC-4), plus the error paths of the
//! declaration format. Expected outputs come from `fixtures/io/dmis/conf/make_expected.py`,
//! an independent Python reference, never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_io_dmis::conf::{
    generate, generate_files, parse_declarations, parse_inventory, stale_files, write_outputs, ConfError, Support,
    JSON_FILE, MD_FILE,
};
use fid_io_dmis::inventory::{inventory_text, qualified_ids, test_binaries, TestBinary};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/dmis/conf")
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fid-io-dmis-conf").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dmis-conformance"))
}

fn small() -> fid_io_dmis::conf::Outputs {
    generate_files(&fixtures().join("decl_small.toml"), &fixtures().join("tests.txt")).unwrap()
}

fn field<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(key).unwrap(),
        _ => panic!("not an object"),
    }
}

fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string"),
    }
}

#[test]
fn ac1_small_declarations_give_a_full_and_b_partial() {
    let out = small();
    let json = parse(&out.json).unwrap();
    let CValue::Arr(levels) = field(&json, "module_levels") else { panic!("module_levels") };
    let got: Vec<(&str, &str, &str)> =
        levels.iter().map(|l| (text(field(l, "module")), text(field(l, "import")), text(field(l, "export")))).collect();
    assert_eq!(got, vec![("A", "full", "none"), ("B", "partial", "none")]);
    let expected_md = std::fs::read_to_string(fixtures().join("expected.md")).unwrap();
    assert_eq!(out.markdown, expected_md);
    let expected_json = std::fs::read(fixtures().join("expected.json")).unwrap();
    assert_eq!(out.json, expected_json);
}

#[test]
fn ac1_binary_writes_the_expected_files() {
    let dir = tmp("ac1");
    let st = bin()
        .args(["--decl", fixtures().join("decl_small.toml").to_str().unwrap()])
        .args(["--tests", fixtures().join("tests.txt").to_str().unwrap()])
        .args(["--out-dir", dir.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(st.success());
    assert_eq!(std::fs::read(dir.join(MD_FILE)).unwrap(), std::fs::read(fixtures().join("expected.md")).unwrap());
    assert_eq!(std::fs::read(dir.join(JSON_FILE)).unwrap(), std::fs::read(fixtures().join("expected.json")).unwrap());
}

#[test]
fn ac2_claim_with_missing_test_is_unproven() {
    let err = generate_files(&fixtures().join("decl_unproven.toml"), &fixtures().join("tests.txt")).unwrap_err();
    assert_eq!(err, ConfError::ConformanceUnproven { statement: "STMTB".to_string() });
    assert_eq!(err.code(), "conformance_unproven");
    assert_eq!(err.to_string(), "conformance_unproven{statement: STMTB}");

    let dir = tmp("ac2");
    let out = bin()
        .args(["--decl", fixtures().join("decl_unproven.toml").to_str().unwrap()])
        .args(["--tests", fixtures().join("tests.txt").to_str().unwrap()])
        .args(["--out-dir", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("conformance_unproven{statement: STMTB}"));
    assert!(!dir.join(JSON_FILE).exists() && !dir.join(MD_FILE).exists());
}

#[test]
fn ac2_claim_without_any_test_is_unproven() {
    let decl = "schema = \"fid.dmis-support\"\nversion = 1\n\
                [[statement]]\nkeyword = \"K\"\nimport = \"none\"\nexport = \"partial\"\n";
    assert_eq!(generate(decl, "").unwrap_err(), ConfError::ConformanceUnproven { statement: "K".to_string() });
    let none = "schema = \"fid.dmis-support\"\nversion = 1\n\
                [[statement]]\nkeyword = \"K\"\nimport = \"none\"\nexport = \"none\"\n";
    assert!(generate(none, "").is_ok());
}

#[test]
fn ac3_same_inputs_give_identical_bytes() {
    assert_eq!(small(), small());
    let (a, b) = (tmp("ac3a"), tmp("ac3b"));
    for d in [&a, &b] {
        let st = bin()
            .args(["--decl", fixtures().join("decl_small.toml").to_str().unwrap()])
            .args(["--tests", fixtures().join("tests.txt").to_str().unwrap()])
            .args(["--out-dir", d.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(st.success());
    }
    for f in [JSON_FILE, MD_FILE] {
        assert_eq!(std::fs::read(a.join(f)).unwrap(), std::fs::read(b.join(f)).unwrap());
    }
}

#[test]
fn ac4_check_fails_when_committed_table_differs() {
    let dir = tmp("ac4");
    let out = small();
    write_outputs(&out, &dir).unwrap();
    assert!(stale_files(&out, &dir).is_empty());
    let check = |d: &Path| {
        bin()
            .args(["--decl", fixtures().join("decl_small.toml").to_str().unwrap()])
            .args(["--tests", fixtures().join("tests.txt").to_str().unwrap()])
            .args(["--check", d.to_str().unwrap()])
            .output()
            .unwrap()
    };
    assert_eq!(check(&dir).status.code(), Some(0));

    // A hand edit of the Markdown table makes the check fail.
    let edited = out.markdown.replace("| B | 1 | partial |", "| B | 1 | full |");
    assert_ne!(edited, out.markdown);
    std::fs::write(dir.join(MD_FILE), edited).unwrap();
    assert_eq!(stale_files(&out, &dir), vec![MD_FILE]);
    let res = check(&dir);
    assert_eq!(res.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&res.stderr).contains("stale"));

    // A missing JSON file is stale too.
    std::fs::remove_file(dir.join(JSON_FILE)).unwrap();
    assert_eq!(stale_files(&out, &dir), vec![JSON_FILE, MD_FILE]);
}

#[test]
fn ac4_committed_table_matches_declarations() {
    // Every published declaration is `none` today, so the empty inventory proves nothing extra;
    // CI also runs the check with the real `cargo test -- --list` inventory.
    let out = generate_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance/declarations.toml"),
        &fixtures().join("tests.txt"),
    )
    .unwrap();
    assert!(stale_files(&out, &repo().join("docs/dmis")).is_empty(), "docs/dmis is stale; regenerate it");
}

#[test]
fn module_level_is_minimum_over_statements() {
    let decl = "schema = \"fid.dmis-support\"\nversion = 1\n\
        [[statement]]\nkeyword = \"P\"\nimport = \"full\"\nexport = \"partial\"\nmodules = [{ module = \"M\", level = 2 }]\ntests = [\"p::t::t\"]\n\
        [[statement]]\nkeyword = \"Q\"\nimport = \"none\"\nexport = \"full\"\nmodules = [{ module = \"M\", level = 2 }]\ntests = [\"p::t::t\"]\n";
    let decls = parse_declarations(decl).unwrap();
    let inv = parse_inventory("p::t::t: test\n");
    let table = fid_io_dmis::conf::build(decls, &inv).unwrap();
    let l = table.module_levels.values().next().unwrap();
    assert_eq!((l.import, l.export), (Support::None, Support::Partial));
    assert_eq!(l.statements, vec!["P".to_string(), "Q".to_string()]);
}

#[test]
fn inventory_reads_cargo_list_output() {
    let inv = parse_inventory("conf::a: test\r\nb: benchmark\n\n3 tests, 1 benchmarks\nplain_id\n: test\n");
    let got: Vec<&str> = inv.iter().map(String::as_str).collect();
    assert_eq!(got, vec!["conf::a", "plain_id"]);
}

#[test]
fn malformed_declarations_are_rejected() {
    let head = "schema = \"fid.dmis-support\"\nversion = 1\n";
    let stmt = |body: &str| format!("{head}[[statement]]\n{body}\n");
    let cases = [
        "not toml = = =".to_string(),
        "schema = \"other\"\nversion = 1\n".to_string(),
        "schema = \"fid.dmis-support\"\nversion = 2\n".to_string(),
        format!("{head}extra = 1\n"),
        stmt("keyword = \"K\"\nimport = \"most\"\nexport = \"none\""),
        stmt("keyword = \"k\"\nimport = \"none\"\nexport = \"none\""),
        stmt("keyword = \"K\"\nimport = \"none\""),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\nnote = \"x\""),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\nmodules = [{ module = \"M\", level = 256 }]"),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\nmodules = [{ module = \"a|b\", level = 1 }]"),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\nmodules = [{ module = \"M\", level = 1 }, { module = \"M\", level = 1 }]"),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\ntests = [\"a b\"]"),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\ntests = [\"p::t::t\", \"p::t::t\"]"),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\ntests = [\"t_a\"]"),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\ntests = [\"acceptance::t_a\"]"),
        stmt("keyword = \"K\"\nimport = \"none\"\nexport = \"none\"\ntests = [\"Pkg::acceptance::t_a\"]"),
        format!("{head}[[statement]]\nkeyword = \"K\"\nimport = \"none\"\nexport = \"none\"\n[[statement]]\nkeyword = \"K\"\nimport = \"none\"\nexport = \"none\"\n"),
    ];
    for c in &cases {
        let err = generate(c, "").unwrap_err();
        assert_eq!(err.code(), "malformed", "{c}: {err}");
    }
}

#[test]
fn bad_usage_exits_2() {
    let out = bin().args(["--decl", "x.toml"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

const ONE_CLAIM: &str = "schema = \"fid.dmis-support\"\nversion = 1\n\
    [[statement]]\nkeyword = \"K\"\nimport = \"full\"\nexport = \"none\"\ntests = [\"fid-demo::acceptance::slow\"]\n";

#[test]
fn ignored_tests_cannot_back_a_claim() {
    let list = "fast: test\nslow: test\n\n2 tests, 0 benchmarks\n";
    let ignored = "slow: test\n\n1 tests, 0 benchmarks\n";
    let ids = qualified_ids("fid-demo", "acceptance", list, ignored);
    let got: Vec<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(got, vec!["fid-demo::acceptance::fast"]);
    assert_eq!(
        generate(ONE_CLAIM, &inventory_text(&ids)).unwrap_err(),
        ConfError::ConformanceUnproven { statement: "K".to_string() }
    );
    // The same test, not ignored, proves the claim.
    let ids = qualified_ids("fid-demo", "acceptance", list, "");
    assert!(generate(ONE_CLAIM, &inventory_text(&ids)).is_ok());
}

#[test]
fn same_named_test_in_another_package_does_not_match() {
    let ids = qualified_ids("fid-other", "acceptance", "slow: test\n", "");
    assert_eq!(inventory_text(&ids), "fid-other::acceptance::slow: test\n");
    assert_eq!(
        generate(ONE_CLAIM, &inventory_text(&ids)).unwrap_err(),
        ConfError::ConformanceUnproven { statement: "K".to_string() }
    );
    // An unqualified short name is rejected outright.
    let short = ONE_CLAIM.replace("fid-demo::acceptance::slow", "slow");
    assert_eq!(generate(&short, "slow: test\n").unwrap_err().code(), "malformed");
}

#[test]
fn test_binaries_come_from_cargo_json() {
    let text = std::fs::read_to_string(fixtures().join("cargo-no-run.json")).unwrap();
    assert_eq!(
        test_binaries(&text).unwrap(),
        vec![TestBinary {
            manifest_path: PathBuf::from("/w/crates/fid-demo/Cargo.toml"),
            target: "acceptance".to_string(),
            executable: PathBuf::from("/w/target/debug/deps/acceptance-3c4d"),
        }]
    );
    assert_eq!(test_binaries("not json").unwrap_err().code(), "malformed");
}

/// Never runs by default. CI checks that the real inventory leaves this test out
/// (ignored tests cannot back a claim) while it lists the other tests of this binary.
#[test]
#[ignore = "probe for the CI inventory check"]
fn ignored_inventory_probe() {}
