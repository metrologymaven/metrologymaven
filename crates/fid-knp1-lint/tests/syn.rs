//! Acceptance criteria of spec node KRN.NUM.LINT.SYN (AC-1 .. AC-5), the hand-labelled
//! bypass cases, the fail-closed paths and the pinned rule table. Expected results come from
//! hand labels: `fixtures/knp1/seeded/expected-findings.txt` (written by the independent
//! Python reference `make_expected.py` from hand-labelled positions), the `//~` markers in
//! `fixtures/knp1/syn/cases/src/`, and the hand-written `expected-*.txt` files. None of them is
//! produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_knp1_lint::find::{
    apply_waivers, lines_digest, Applied, ErrorCode, Finding, InputDigests, Report, Rule, Waiver,
};
use fid_knp1_lint::graph::{enumerate_files, Closure, CrateEntry, SourceKind};
use fid_knp1_lint::syn_scan::{
    lex, parse_config, run, scan_closure, scan_file, Config, FileRef, Outcome, PatternKind, SynErrorCode,
    MAX_FILE_BYTES, RULE_TABLE, RULE_TABLE_LEN,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/knp1").join(rel)
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn entry(name: &str, version: &str, dir: &str) -> CrateEntry {
    let root = fix(dir);
    let files = enumerate_files(&root, &[], &BTreeSet::new()).unwrap();
    CrateEntry {
        name: name.to_string(),
        version: version.to_string(),
        source_kind: SourceKind::Vendored,
        root,
        root_label: dir.to_string(),
        files,
        licence: None,
        path_from_root: Vec::new(),
    }
}

fn scan(entries: Vec<CrateEntry>, cfg: &Config) -> Outcome {
    let mut crates = entries;
    crates.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    let out = scan_closure(&Closure { crates }, cfg).unwrap();
    for f in out.findings.iter().chain(&out.notes) {
        f.validate().unwrap();
    }
    out
}

/// `RULE crate version file:line:column`.
fn lines(fs: &[Finding]) -> Vec<String> {
    fs.iter()
        .map(|f| format!("{} {} {} {}:{}:{}", f.rule, f.crate_name, f.crate_version, f.file, f.line, f.column))
        .collect()
}

fn text_lines(p: &Path) -> Vec<String> {
    String::from_utf8(read(p)).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn cfg_file(rel: &str) -> Config {
    parse_config(std::str::from_utf8(&read(&fix(rel))).unwrap()).unwrap()
}

fn one(src: &str) -> Outcome {
    one_cfg(src, &Config::default())
}

fn one_cfg(src: &str, cfg: &Config) -> Outcome {
    let files = vec!["src/lib.rs".to_string()];
    let f = FileRef { crate_name: "k", crate_version: "1.0.0", file: "src/lib.rs", crate_files: &files };
    let out = scan_file(f, src.as_bytes(), cfg);
    for x in out.findings.iter().chain(&out.notes) {
        x.validate().unwrap();
    }
    out
}

/// `RULE@line:col` of each finding.
fn short(fs: &[Finding]) -> Vec<String> {
    fs.iter().map(|f| format!("{}@{}:{}", f.rule, f.line, f.column)).collect()
}

// ------------------------------------------------------------------ AC-1

fn seeded_dirs() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(fix("seeded"))
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn ac1_each_seeded_crate_yields_exactly_its_planted_finding() {
    let mut all = Vec::new();
    for rule in seeded_dirs() {
        let name = format!("seeded-{}", rule.to_lowercase());
        let out = scan(vec![entry(&name, "0.0.1", &format!("seeded/{rule}"))], &Config::default());
        assert_eq!(out.findings.len(), 1, "{rule}: {:#?}", out.findings);
        assert_eq!(out.findings[0].rule.id(), rule);
        assert!(out.notes.is_empty(), "{rule}: {:?}", out.notes);
        all.extend(out.findings);
    }
    let got: Vec<String> =
        all.iter().map(|f| format!("{} {}", lines(std::slice::from_ref(f))[0], f.excerpt_digest)).collect();
    assert_eq!(got, text_lines(&fix("seeded/expected-findings.txt")));
}

#[test]
fn ac1_every_rule_of_the_table_has_a_seeded_crate() {
    let rules: BTreeSet<String> = RULE_TABLE.iter().map(|(r, _, _)| r.id()).collect();
    let dirs: BTreeSet<String> = seeded_dirs().into_iter().collect();
    assert_eq!(rules, dirs);
}

// ------------------------------------------------------------------ AC-2 .. AC-5

#[test]
fn ac2_macro_body_in_a_dependency_is_reported_at_the_body_line() {
    let out = scan(
        vec![entry("knp1-macro-dep", "0.0.1", "syn/macro/dep"), entry("knp1-macro-user", "0.0.1", "syn/macro/user")],
        &Config::default(),
    );
    assert_eq!(lines(&out.findings), ["KNP1-FMA knp1-macro-dep 0.0.1 src/lib.rs:6:12"]);
}

#[test]
fn ac3_use_rename_is_reported_at_the_use_line_and_at_the_alias() {
    let out = scan(vec![entry("knp1-syn-alias", "0.0.1", "syn/alias")], &Config::default());
    assert_eq!(
        lines(&out.findings),
        ["KNP1-HASH knp1-syn-alias 0.0.1 src/lib.rs:1:23", "KNP1-HASH knp1-syn-alias 0.0.1 src/lib.rs:4:5"]
    );
    assert!(out.findings[1].message.contains("std::collections::HashMap"), "{}", out.findings[1].message);
}

#[test]
fn ac4_syntax_error_is_an_unwaivable_parse_finding() {
    let e = entry("knp1-syn-parse-error", "0.0.1", "syn/parse-error");
    let src = read(&e.root.join("src/lib.rs"));
    let out = scan(vec![e], &Config::default());
    assert_eq!(lines(&out.findings), ["KNP1-PARSE knp1-syn-parse-error 0.0.1 src/lib.rs:3:14"]);
    assert_eq!(out.findings[0].rule, Rule::Parse);
    assert!(!Rule::Parse.is_waivable());

    // Counted as unwaived in the report.
    let inputs = InputDigests::of(b"c", b"l", b"", b"d");
    let r = Report::new("0.0.1", inputs, Applied::without_waivers(out.findings.clone())).unwrap();
    assert_eq!(r.unwaived_count(), 1);
    assert!(!r.passed());

    // A waiver cannot cover it.
    let w = Waiver {
        crate_name: "knp1-syn-parse-error".into(),
        file: "src/lib.rs".into(),
        lines: (3, 3),
        rule: Rule::Parse,
        justification: "try".into(),
        reviewer: "someone".into(),
        date: "2026-11-19".into(),
        lines_sha256: lines_digest(&src, 3, 3).unwrap(),
    };
    let mut tree: BTreeMap<(String, String, String), Vec<u8>> = BTreeMap::new();
    tree.insert(("knp1-syn-parse-error".into(), "0.0.1".into(), "src/lib.rs".into()), src);
    let errs = apply_waivers(out.findings, &[w], &tree).unwrap_err();
    assert_eq!(errs[0].code, ErrorCode::WaiverSchema);
}

#[test]
fn ac5_clean_code_has_no_findings_and_reports_its_facade_uses() {
    let cfg = cfg_file("syn/clean/knp1-lint.toml");
    let out = scan(vec![entry("knp1-syn-clean", "0.0.1", "syn/clean"), facade_stub()], &cfg);
    assert!(out.findings.is_empty(), "{:#?}", out.findings);
    assert_eq!(short(&out.notes), ["KNP1-LIBM@3:17", "KNP1-LIBM@9:29", "KNP1-LIBM@9:54"]);
    for n in &out.notes {
        assert!(n.message.contains("facade path `fid_num::f`"), "{}", n.message);
        assert!(n.message.contains("KRN.NUM.FN"), "{}", n.message);
    }
}

// ------------------------------------------------------------------ hand-labelled cases

/// Parses the `//~` markers of one file: (findings, notes) as (line, column, rule).
type Pos = BTreeSet<(u32, u32, String)>;

fn markers(src: &str) -> (Pos, Pos) {
    let mut found = BTreeSet::new();
    let mut notes = BTreeSet::new();
    for (n, line) in src.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue; // a comment line describing the markers
        }
        let Some(at) = line.find("//~") else { continue };
        let code = &line[..at];
        for m in line[at + 3..].split_whitespace() {
            let (is_note, m) = match m.strip_prefix("note:") {
                Some(rest) => (true, rest),
                None => (false, m),
            };
            let (rule, tok) = m.split_once('=').unwrap_or_else(|| panic!("bad marker {m:?} on line {}", n + 1));
            let (tok, nth) = match tok.split_once('#') {
                Some((t, k)) => (t, k.parse::<usize>().unwrap()),
                None => (tok, 1),
            };
            let byte = code
                .match_indices(tok)
                .nth(nth - 1)
                .unwrap_or_else(|| panic!("token {tok:?}#{nth} not on line {}", n + 1))
                .0;
            let col = u32::try_from(code[..byte].chars().count() + 1).unwrap();
            let key = (u32::try_from(n + 1).unwrap(), col, rule.to_string());
            if is_note {
                notes.insert(key);
            } else {
                found.insert(key);
            }
        }
    }
    (found, notes)
}

fn keyed(fs: &[Finding], file: &str) -> Pos {
    fs.iter()
        .filter(|f| f.crate_name == "knp1-syn-cases" && f.file == file)
        .map(|f| (f.line, f.column, f.rule.id()))
        .collect()
}

/// The facade crate the `[syn.facade_paths]` entries of the fixtures name.
fn facade_stub() -> CrateEntry {
    entry("fid-num", "0.1.0", "syn/fid-num-stub")
}

#[test]
fn hand_labelled_cases_match_exactly() {
    let cfg = cfg_file("syn/cases/knp1-lint.toml");
    let e = entry("knp1-syn-cases", "0.0.1", "syn/cases");
    let files = e.files.clone();
    let root = e.root.clone();
    assert!(files.len() >= 10, "{files:?}");
    let out = scan(vec![e, facade_stub()], &cfg);
    let mut total = 0;
    for file in &files {
        let src = String::from_utf8(read(&root.join(file))).unwrap();
        let (want, want_notes) = markers(&src);
        total += want.len();
        assert_eq!(keyed(&out.findings, file), want, "findings of {file}");
        assert_eq!(keyed(&out.notes, file), want_notes, "notes of {file}");
    }
    assert_eq!(out.findings.len(), total);
    assert!(total >= 90, "{total}");
}

// ------------------------------------------------------------------ end to end

#[test]
fn run_scans_the_closure_from_the_config() {
    let out = run(&fix("syn/run/knp1-lint.toml")).unwrap();
    assert_eq!(lines(&out.findings), text_lines(&fix("syn/run/expected-findings.txt")));
    assert_eq!(lines(&out.notes), text_lines(&fix("syn/run/expected-notes.txt")));
    // Deterministic: a second run gives the same output.
    assert_eq!(run(&fix("syn/run/knp1-lint.toml")).unwrap(), out);
}

#[test]
fn syn_findings_go_through_the_waiver_flow() {
    let e = entry("seeded-knp1-fma", "0.0.1", "seeded/KNP1-FMA");
    let src = read(&e.root.join("src/lib.rs"));
    let out = scan(vec![e], &Config::default());
    let w = Waiver {
        crate_name: "seeded-knp1-fma".into(),
        file: "src/lib.rs".into(),
        lines: (4, 4),
        rule: Rule::Fma,
        justification: "vendored solver, reviewed".into(),
        reviewer: "reviewer".into(),
        date: "2026-11-19".into(),
        lines_sha256: lines_digest(&src, 4, 4).unwrap(),
    };
    let mut tree: BTreeMap<(String, String, String), Vec<u8>> = BTreeMap::new();
    tree.insert(("seeded-knp1-fma".into(), "0.0.1".into(), "src/lib.rs".into()), src);
    let applied = apply_waivers(out.findings, &[w], &tree).unwrap();
    assert!(applied.findings.is_empty());
    assert_eq!(applied.waived.len(), 1);
}

// ------------------------------------------------------------------ fatal errors and config

#[test]
fn unreadable_listed_file_is_fatal() {
    let mut e = entry("seeded-knp1-fma", "0.0.1", "seeded/KNP1-FMA");
    e.files.push("src/missing.rs".into());
    let err = scan_closure(&Closure { crates: vec![e] }, &Config::default()).unwrap_err();
    assert_eq!(err.code, SynErrorCode::Read);
    assert_eq!(err.code.as_str(), "KNP1-READ");
    assert!(err.message.contains("src/missing.rs"), "{err}");
}

#[test]
fn facade_file_outside_the_closure_is_a_config_error() {
    for text in [
        "[[syn.facade_files]]\ncrate = \"x\"\nversion = \"0.0.1\"\nfile = \"src/a.rs\"\nreason = \"r\"\n",
        // Right crate and file, wrong version.
        "[[syn.facade_files]]\ncrate = \"seeded-knp1-fma\"\nversion = \"9.9.9\"\nfile = \"src/lib.rs\"\nreason = \"r\"\n",
        // A facade path whose crate is not in the closure, or not at that version.
        "[syn.facade_paths]\n\"fid_num::f\" = { crate = \"fid-num\", version = \"0.1.0\", reason = \"r\" }\n",
        "[syn.facade_paths]\n\"seeded_knp1_fma::f\" = { crate = \"seeded-knp1-fma\", version = \"0.0.2\", reason = \"r\" }\n",
    ] {
        let cfg = parse_config(text).unwrap();
        let e = entry("seeded-knp1-fma", "0.0.1", "seeded/KNP1-FMA");
        let err = scan_closure(&Closure { crates: vec![e] }, &cfg).unwrap_err();
        assert_eq!(err.code, SynErrorCode::ConfigInvalid, "{text}");
    }
    // The same facade file at the right version is accepted.
    let ok = "[[syn.facade_files]]\ncrate = \"seeded-knp1-fma\"\nversion = \"0.0.1\"\nfile = \"src/lib.rs\"\nreason = \"r\"\n";
    let e = entry("seeded-knp1-fma", "0.0.1", "seeded/KNP1-FMA");
    assert!(scan_closure(&Closure { crates: vec![e] }, &parse_config(ok).unwrap()).is_ok());
}

#[test]
fn config_is_validated() {
    assert_eq!(parse_config("[graph]\nroots = [\"a\"]\n").unwrap(), Config::default());
    let ok = parse_config(
        "[syn.facade_paths]\n\"fid_num::f\" = { crate = \"fid-num\", version = \"0.1.0\", reason = \"facade\" }\n[[syn.facade_files]]\ncrate = \"fid-num\"\nversion = \"0.1.0\"\nfile = \"src/f/mod.rs\"\nreason = \"impl\"\n",
    )
    .unwrap();
    assert_eq!(ok.facade_paths.len(), 1);
    assert_eq!(ok.facade_files.len(), 1);
    for bad in [
        "syn = 1\n",
        "[syn]\nother = 1\n",
        "[syn.facade_paths]\n\"std::f64\" = \"no\"\n",
        "[syn.facade_paths]\n\"core\" = \"no\"\n",
        "[syn.facade_paths]\n\"crate::f\" = \"no\"\n",
        "[syn.facade_paths]\n\"fid_num::f\" = \"  \"\n",
        "[syn.facade_paths]\n\"fid num\" = \"r\"\n",
        "[syn.facade_paths]\n\"fid_num::\" = \"r\"\n",
        "[syn.facade_paths]\n\"fid_num::f\" = 3\n",
        "[[syn.facade_files]]\ncrate = \"a\"\nfile = \"src/a.rs\"\n",
        "[[syn.facade_files]]\ncrate = \"a\"\nfile = \"../a.rs\"\nreason = \"r\"\n",
        "[[syn.facade_files]]\ncrate = \"a\"\nfile = \"C:/a.rs\"\nreason = \"r\"\n",
        "[[syn.facade_files]]\ncrate = \"a\"\nfile = \"src\\\\a.rs\"\nreason = \"r\"\n",
        "[[syn.facade_files]]\ncrate = \"a\"\nfile = \"src/a.rs\"\nreason = \"r\"\nextra = 1\n",
        "[[syn.facade_files]]\ncrate = \"a\"\nfile = \"src/a.rs\"\nreason = \"r\"\n[[syn.facade_files]]\ncrate = \"a\"\nfile = \"src/a.rs\"\nreason = \"s\"\n",
        "[syn.facade_paths]\n\"a\" = \"r\"\n\"a\" = \"s\"\n",
        "[syn.facade_paths]\n\"fid_num::f\" = { crate = \"other\", version = \"1\", reason = \"r\" }\n",
        "[syn.facade_paths]\n\"fid_num::f\" = { crate = \"fid-num\", reason = \"r\" }\n",
        "[syn.facade_paths]\n\"fid_num::f\" = { crate = \"fid-num\", version = \"1\", reason = \"\" }\n",
        "[syn.facade_paths]\n\"fid_num::f\" = { crate = \"fid-num\", version = \"1\", reason = \"r\", x = 1 }\n",
        "[[syn.facade_files]]\ncrate = \"a\"\nfile = \"src/a.rs\"\nreason = \"r\"\n",
    ] {
        let e = parse_config(bad).unwrap_err();
        assert_eq!(e.code, SynErrorCode::ConfigInvalid, "{bad}");
        assert_eq!(e.code.as_str(), "KNP1-CONFIG-INVALID");
    }
}

// ------------------------------------------------------------------ lexer and fail-closed paths

#[test]
fn unlexable_inputs_are_parse_findings() {
    for (src, pos) in [
        ("fn f() { \"open }\n", "KNP1-PARSE@1:10"),
        ("fn f() {}\n/* never /* closed */\n", "KNP1-PARSE@2:1"),
        ("fn f() { }}\n", "KNP1-PARSE@1:11"),
        ("fn f(] {}\n", "KNP1-PARSE@1:6"),
        ("fn f() {\n", "KNP1-PARSE@1:8"),
        ("fn caf\u{e9}() {}\n", "KNP1-PARSE@1:4"),
        ("fn \u{e9}t\u{e9}() {}\n", "KNP1-PARSE@1:4"),
        ("let x = `y`;\n", "KNP1-PARSE@1:9"),
        ("let x = a \\ b;\n", "KNP1-PARSE@1:11"),
        ("let c = 'ab';\n", "KNP1-PARSE@1:12"),
        ("let s = r#\"open\";\n", "KNP1-PARSE@1:9"),
        ("let x = \u{2603};\n", "KNP1-PARSE@1:9"),
        ("let x = '\n", "KNP1-PARSE@1:9"),
    ] {
        let out = one(src);
        assert_eq!(short(&out.findings), [pos], "{src:?}: {:?}", out.findings);
        assert!(out.findings[0].message.starts_with("cannot lex file"), "{}", out.findings[0].message);
    }
}

#[test]
fn bad_utf8_and_oversized_files_are_parse_findings() {
    let files = vec!["src/lib.rs".to_string()];
    let f = FileRef { crate_name: "k", crate_version: "1.0.0", file: "src/lib.rs", crate_files: &files };
    let out = scan_file(f, b"fn f() {}\nab\xff\n", &Config::default());
    assert_eq!(short(&out.findings), ["KNP1-PARSE@2:3"]);
    let big = vec![b' '; usize::try_from(MAX_FILE_BYTES).unwrap() + 1];
    let out = scan_file(f, &big, &Config::default());
    assert_eq!(short(&out.findings), ["KNP1-PARSE@1:1"]);
    assert!(out.findings[0].message.contains("larger than"));
}

#[test]
fn deep_nesting_and_long_inputs_do_not_crash() {
    let deep = "(".repeat(600) + &")".repeat(600);
    let out = one(&format!("fn f() {{ let x = {deep}; }}"));
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].rule, Rule::Parse);
    assert!(out.findings[0].message.contains("nested deeper"), "{}", out.findings[0].message);

    let ok_deep = "(".repeat(500) + "1.0" + &")".repeat(500);
    assert!(one(&format!("fn f() -> f64 {{ {ok_deep} }}")).findings.is_empty());

    let bangs = "!".repeat(200_000);
    assert!(one(&format!("fn f() -> bool {{ {bangs}true }}")).findings.is_empty());

    let use_deep = "{".repeat(40) + "a" + &"}".repeat(40);
    let out = one(&format!("use x::{use_deep};"));
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].rule, Rule::ParseResolve);
}

#[test]
fn crlf_bom_and_shebang_keep_positions() {
    let lf = one("fn f(x: f64) -> f64 {\n    x.sqrt()\n}\n");
    let crlf = one("fn f(x: f64) -> f64 {\r\n    x.sqrt()\r\n}\r\n");
    assert_eq!(short(&lf.findings), ["KNP1-LIBM@2:7"]);
    assert_eq!(lf, crlf);
    let bom = one("\u{feff}fn f(x: f64) -> f64 {\n    x.sqrt()\n}\n");
    assert_eq!(bom, lf);
    let shebang = one("#!/usr/bin/env run-cargo-script\nfn f(x: f64) -> f64 { x.sqrt() }\n");
    assert_eq!(short(&shebang.findings), ["KNP1-LIBM@2:25"]);
    let inner_attr = one("#![forbid(unsafe_code)]\nfn f() {}\n");
    assert!(inner_attr.findings.is_empty(), "{:?}", inner_attr.findings);
    // rustc's rule: whitespace and non-doc comments between `#!` and `[` keep it an attribute.
    for src in [
        "#!/**/[allow(unused)] fn f(x: f64) -> f64 { x.mul_add(1.0, 2.0) }\n",
        "#! // c\n /* d /* e */ */ [allow(unused)] fn f(x: f64) -> f64 { x.mul_add(1.0, 2.0) }\n",
        "#!\n\n[allow(unused)] fn f(x: f64) -> f64 { x.mul_add(1.0, 2.0) }\n",
    ] {
        let out = one(src);
        assert!(out.findings.iter().any(|f| f.rule == Rule::Fma), "{src:?}: {:?}", out.findings);
    }
    // A doc comment after `#!` makes it a shebang line, as in rustc: only that line is dropped.
    let doc = one("#!/// doc [x.mul_add(1.0, 2.0)]\nfn f(x: f64) -> f64 { x.sqrt() }\n");
    assert_eq!(short(&doc.findings), ["KNP1-LIBM@2:25"]);
}

#[test]
fn alias_cycles_and_unreadable_use_trees_fail_closed() {
    let out = one("use a as b;\nuse b as a;\nfn f() { a::x(); }\n");
    assert!(out.findings.iter().any(|f| f.rule == Rule::ParseResolve && f.line == 3), "{:?}", out.findings);
    assert!(Rule::ParseResolve.is_waivable());
    let out = one("macro_rules! m { ($($x:ident),*) => { use a::{$($x),*}; } }\n");
    assert_eq!(out.findings.iter().filter(|f| f.rule == Rule::ParseResolve).count(), 1, "{:?}", out.findings);
    // `use a::a;` refers to the module `a`, not to itself: no cycle.
    assert!(one("use a::a;\nfn f() { a::x(); }\n").findings.is_empty());
}

#[test]
fn metavariables_are_wildcards() {
    let out = one("macro_rules! m { ($a:ident, $b:ident) => { $a::$b::read() } }\n");
    let rules: BTreeSet<String> = out.findings.iter().map(|f| f.rule.id()).collect();
    for r in ["KNP1-IO-FS", "KNP1-IO-NET", "KNP1-CLOCK", "KNP1-ENV", "KNP1-THREAD", "KNP1-SIMD"] {
        assert!(rules.contains(r), "{r}: {rules:?}");
    }
    let out = one("macro_rules! m { ($x:expr, $y:expr) => { $x.max($y) } }\n");
    assert_eq!(short(&out.findings), ["KNP1-MINMAX@1:45"]);
}

#[test]
fn include_and_path_targets_must_be_listed_source() {
    let files = vec!["src/lib.rs".to_string(), "src/gen.rs".to_string()];
    let f = FileRef { crate_name: "k", crate_version: "1.0.0", file: "src/lib.rs", crate_files: &files };
    let cfg = Config::default();
    let s = |src: &str| short(&scan_file(f, src.as_bytes(), &cfg).findings);
    assert!(s("include!(\"gen.rs\");").is_empty());
    assert!(s("#[path = \"gen.rs\"] mod g;").is_empty());
    assert_eq!(s("include!(\"other.rs\");"), ["KNP1-IO-FS@1:1"]);
    assert_eq!(s("include!(\"./gen.rs\");"), ["KNP1-IO-FS@1:1"]);
    assert_eq!(s("include!(\"gen.rs\", x);"), ["KNP1-IO-FS@1:1"]);
    assert_eq!(s("include!(concat!(\"gen\", \".rs\"));"), ["KNP1-IO-FS@1:1"]);
    assert_eq!(s("include_str!(\"gen.rs\"suffix);"), ["KNP1-IO-FS@1:1"]);
    assert_eq!(s("#[path = \"/abs/gen.rs\"] mod g;"), ["KNP1-IO-FS@1:3"]);
    assert_eq!(s("#[cfg_attr(unix, path = \"bin/x.rs\")] mod g;"), ["KNP1-IO-FS@1:18"]);
    assert_eq!(s("include!(env!(\"OUT_DIR\"));"), ["KNP1-IO-FS@1:1", "KNP1-ENV@1:10"]);
}

#[test]
fn proc_macro_crates_are_parse_findings_whatever_the_spelling() {
    // `[lib] crate-type = ["proc-macro"]` with no entry-point attribute (review 5).
    let out = scan(vec![entry("knp1-syn-pm-crate-type", "0.0.1", "syn/procmacro-crate-type")], &Config::default());
    assert_eq!(lines(&out.findings), ["KNP1-PARSE knp1-syn-pm-crate-type 0.0.1 Cargo.toml:1:1"]);
    assert!(out.findings[0].message.contains("proc-macro crate"), "{}", out.findings[0].message);
    // An entry-point attribute is reported whatever the manifest says.
    for (src, col) in [
        (
            "#[proc_macro]
pub fn a(i: TokenStream) -> TokenStream { i }
",
            3,
        ),
        (
            "#[proc_macro_derive(X)]
pub fn a(i: TokenStream) -> TokenStream { i }
",
            3,
        ),
        (
            "#[cfg_attr(all(), proc_macro_attribute)]
pub fn a(a: TokenStream, i: TokenStream) -> TokenStream { i }
",
            19,
        ),
    ] {
        assert_eq!(short(&one(src).findings), [format!("KNP1-PARSE@1:{col}")], "{src:?}");
    }
}

#[test]
fn coarse_performance() {
    // 20 000 lines of mixed code; the NODE.md budget (300 000 lines < 10 s, release) is not
    // measured here. This only guards against quadratic blow-ups, with a generous debug bound.
    let mut src = String::new();
    for i in 0..20_000 {
        src.push_str(&format!(
            "    let v{i} = a.max(b) + c::d::e(x, y) + s.iter().map(|z| z + 1.0).sum::<f64>() + w[{i}].len().min(3);\n"
        ));
    }
    let src = format!("fn f() {{\n{src}}}\n");
    let t = std::time::Instant::now();
    let out = one(&src);
    assert!(out.findings.is_empty(), "{:?}", out.findings.first());
    assert!(t.elapsed().as_secs() < 120, "{:?}", t.elapsed());
}

#[test]
fn never_panics_on_random_input() {
    const PIECES: [&str; 40] = [
        "fn", " ", "(", ")", "{", "}", "[", "]", "::", ".", "..", "'", "\"", "r#", "#", "!", "$", "<", ">", "=", "use",
        "as", "*", ",", ";", "std", "fs", "f64", "max", "1.0", "0x", "e", "/*", "*/", "//", "\n", "\\", "b'", "unsafe",
        "\u{e9}",
    ];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..3000 {
        let n = (next() % 60) as usize;
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(PIECES[(next() % PIECES.len() as u64) as usize]);
        }
        let out = one(&s);
        let mut sorted = out.findings.clone();
        fid_knp1_lint::find::sort_findings(&mut sorted);
        assert_eq!(sorted, out.findings);
        // Raw bytes too.
        let bytes: Vec<u8> = (0..n).map(|_| (next() & 0xFF) as u8).collect();
        let files = vec!["src/lib.rs".to_string()];
        let f = FileRef { crate_name: "k", crate_version: "1.0.0", file: "src/lib.rs", crate_files: &files };
        for x in scan_file(f, &bytes, &Config::default()).findings {
            x.validate().unwrap();
        }
    }
}

#[test]
fn lexer_tokens_are_classified() {
    let toks = lex::lex("a.b 1.0 1. 2.max 1e3 0x1f 1f32 'a' 'b: r#unsafe b'x' br#\"q\"# ..= :: !=").unwrap();
    let kinds: Vec<String> = toks.iter().map(|t| format!("{:?}:{}", t.kind, t.text)).collect();
    let want = [
        "Ident { raw: false }:a",
        "Dot:.",
        "Ident { raw: false }:b",
        "Lit { kind: Float, suffix: false }:1.0",
        "Lit { kind: Float, suffix: false }:1.",
        "Lit { kind: Int, suffix: false }:2",
        "Dot:.",
        "Ident { raw: false }:max",
        "Lit { kind: Float, suffix: false }:1e3",
        "Lit { kind: Int, suffix: false }:0x1f",
        "Lit { kind: Float, suffix: true }:1f32",
        "Lit { kind: Char, suffix: false }:a",
        "Lifetime:b",
        "Other::",
        "Ident { raw: true }:unsafe",
        "Lit { kind: Byte, suffix: false }:x",
        "Lit { kind: RawByteStr, suffix: false }:q",
        "DotDot:..=",
        "PathSep:::",
        "Other:!=",
    ];
    assert_eq!(kinds, want);
}

// ------------------------------------------------------------------ the pinned rule table

fn kind_name(k: PatternKind) -> String {
    format!("{k:?}")
}

#[test]
fn rule_table_is_pinned_in_node_md() {
    assert_eq!(RULE_TABLE.len(), RULE_TABLE_LEN);
    let node =
        String::from_utf8(read(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/krn/num/lint/syn/NODE.md")))
            .unwrap()
            .replace('\r', "");
    let mut rows = BTreeSet::new();
    for (rule, kind, pat) in &RULE_TABLE {
        let row = format!("| `{}` | {} | `{}` |", rule.id(), kind_name(*kind), pat);
        assert!(node.contains(&row), "NODE.md Design does not pin {row}");
        assert!(rows.insert(row.clone()), "duplicate rule-table row {row}");
        assert!(rule.is_waivable(), "{row}");
    }
    let pinned = node.lines().filter(|l| l.starts_with("| `KNP1-")).count();
    assert_eq!(pinned, RULE_TABLE_LEN, "NODE.md pins rows that the code does not have");
}
