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
fn run(case: &str) -> (Vec<(String, String, usize)>, Vec<LintFinding>) {
    let prefix = format!("{}/", fx().join(case).display().to_string().replace('\\', "/"));
    let report = lint(&inputs(case));
    let mut got: Vec<(String, String, usize)> = report
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

fn expected() -> Vec<(String, Vec<(String, String, usize)>)> {
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
