//! Acceptance criteria of spec node KRN.NUM.LINT.DATA (AC-1 .. AC-5) and its fail-closed
//! paths. The denylists, the canary crates and the literal-forms file come from the independent
//! Python reference `fixtures/knp1/data/make_fixtures.py`; the expected findings are its
//! hand-labelled `//~ RULE=token` markers. None is produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_knp1_lint::data::{
    load_denylist, load_denylist_file, normalise_decimal, run, scan_closure, scan_file, DataErrorCode, FileRef, Matcher,
};
use fid_knp1_lint::find::{Finding, Rule};
use fid_knp1_lint::graph::{enumerate_files, Closure, CrateEntry, SourceKind};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/knp1/data").join(rel)
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn entry(name: &str, dir: &str) -> CrateEntry {
    let root = fix(dir);
    let files = enumerate_files(&root, &[], &BTreeSet::new()).unwrap();
    CrateEntry {
        name: name.to_string(),
        version: "0.0.1".to_string(),
        source_kind: SourceKind::Vendored,
        root,
        root_label: dir.to_string(),
        files,
        licence: None,
        path_from_root: Vec::new(),
    }
}

fn denylist() -> fid_knp1_lint::data::Denylist {
    load_denylist_file(&fix("denylist.json")).unwrap()
}

type Pos = BTreeSet<(u32, u32, String)>;

fn markers(src: &str) -> Pos {
    let mut out = BTreeSet::new();
    for (n, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let Some(at) = line.find("//~") else { continue };
        let code = &line[..at];
        for m in line[at + 3..].split_whitespace() {
            let (rule, tok) = m.split_once('=').unwrap();
            let byte = code.find(tok).unwrap_or_else(|| panic!("token {tok:?} not on line {}", n + 1));
            let col = u32::try_from(code[..byte].chars().count() + 1).unwrap();
            out.insert((u32::try_from(n + 1).unwrap(), col, rule.to_string()));
        }
    }
    out
}

fn keyed(fs: &[Finding]) -> Pos {
    fs.iter().map(|f| (f.line, f.column, f.rule.id())).collect()
}

#[test]
fn ac1_canary_positive_and_negative() {
    let r = run(&fix("ws/knp1-lint.toml")).unwrap();
    assert_eq!(r.findings.len(), 1, "{:#?}", r.findings);
    let f = &r.findings[0];
    assert_eq!(f.rule, Rule::Data("VALUE".into()));
    assert_eq!((f.crate_name.as_str(), f.file.as_str(), f.line, f.column), ("knp1-data-canary", "src/lib.rs", 3, 20));
    assert_eq!(r.denylist_digest, denylist().digest);
    f.validate().unwrap();

    let clean = scan_closure(&Closure { crates: vec![entry("knp1-data-canary", "clean")] }, &denylist()).unwrap();
    assert!(clean.is_empty(), "{clean:#?}");
}

#[test]
fn ac2_to_ac4_literal_forms_match_the_hand_labels() {
    let e = entry("knp1-data-forms", "forms");
    let src = String::from_utf8(read(&e.root.join("src/lib.rs"))).unwrap();
    let out = scan_closure(&Closure { crates: vec![e] }, &denylist()).unwrap();
    for f in &out {
        f.validate().unwrap();
    }
    let want = markers(&src);
    assert!(want.len() >= 15, "{want:?}");
    assert_eq!(keyed(&out), want);
}

#[test]
fn ac5_bad_digest_and_bad_schema_fail_closed() {
    for name in ["bad-digest.json", "schema-no-ids.json", "missing.json"] {
        let e = load_denylist_file(&fix(name)).unwrap_err();
        assert_eq!(e.code, DataErrorCode::Denylist, "{name}: {e}");
        assert_eq!(e.code.as_str(), "KNP1-DATA-DENYLIST");
    }
    let good = String::from_utf8(read(&fix("denylist.json"))).unwrap();
    let variants = [
        good.replacen("\"version\": 1", "\"version\": 2", 1),
        good.replacen("{", "{\"ids\": [], ", 1),
        good.replacen("\"hex\": \"0x", "\"hex\": \"0X", 1),
        good.replacen("\"1e9\"", "\"nine\"", 1),
        good.replacen("\"tracim.lsq.circle.007\"", "\"\"", 1),
        "[]".to_string(),
        "{".to_string(),
    ];
    for v in variants {
        let e = load_denylist(v.as_bytes()).unwrap_err();
        assert_eq!(e.code, DataErrorCode::Denylist, "{v:.80}");
    }
}

#[test]
fn decimals_normalise_exactly() {
    let n = |s: &str| normalise_decimal(s).unwrap();
    assert_eq!(n("0.123_456_789_012"), (("123456789012".into(), -12), 12));
    assert_eq!(n("1.23456789012e-1"), (("123456789012".into(), -12), 12));
    assert_eq!(n("-0.1234567890120"), (("123456789012".into(), -12), 13));
    assert_eq!(n("1200"), (("12".into(), 2), 4));
    assert_eq!(n("0.000"), ((String::new(), 0), 0));
    assert_eq!(n("1."), (("1".into(), 0), 1));
    for bad in ["", ".", "e5", "1e", "1.2.3", "0x10", "1e9999999999", "abc"] {
        assert!(normalise_decimal(bad).is_none(), "{bad}");
    }
}

#[test]
fn unlexable_file_is_a_parse_finding_and_never_panics() {
    let m = Matcher::new(&denylist());
    let files = vec!["src/lib.rs".to_string()];
    let f = FileRef { crate_name: "k", crate_version: "1.0.0", file: "src/lib.rs", crate_files: &files };
    let out = scan_file(f, b"const S: &str = \"open;\n", &m);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].rule, Rule::Parse);
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..2000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let n = (state % 64) as usize;
        let bytes: Vec<u8> =
            (0..n).map(|k| b"0123456789._efx\"\\'r#b!(),;u"[((state >> (k % 50)) as usize + k) % 27]).collect();
        for x in scan_file(f, &bytes, &m) {
            x.validate().unwrap();
        }
    }
}
