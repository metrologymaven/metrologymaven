//! Acceptance criteria of spec node UX.SYSTEM.PRINC (AC-1 .. AC-4) and further error paths.
//! Expected results are hand-labelled in `fixtures/ux/system/princ/expected.json` (written by
//! `make_fixtures.py` from the leaf's rules and cross-checked there by an independent Python
//! reading), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> String {
    root().join("fixtures/ux/system/princ").join(rel).to_string_lossy().into_owned()
}

fn shipped(rel: &str) -> String {
    root().join("design").join(rel).to_string_lossy().into_owned()
}

/// Runs the binary from the repository root, so the default paths resolve.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(root()).args(args).output().expect("run fid-design")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn json_out(o: &Output) -> CValue {
    fid_canon::parse(stdout(o).trim_end().as_bytes()).unwrap()
}

fn expected() -> CValue {
    fid_canon::parse(&std::fs::read(fix("expected.json")).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, path: &[&str]) -> &'a CValue {
    let mut cur = v;
    for k in path {
        match cur {
            CValue::Obj(m) => cur = m.get(*k).unwrap_or_else(|| panic!("missing key {k}")),
            _ => panic!("not an object at {k}"),
        }
    }
    cur
}

fn num(v: &CValue) -> i64 {
    match v {
        CValue::Num(n) => *n as i64,
        other => panic!("not a number: {other:?}"),
    }
}

fn strs(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                other => panic!("not a string: {other:?}"),
            })
            .collect(),
        other => panic!("not an array: {other:?}"),
    }
}

fn finding_keys(report: &CValue) -> Vec<String> {
    match get(report, &["findings"]) {
        CValue::Arr(a) => a
            .iter()
            .map(|f| match (get(f, &["code"]), get(f, &["subject"])) {
                (CValue::Str(c), CValue::Str(s)) => format!("{c} {s}"),
                _ => panic!("bad finding"),
            })
            .collect(),
        _ => panic!("findings not an array"),
    }
}

fn princ_case(name: &str, args: &[&str]) {
    let exp = expected();
    let e = get(&exp, &["princ", name]);
    let o = run(args);
    assert_eq!(i64::from(o.status.code().unwrap()), num(get(e, &["exit"])), "{name}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(e, &["findings"])), "{name}");
    assert_eq!(strs(get(&r, &["principles"])), strs(get(e, &["principles"])), "{name}");
    assert_eq!(get(&r, &["format"]), &CValue::Str("fid-design-princ-check@1".into()));
}

fn words_case(key: &str) {
    let exp = expected();
    let e = get(&exp, &["words", key]);
    let copy = key.split('+').next().unwrap();
    let words = match get(e, &["words"]) {
        CValue::Str(s) if s == "shipped" => shipped("reserved-words.json"),
        CValue::Str(s) => fix(s),
        _ => panic!("bad words"),
    };
    let o = run(&["words-check", "--copy", &fix(copy), "--words", &words, "--json"]);
    assert_eq!(i64::from(o.status.code().unwrap()), num(get(e, &["exit"])), "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(e, &["findings"])), "{key}");
    assert_eq!(num(get(&r, &["messages"])), num(get(e, &["messages"])), "{key}");
}

#[test]
fn ac1_shipped_principles_pass_and_list_six_ids() {
    // Default --file is design/principles.md under the working directory.
    princ_case("shipped", &["princ-check", "--json"]);
    princ_case("shipped", &["princ-check", "--file", &shipped("principles.md"), "--json"]);
    let r = json_out(&run(&["princ-check", "--json"]));
    assert_eq!(strs(get(&r, &["principles"])), ["P-DATA", "P-DENSE", "P-EXPLAIN", "P-FAMILIAR", "P-KEYS", "P-NOCOLOR"]);
    assert_eq!(get(&r, &["ok"]), &CValue::Bool(true));
    let t = stdout(&run(&["princ-check"]));
    assert!(t.contains("ok: no findings"), "{t}");
}

#[test]
fn ac2_missing_dont_names_the_section() {
    princ_case("no-dont.md", &["princ-check", "--file", &fix("no-dont.md"), "--json"]);
    let o = run(&["princ-check", "--file", &fix("no-dont.md")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("PRINC-EXAMPLE-MISSING P-DATA"), "{}", stdout(&o));
}

#[test]
fn princ_heading_without_id_is_reported() {
    princ_case("no-id.md", &["princ-check", "--file", &fix("no-id.md"), "--json"]);
}

#[test]
fn princ_unknown_duplicate_empty_do_and_fenced_heading() {
    princ_case("unknown.md", &["princ-check", "--file", &fix("unknown.md"), "--json"]);
}

#[test]
fn ac3_clean_copy_passes_with_shipped_words() {
    words_case("copy-ok.jsonl");
    // Default --words is design/reserved-words.json under the working directory.
    let o = run(&["words-check", "--copy", &fix("copy-ok.jsonl"), "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert_eq!(num(get(&json_out(&o), &["words"])), 5);
}

#[test]
fn ac4_approve_outside_permitted_prefixes_fails() {
    words_case("copy-bad.jsonl");
    let o = run(&["words-check", "--copy", &fix("copy-bad.jsonl")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("WORD-RESERVED approve agent.proposal.apply_button"), "{}", stdout(&o));
}

#[test]
fn words_malformed_copy_lines_and_empty_export_are_schema_findings() {
    words_case("copy-schema.jsonl");
    words_case("copy-empty.jsonl");
}

#[test]
fn words_obfuscation_fails_closed_and_added_forms_match() {
    words_case("copy-obfuscated.jsonl");
}

#[test]
fn words_malformed_or_incomplete_word_lists() {
    words_case("words-bad-copy.jsonl");
    words_case("copy-ok.jsonl+words-object.json");
    words_case("copy-ok.jsonl+words-empty.json");
}

#[test]
fn shipped_words_meet_req_004() {
    let v = fid_canon::parse(&std::fs::read(shipped("reserved-words.json")).unwrap()).unwrap();
    let CValue::Arr(entries) = v else { panic!("not an array") };
    let words: Vec<String> = entries
        .iter()
        .map(|e| match get(e, &["word"]) {
            CValue::Str(s) => s.clone(),
            other => panic!("word not a string: {other:?}"),
        })
        .collect();
    for w in ["approve", "release", "sign", "conforms", "verified"] {
        assert!(words.iter().any(|x| x == w), "{w} missing");
    }
    for e in &entries {
        assert!(matches!(get(e, &["meaning"]), CValue::Str(s) if !s.is_empty()));
        let prefixes = strs(get(e, &["allowed_prefixes"]));
        assert!(!prefixes.is_empty());
        assert!(prefixes.iter().all(|p| p.ends_with('.')));
        assert_eq!(get(e, &["case_insensitive"]), &CValue::Bool(true));
    }
    // The shipped list itself raises no schema finding.
    let text = std::fs::read_to_string(shipped("reserved-words.json")).unwrap();
    assert!(fid_design::words::parse_words(&text).findings.is_empty());
}

#[test]
fn writing_guide_has_required_sections() {
    let g = std::fs::read_to_string(shipped("writing-guide.md")).unwrap();
    for h in ["## Voice", "## Preferred terms", "## Reserved words", "## Numbers and units", "## Command titles"] {
        assert!(g.lines().any(|l| l.trim_end() == h), "{h} missing");
    }
    // Every reserved word in the machine list appears in the guide's table.
    for w in ["approve", "conforms", "release", "sign", "verified"] {
        assert!(g.contains(&format!("| {w} |")), "{w} not in the guide table");
    }
}

#[test]
fn unreadable_input_exits_2() {
    let o = run(&["princ-check", "--file", &fix("does-not-exist.md")]);
    assert_eq!(o.status.code(), Some(2));
    let o = run(&["words-check", "--copy", &fix("does-not-exist.jsonl")]);
    assert_eq!(o.status.code(), Some(2));
    let o = run(&["words-check", "--copy", &fix("copy-ok.jsonl"), "--words", &fix("missing.json")]);
    assert_eq!(o.status.code(), Some(2));

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fid-design-princ");
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("big.md");
    std::fs::write(&big, vec![b'a'; 1024 * 1024 + 1]).unwrap();
    let o = run(&["princ-check", "--file", &big.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(2));
    let bin = dir.join("latin1.jsonl");
    std::fs::write(&bin, b"{\"id\":\"a\",\"text\":\"\xe9\"}\n").unwrap();
    let o = run(&["words-check", "--copy", &bin.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn bad_usage_exits_2() {
    assert_eq!(run(&["words-check"]).status.code(), Some(2));
    assert_eq!(run(&["princ-check", "--copy", &fix("copy-ok.jsonl")]).status.code(), Some(2));
    assert_eq!(run(&["pkg-check", "--file", &fix("no-dont.md")]).status.code(), Some(2));
    assert_eq!(run(&["princ-check", "--root", "x"]).status.code(), Some(2));
}

#[test]
fn library_matches_whole_words_case_insensitively() {
    let words = std::fs::read_to_string(shipped("reserved-words.json")).unwrap();
    let copy = "{\"id\":\"viz.a\",\"text\":\"pre-approved\"}\n{\"id\":\"viz.b\",\"text\":\"preapproved\"}\n";
    let r = fid_design::words::check_text(copy, &words);
    let keys: Vec<String> = r.findings.iter().map(|f| format!("{} {}", f.code, f.subject)).collect();
    assert_eq!(keys, ["WORD-RESERVED approve viz.a", "WORD-RESERVED approve viz.b"]);
    // Output is deterministic.
    assert_eq!(r, fid_design::words::check_text(copy, &words));
}

#[test]
fn obfuscation_reasons_and_accented_latin() {
    use fid_design::words::obfuscation;
    assert!(obfuscation("Gr\u{f6}\u{df}e pr\u{fc}fen, v\u{e9}rifier, \u{c5}ngstr\u{f6}m").is_empty());
    assert!(obfuscation("Export report\u{2026} 12.004 mm \u{b1} 0.5\u{b0}").is_empty());
    for bad in [
        "a\u{ad}b",
        "a\u{34f}b",
        "a\u{200d}b",
        "a\u{202a}b",
        "a\u{206f}b",
        "\u{feff}a",
        "\u{fb00}",
        "\u{ff41}",
        "\u{1d7ff}",
    ] {
        assert_eq!(obfuscation(bad).len(), 1, "{bad:?}");
    }
    assert!(!obfuscation("\u{3b1}pprove").is_empty());
    assert!(obfuscation("\u{3b1}\u{3b2}\u{3b3} \u{430}\u{431}").is_empty());
}
