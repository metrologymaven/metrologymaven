//! Acceptance criteria of spec node KRN.NUM.LINT.FIND (AC-1 .. AC-5) and its error paths.
//! Expected reports come from `fixtures/knp1/find/make_fixtures.py`, not from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_knp1_lint::find::{
    apply_waivers, catalogue, lines_digest, load_waivers, parse_waivers, Applied, CrateRoots, ErrorCode, Finding,
    InputDigests, Report, Rule, WAIVER_UNUSED,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/knp1/find").join(rel)
}
fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fix(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}
fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}
fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}
fn st(o: &BTreeMap<String, CValue>, k: &str) -> String {
    match o.get(k) {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("{k}: expected string, got {other:?}"),
    }
}
fn int(o: &BTreeMap<String, CValue>, k: &str) -> u32 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n as u32,
        other => panic!("{k}: expected number, got {other:?}"),
    }
}
fn finding(v: &CValue) -> Finding {
    let o = obj(v);
    Finding {
        rule: Rule::parse(&st(o, "rule")).expect("rule id"),
        crate_name: st(o, "crate_name"),
        crate_version: st(o, "crate_version"),
        file: st(o, "file"),
        line: int(o, "line"),
        column: int(o, "column"),
        excerpt_digest: st(o, "excerpt_digest"),
        message: st(o, "message"),
    }
}
fn findings(rel: &str) -> Vec<Finding> {
    arr(&parse(&read(rel)).unwrap()).iter().map(finding).collect()
}

/// Input digests the Python generator used for the waiver cases.
fn inputs_with(waivers: &[u8]) -> InputDigests {
    InputDigests::of(b"knp1-lint.toml fixture\n", b"Cargo.lock fixture\n", waivers, b"rdl-denylist.json fixture\n")
}
fn roots(dir: &str) -> CrateRoots {
    let mut r = CrateRoots::new();
    r.insert("fid-clarabel", "0.1.0", fix(dir).join("fid-clarabel"));
    r
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_report_matches_expected_bytes() {
    let input = parse(&read("report-v1/input.json")).unwrap();
    let o = obj(&input);
    let fs: Vec<Finding> = arr(o.get("findings").unwrap()).iter().map(finding).collect();
    assert_eq!(fs.len(), 12);
    let i = obj(o.get("inputs").unwrap());
    let inputs = InputDigests {
        config: st(i, "config"),
        lockfile: st(i, "lockfile"),
        waivers: st(i, "waivers"),
        denylist: st(i, "denylist"),
    };
    let expected = read("report-v1/expected-report.json");
    let tool_version = st(o, "tool_version");

    let r = Report::new(&tool_version, inputs.clone(), Applied::without_waivers(fs.clone())).unwrap();
    assert_eq!(
        String::from_utf8(r.to_canonical_bytes().unwrap()).unwrap(),
        String::from_utf8(expected.clone()).unwrap()
    );
    assert_eq!(r.unwaived_count(), 12);
    assert!(!r.passed());

    // Any input order gives the same bytes (REQ-002).
    let mut rev = fs;
    rev.reverse();
    let r2 = Report::new(&tool_version, inputs, Applied::without_waivers(rev)).unwrap();
    assert_eq!(r2.to_canonical_bytes().unwrap(), expected);
}

#[test]
fn report_write_and_write_failure() {
    let dir = std::env::temp_dir().join(format!("fid-knp1-lint-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let r = Report::new("0.0.1", inputs_with(b""), Applied::default()).unwrap();
    let path = dir.join("knp1-findings.json");
    r.write(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), r.to_canonical_bytes().unwrap());
    let bad = r.write(&dir.join("missing-dir").join("knp1-findings.json")).unwrap_err();
    assert_eq!(bad.code, ErrorCode::ReportWriteFailed);
    assert_eq!(bad.code.as_str(), "report_write_failed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_findings_are_rejected() {
    let good = findings("waive-v1/findings.json").remove(0);
    type Mutate = Box<dyn Fn(&mut Finding)>;
    let cases: Vec<(&str, Mutate)> = vec![
        ("absolute path", Box::new(|f| f.file = "/home/ci/src/solver.rs".into())),
        ("drive path", Box::new(|f| f.file = "C:/src/solver.rs".into())),
        ("backslash", Box::new(|f| f.file = "src\\solver.rs".into())),
        ("parent", Box::new(|f| f.file = "../x/src/solver.rs".into())),
        ("line 0", Box::new(|f| f.line = 0)),
        ("column 0", Box::new(|f| f.column = 0)),
        ("bad digest", Box::new(|f| f.excerpt_digest = "sha256:XYZ".into())),
        ("empty crate", Box::new(|f| f.crate_name = String::new())),
        ("rule suffix with space", Box::new(|f| f.rule = Rule::Flag("bad x".into()))),
        ("empty rule suffix", Box::new(|f| f.rule = Rule::Data(String::new()))),
        ("lowercase rule suffix", Box::new(|f| f.rule = Rule::Allow("licence".into()))),
    ];
    for (name, mutate) in cases {
        let mut f = good.clone();
        mutate(&mut f);
        let e = Report::new("0.0.1", inputs_with(b""), Applied::without_waivers(vec![f])).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidFinding, "{name}");
    }
    let mut bad_inputs = inputs_with(b"");
    bad_inputs.lockfile = "md5:00".into();
    assert_eq!(Report::new("0.0.1", bad_inputs, Applied::default()).unwrap_err().code, ErrorCode::InvalidFinding);
    // tool_version is limited to 64 bytes, as in the schema.
    assert!(Report::new(&"9".repeat(64), inputs_with(b""), Applied::default()).is_ok());
    let long = Report::new(&"9".repeat(65), inputs_with(b""), Applied::default()).unwrap_err();
    assert_eq!(long.code, ErrorCode::InvalidFinding);
}

/// A waiver carries no version, so a crate present in two versions is ambiguous and fails closed.
#[test]
fn waiver_for_crate_in_two_versions_fails_schema() {
    let waivers = load_waivers(&read("waivers/valid.toml")).unwrap();
    // Two versions in the source tree, both with the waived text: still ambiguous.
    let mut two = roots("src");
    two.insert("fid-clarabel", "0.2.0", fix("src").join("fid-clarabel"));
    let errs = apply_waivers(findings("waive-v1/findings.json"), &waivers, &two).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, ErrorCode::WaiverSchema);
    assert!(errs[0].message.contains("0.1.0, 0.2.0"), "{}", errs[0]);
    // One version on disk, but a finding from another version of the same crate: ambiguous too.
    let mut other = findings("waive-v1/findings.json").remove(0);
    other.crate_version = "0.0.9".into();
    let mut fs = findings("waive-v1/findings.json");
    fs.push(other);
    let errs = apply_waivers(fs, &waivers, &roots("src")).unwrap_err();
    assert_eq!(errs[0].code, ErrorCode::WaiverSchema);
    // A different crate name in two versions does not affect the waiver.
    let mut unrelated = roots("src");
    unrelated.insert("fid-other", "1.0.0", fix("src"));
    unrelated.insert("fid-other", "2.0.0", fix("src"));
    assert_eq!(apply_waivers(findings("waive-v1/findings.json"), &waivers, &unrelated).unwrap().waived.len(), 1);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_matching_waiver_moves_finding_to_waived() {
    let wbytes = read("waivers/valid.toml");
    let waivers = load_waivers(&wbytes).unwrap();
    assert_eq!(waivers.len(), 1);
    assert_eq!(waivers[0].lines, (10, 12));
    assert_eq!(waivers[0].rule, Rule::Pow);
    let applied = apply_waivers(findings("waive-v1/findings.json"), &waivers, &roots("src")).unwrap();
    assert!(applied.warnings.is_empty());
    let r = Report::new("0.0.1", inputs_with(&wbytes), applied).unwrap();
    assert_eq!(r.unwaived_count(), 0);
    assert_eq!(r.waived.len(), 1);
    assert_eq!(r.waived[0].0.line, 11);
    assert!(r.passed());
    assert_eq!(r.to_canonical_bytes().unwrap(), read("waive-v1/expected-report.json"));
}

#[test]
fn waiver_matches_only_same_crate_file_rule_and_range() {
    let wbytes = read("waivers/valid.toml");
    let waivers = load_waivers(&wbytes).unwrap();
    let applied = apply_waivers(findings("waive-v1/findings-mixed.json"), &waivers, &roots("src")).unwrap();
    assert_eq!(applied.waived.len(), 1);
    assert_eq!(applied.findings.len(), 3);
    let r = Report::new("0.0.1", inputs_with(&wbytes), applied).unwrap();
    assert!(!r.passed());
    assert_eq!(r.to_canonical_bytes().unwrap(), read("waive-v1/expected-report-mixed.json"));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_edited_line_makes_waiver_stale() {
    let waivers = load_waivers(&read("waivers/valid.toml")).unwrap();
    let errs = apply_waivers(findings("waive-v1/findings.json"), &waivers, &roots("src-edited")).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, ErrorCode::WaiverStale);
    assert_eq!(errs[0].code.as_str(), "KNP1-WAIVER-STALE");
    assert!(errs[0].message.contains("waiver #1 (fid-clarabel src/solver.rs lines 10-12 KNP1-POW)"), "{}", errs[0]);
    // Stale whether or not a finding still occurs there (REQ-005).
    let errs = apply_waivers(Vec::new(), &waivers, &roots("src-edited")).unwrap_err();
    assert_eq!(errs[0].code, ErrorCode::WaiverStale);
}

#[test]
fn stale_when_file_missing_or_range_past_end() {
    let mut waivers = load_waivers(&read("waivers/valid.toml")).unwrap();
    let empty = CrateRoots::new();
    assert_eq!(apply_waivers(Vec::new(), &waivers, &empty).unwrap_err()[0].code, ErrorCode::WaiverStale);
    waivers[0].lines = (20, 99);
    assert_eq!(apply_waivers(Vec::new(), &waivers, &roots("src")).unwrap_err()[0].code, ErrorCode::WaiverStale);
}

#[test]
fn line_digest_is_lf_normalised() {
    let lf = read("src/fid-clarabel/src/solver.rs");
    let crlf: Vec<u8> = String::from_utf8(lf.clone()).unwrap().replace('\n', "\r\n").into_bytes();
    let waivers = load_waivers(&read("waivers/valid.toml")).unwrap();
    assert_eq!(lines_digest(&lf, 10, 12).as_deref(), Some(waivers[0].lines_sha256.as_str()));
    assert_eq!(lines_digest(&crlf, 10, 12), lines_digest(&lf, 10, 12));
    // No trailing newline: same lines, same digest.
    assert_eq!(lines_digest(lf.strip_suffix(b"\n").unwrap(), 10, 12), lines_digest(&lf, 10, 12));
    assert_eq!(lines_digest(&lf, 0, 1), None);
    assert_eq!(lines_digest(&lf, 3, 2), None);
    let mut mem: BTreeMap<(String, String, String), Vec<u8>> = BTreeMap::new();
    mem.insert(("fid-clarabel".into(), "0.1.0".into(), "src/solver.rs".into()), crlf);
    let applied = apply_waivers(findings("waive-v1/findings.json"), &waivers, &mem).unwrap();
    assert_eq!(applied.waived.len(), 1);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_waiver_without_reviewer_fails_schema() {
    let e = load_waivers(&read("waivers/no-reviewer.toml")).unwrap_err();
    assert_eq!(e.code, ErrorCode::WaiverSchema);
    assert_eq!(e.code.as_str(), "KNP1-WAIVER-SCHEMA");
    assert!(e.message.contains("waiver #1") && e.message.contains("reviewer"), "{e}");
}

#[test]
fn malformed_waivers_fail_schema() {
    let valid = String::from_utf8(read("waivers/valid.toml")).unwrap();
    let without = |key: &str| {
        let prefix = format!("{key} =");
        valid.lines().filter(|l| !l.starts_with(&prefix)).collect::<Vec<_>>().join("\n")
    };
    let cases: Vec<(&str, String)> = vec![
        ("no crate", without("crate")),
        ("no file", without("file")),
        ("no lines", without("lines")),
        ("no rule", without("rule")),
        ("no justification", without("justification")),
        ("no date", without("date")),
        ("no digest", without("lines_sha256")),
        ("blank justification", valid.replace("Vendored Clarabel powf; input range reviewed bit-stable.", "  ")),
        ("blank reviewer", valid.replace("maintainer-a", "")),
        ("reversed range", valid.replace("[10, 12]", "[12, 10]")),
        ("zero line", valid.replace("[10, 12]", "[0, 12]")),
        ("three numbers", valid.replace("[10, 12]", "[10, 11, 12]")),
        ("unknown rule", valid.replace("KNP1-POW", "KNP1-POWER")),
        ("absolute file", valid.replace("\"src/solver.rs\"", "\"/src/solver.rs\"")),
        ("bad date", valid.replace("2026-11-17", "17.11.2026")),
        ("bad digest", valid.replace("sha256:3d", "sha256:3D")),
        ("unknown field", valid.replace("[[waiver]]\n", "[[waiver]]\nexpires = \"never\"\n")),
        ("unknown table", format!("{valid}\n[other]\nx = 1\n")),
        ("not toml", "[[waiver]\n".to_string()),
        ("waiver not array", "waiver = 3\n".to_string()),
    ];
    for (name, text) in cases {
        let e = parse_waivers(&text).unwrap_err();
        assert_eq!(e.code, ErrorCode::WaiverSchema, "{name}");
    }
    assert_eq!(load_waivers(&[0xff, 0xfe]).unwrap_err().code, ErrorCode::WaiverSchema);
    assert_eq!(load_waivers("#".repeat(2 * 1024 * 1024).as_bytes()).unwrap_err().code, ErrorCode::WaiverSchema);
    // A bare TOML date is accepted as well as a string.
    let bare = parse_waivers(&valid.replace("date = \"2026-11-17\"", "date = 2026-11-17")).unwrap();
    assert_eq!(bare[0].date, "2026-11-17");
    assert!(parse_waivers("").unwrap().is_empty());
}

/// Calendar dates, hand-labelled in `waivers/dates/expected.json` by make_fixtures.py.
#[test]
fn waiver_dates_must_exist_on_the_calendar() {
    let labels = parse(&read("waivers/dates/expected.json")).unwrap();
    let labels = obj(&labels);
    assert_eq!(labels.len(), 7);
    for (name, ok) in labels {
        let got = load_waivers(&read(&format!("waivers/dates/{name}.toml")));
        match ok {
            CValue::Bool(true) => assert!(got.is_ok(), "{name}: {got:?}"),
            CValue::Bool(false) => assert_eq!(got.unwrap_err().code, ErrorCode::WaiverSchema, "{name}"),
            other => panic!("{name}: label {other:?}"),
        }
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_unused_waiver_is_a_warning_only() {
    let wbytes = read("waivers/unused.toml");
    let waivers = load_waivers(&wbytes).unwrap();
    assert_eq!(waivers.len(), 2);
    let applied = apply_waivers(findings("waive-v1/findings.json"), &waivers, &roots("src")).unwrap();
    assert_eq!(applied.warnings.len(), 1);
    assert_eq!(applied.warnings[0].code, WAIVER_UNUSED);
    assert_eq!(applied.warnings[0].code, "KNP1-WAIVER-UNUSED");
    assert!(applied.warnings[0].message.contains("waiver #2"), "{}", applied.warnings[0].message);
    let r = Report::new("0.0.1", inputs_with(&wbytes), applied).unwrap();
    assert!(r.passed());
    assert_eq!(r.to_canonical_bytes().unwrap(), read("waive-v1/expected-report-unused.json"));
}

// ------------------------------------------------------------------ rule catalogue

#[test]
fn rule_ids_are_stable() {
    let fixed = [
        ("KNP1-FMA", Rule::Fma),
        ("KNP1-POW", Rule::Pow),
        ("KNP1-LIBM", Rule::Libm),
        ("KNP1-MINMAX", Rule::MinMax),
        ("KNP1-HASH", Rule::Hash),
        ("KNP1-SIMD", Rule::Simd),
        ("KNP1-UNSAFE", Rule::Unsafe),
        ("KNP1-IO-FS", Rule::IoFs),
        ("KNP1-IO-NET", Rule::IoNet),
        ("KNP1-CLOCK", Rule::Clock),
        ("KNP1-ENV", Rule::Env),
        ("KNP1-THREAD", Rule::Thread),
    ];
    for (id, rule) in fixed {
        assert_eq!(Rule::parse(id), Some(rule.clone()));
        assert_eq!(rule.id(), id);
        assert!(!rule.description().is_empty());
    }
    assert_eq!(Rule::parse("KNP1-FLAG-TARGET-CPU"), Some(Rule::Flag("TARGET-CPU".into())));
    assert_eq!(Rule::parse("KNP1-ALLOW-LICENCE").map(|r| r.id()).as_deref(), Some("KNP1-ALLOW-LICENCE"));
    assert_eq!(Rule::parse("KNP1-DATA-CONST2"), Some(Rule::Data("CONST2".into())));
    for bad in [
        "",
        "KNP1-",
        "KNP1-FLAG-",
        "KNP1-flag-x",
        "KNP1-FLAG-x",
        "KNP1-POWX",
        "KNP1-DATA-A--B",
        "KNP1-DATA-A-",
        "knp1-pow",
    ] {
        assert_eq!(Rule::parse(bad), None, "{bad}");
    }
    let cat = catalogue();
    assert_eq!(cat.len(), 15);
    for pat in ["KNP1-FLAG-*", "KNP1-ALLOW-*", "KNP1-DATA-*"] {
        assert!(cat.iter().any(|(id, d)| id == pat && !d.is_empty()));
    }
}
