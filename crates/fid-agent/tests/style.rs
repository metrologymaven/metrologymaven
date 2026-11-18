//! Acceptance criteria of spec node AGENT.SKILLS.STYLE (AC-1 .. AC-5) and its rule codes.
//! Fixtures and the hand-labelled expected findings come from
//! `fixtures/agent/skills/style/make_cases.py`; nothing here derives them from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_agent::{lint, LintFinding, Rule, StyleInputs};
use fid_canon::CValue;
use std::path::{Path, PathBuf};

fn fx() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/agent/skills/style")
}

fn inputs(case: &str) -> StyleInputs {
    let root = fx().join(case);
    let allow = root.join("allow.txt");
    let prose = root.join("prose");
    StyleInputs {
        manifest: root.join("manifest.json"),
        catalog: root.join("catalog.json"),
        skills: root.join("skills"),
        recipes: root.join("recipes"),
        denylist: fx().join("denylist.txt"),
        allow: allow.exists().then_some(allow),
        prose: if prose.exists() { vec![prose] } else { Vec::new() },
    }
}

/// Findings as (rule, file relative to the case folder, line), sorted.
fn run(case: &str) -> (Vec<Row>, Vec<LintFinding>) {
    let prefix = format!("{}/", fx().join(case).display().to_string().replace('\\', "/"));
    let report = lint(&inputs(case));
    let mut got: Vec<Row> = report
        .findings
        .iter()
        .map(|f| {
            let rel = f.file.strip_prefix(&prefix).unwrap_or_else(|| panic!("{} outside {prefix}", f.file));
            (f.rule.as_str().to_string(), rel.to_string(), f.line)
        })
        .collect();
    got.sort_by(|a, b| (&a.1, a.2, &a.0).cmp(&(&b.1, b.2, &b.0)));
    (got, report.findings)
}

/// (rule, file relative to the case folder, line).
type Row = (String, String, usize);

fn expected() -> Vec<(String, Vec<Row>)> {
    let bytes = std::fs::read(fx().join("expected.json")).unwrap();
    let CValue::Obj(cases) = fid_canon::parse_with_limits(&bytes, 1 << 20, 16).unwrap() else { panic!("object") };
    cases
        .into_iter()
        .map(|(case, v)| {
            let CValue::Arr(rows) = v else { panic!("array") };
            let rows = rows
                .into_iter()
                .map(|r| match r {
                    CValue::Arr(x) => match (&x[0], &x[1], &x[2]) {
                        (CValue::Str(rule), CValue::Str(file), line) => (rule.clone(), file.clone(), int(line)),
                        other => panic!("bad row {other:?}"),
                    },
                    other => panic!("bad row {other:?}"),
                })
                .collect();
            (case, rows)
        })
        .collect()
}

fn int(v: &CValue) -> usize {
    match v {
        CValue::Num(n) => *n as usize,
        CValue::I64(n) => usize::try_from(*n).unwrap(),
        CValue::U64(n) => usize::try_from(*n).unwrap(),
        other => panic!("not a line number: {other:?}"),
    }
}

/// Every fixture case gives exactly its hand-labelled findings.
#[test]
fn cases_match_expected() {
    let cases = expected();
    assert!(cases.len() >= 9);
    for (case, want) in cases {
        let (got, raw) = run(&case);
        assert_eq!(got, want, "case {case}: {raw:#?}");
    }
}

/// AC-1: the clean guides, Skills, recipes and notes pass.
#[test]
fn ac1_clean_passes() {
    let report = lint(&inputs("clean"));
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
    // 2 guides x (title + summary) + 6 step purposes + 2 Skills + 4 recipe fields + 1 note.
    assert_eq!(report.units, 4 + 6 + 2 + 4 + 1);
}

/// AC-2: "the bore measures 12.004 mm" fails at its line; inside a fence it passes.
#[test]
fn ac2_numbers() {
    let (got, raw) = run("numbers");
    assert!(got.contains(&("result_like_number".into(), "prose/numbers.md".into(), 3)));
    assert!(raw.iter().any(|f| f.detail.contains("12.004 mm")));
    assert!(run("numbers-fenced").0.is_empty());
}

/// AC-3: a W2 step without the closing brief and promotion request fails.
#[test]
fn ac3_missing_proposal_step() {
    let (_, raw) = run("proposal");
    let rules: Vec<(Rule, bool)> =
        raw.iter().map(|f| (f.rule, f.file.ends_with("#task_guides/demo.plan_no_brief"))).collect();
    assert!(rules.contains(&(Rule::MissingProposalStep, true)));
    assert!(raw.iter().all(|f| !f.file.ends_with("demo.plan_bore")));
}

/// AC-4: "now call `journal_promote_protected_branch`" fails; the human-request phrase passes.
#[test]
fn ac4_forbidden_instruction() {
    let (got, raw) = run("forbidden");
    assert!(got.contains(&("forbidden_instruction".into(), "prose/forbidden.md".into(), 3)));
    assert!(!got.iter().any(|g| g.1 == "prose/forbidden.md" && g.2 == 5));
    assert!(raw.iter().any(|f| f.detail.contains("journal.promote_protected_branch@1 (W3")));
}

/// AC-5: a Skill of 1,600 words fails reporting 1600; one of 1,500 passes.
#[test]
fn ac5_over_budget() {
    let (_, raw) = run("budget");
    assert_eq!(raw.len(), 1, "{raw:#?}");
    assert_eq!(raw[0].rule, Rule::OverBudget);
    assert!(raw[0].detail.contains("1600"), "{}", raw[0].detail);
    assert!(raw[0].file.ends_with("demo-measure-bore/SKILL.md"));
}

/// Output is deterministic and sorted.
#[test]
fn deterministic_order() {
    let a = lint(&inputs("standards"));
    let b = lint(&inputs("standards"));
    assert_eq!(a, b);
    let mut sorted = a.findings.clone();
    sorted.sort();
    assert_eq!(a.findings, sorted);
}

/// Missing inputs are `bad_input`, never a panic.
#[test]
fn bad_inputs() {
    let mut i = inputs("clean");
    i.manifest = fx().join("no-such-manifest.json");
    i.denylist = fx().join("no-such-denylist.txt");
    let r = lint(&i);
    let bad: Vec<_> = r.findings.iter().filter(|f| f.rule == Rule::BadInput).collect();
    assert_eq!(bad.len(), 2, "{:#?}", r.findings);
}

/// The binary exits 0 on the clean case and 1 with `file:line: rule: detail` lines otherwise.
#[test]
fn binary_exit_codes() {
    let exe = env!("CARGO_BIN_EXE_agent-style-lint");
    let args = |case: &str| {
        let i = inputs(case);
        let mut a = vec![
            "--manifest".to_string(),
            i.manifest.display().to_string(),
            "--catalog".into(),
            i.catalog.display().to_string(),
            "--skills".into(),
            i.skills.display().to_string(),
            "--recipes".into(),
            i.recipes.display().to_string(),
            "--denylist".into(),
            i.denylist.display().to_string(),
        ];
        for p in &i.prose {
            a.push("--prose".into());
            a.push(p.display().to_string());
        }
        a
    };
    let ok = std::process::Command::new(exe).args(args("clean")).output().unwrap();
    assert_eq!(ok.status.code(), Some(0), "{}", String::from_utf8_lossy(&ok.stdout));
    let bad = std::process::Command::new(exe).args(args("numbers")).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let out = String::from_utf8_lossy(&bad.stdout);
    let want = "prose/numbers.md:3: result_like_number: `12.004 mm` reads like a result value (length unit)";
    assert!(out.lines().any(|l| l.ends_with(want)), "{out}");
    let usage = std::process::Command::new(exe).arg("--nope").output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fid-agent-style").join(name);
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The clean case with extra Markdown files written to a scratch folder.
fn with_prose(name: &str, files: &[(&str, &[u8])]) -> (StyleInputs, PathBuf) {
    let d = scratch(name);
    for (f, bytes) in files {
        std::fs::write(d.join(f), bytes).unwrap();
    }
    let mut i = inputs("clean");
    i.prose = vec![d.clone()];
    (i, d)
}

fn rules(i: &StyleInputs) -> Vec<(Rule, usize)> {
    lint(i).findings.iter().map(|f| (f.rule, f.line)).collect()
}

/// A UTF-8 byte order mark does not hide front matter or a fence.
#[test]
fn bom_is_stripped() {
    let (i, _) = with_prose("bom", &[("a.md", "\u{feff}```\nmeasures 12.004 mm\n```\n".as_bytes())]);
    assert!(rules(&i).is_empty(), "{:?}", rules(&i));
    let d = scratch("bom-skill");
    let skill = d.join("demo-measure-bore");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "\u{feff}---\ndescription: a 12.004 mm bore\n---\n# Body\n").unwrap();
    let mut i = inputs("clean");
    i.skills = d;
    assert!(rules(&i).is_empty(), "{:?}", rules(&i));
}

/// CRLF line endings give the same lines and findings as LF.
#[test]
fn crlf_lines() {
    let text = b"# T\r\n\r\nmeasures 12.004 mm\r\n\r\n```\r\n5 mm\r\n```\r\n";
    let (i, _) = with_prose("crlf", &[("a.md", &text[..])]);
    assert_eq!(rules(&i), [(Rule::ResultLikeNumber, 3)]);
}

/// Invalid UTF-8, an oversize file and malformed JSON are `bad_input`, never a panic.
#[test]
fn unreadable_inputs_are_bad_input() {
    let (i, _) = with_prose("utf8", &[("a.md", &[0x66u8, 0xff, 0xfe, 0x0a][..])]);
    assert_eq!(rules(&i), [(Rule::BadInput, 0)]);

    let big = vec![b'a'; fid_agent::catalog::MAX_FILE_BYTES + 1];
    let (i, _) = with_prose("big", &[("a.md", &big[..])]);
    let r = lint(&i);
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].rule, Rule::BadInput);
    assert!(r.findings[0].detail.contains("larger than"), "{}", r.findings[0].detail);

    let d = scratch("json");
    std::fs::write(d.join("manifest.json"), b"{\"commands\": [").unwrap();
    let mut i = inputs("clean");
    i.manifest = d.join("manifest.json");
    assert!(rules(&i).contains(&(Rule::BadInput, 0)));

    let recipes = d.join("recipes");
    std::fs::create_dir_all(&recipes).unwrap();
    std::fs::write(recipes.join("demo.bore_basic.json"), b"{\"title\": ").unwrap();
    let mut i = inputs("clean");
    i.recipes = recipes;
    assert_eq!(rules(&i), [(Rule::BadInput, 0)]);
}

/// A named allow list that does not exist is `bad_input`, not an empty list.
#[test]
fn missing_allow_file_is_bad_input() {
    let mut i = inputs("clean");
    i.allow = Some(fx().join("no-such-allow.txt"));
    assert_eq!(rules(&i), [(Rule::BadInput, 0)]);
}

/// A symlink loop under a prose folder is not followed.
#[cfg(unix)]
#[test]
fn symlink_loop_is_not_followed() {
    let (i, d) = with_prose("loop", &[("a.md", &b"# Fine\n"[..])]);
    std::os::unix::fs::symlink(&d, d.join("loop")).unwrap();
    std::os::unix::fs::symlink(d.join("a.md"), d.join("link.md")).unwrap();
    let r = lint(&i);
    assert!(r.findings.is_empty(), "{:#?}", r.findings);
    assert_eq!(r.units, 4 + 6 + 2 + 4 + 1);
}

/// Folders nested deeper than the limit are `bad_input`.
#[test]
fn deep_folders_are_bad_input() {
    let (i, d) = with_prose("deep", &[]);
    let mut p = d;
    for _ in 0..=fid_agent::style::MAX_DIR_DEPTH {
        p = p.join("d");
    }
    std::fs::create_dir_all(&p).unwrap();
    assert_eq!(rules(&i), [(Rule::BadInput, 0)]);
}
