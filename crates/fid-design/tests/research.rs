//! Acceptance criteria of spec node UX.RESEARCH.PROTO (AC-1 .. AC-4), shipped
//! `design/research/{plan,protocol,consent,data-handling}.md`, privacy/redaction guarantees,
//! capture sidecars, incumbent/consent rules and exit-2 error paths.
//! Expected results are hand-labelled in `fixtures/ux/research/plan/expected.json`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::research::{self, REPORT_FORMAT};
use std::path::PathBuf;
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/research/plan";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> String {
    repo().join(FX).join(rel).to_string_lossy().into_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(repo()).args(args).output().expect("run fid-design")
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

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not an object at {k}"),
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
    let CValue::Arr(a) = get(report, "findings") else { panic!("findings not an array") };
    let mut out = Vec::new();
    for f in a {
        let (CValue::Str(c), CValue::Str(s)) = (get(f, "code"), get(f, "subject")) else { panic!("bad finding") };
        let k = format!("{c} {s}");
        if out.last() != Some(&k) {
            out.push(k);
        }
    }
    out
}

fn run_case(key: &str) -> Output {
    let exp = expected();
    let e = get(&exp, key);
    let CValue::Str(root_rel) = get(e, "root") else { panic!("root") };
    let CValue::Str(key_rel) = get(e, "key") else { panic!("key") };
    let o = run(&["research-check", "--root", &fix(root_rel), "--key", &fix(key_rel), "--json"]);
    let CValue::Num(code) = get(e, "exit") else { panic!("exit") };
    assert_eq!(f64::from(o.status.code().unwrap()), *code, "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(get(&r, "format"), &CValue::Str(REPORT_FORMAT.into()));
    assert_eq!(get(&r, "ok"), &CValue::Bool(*code == 0.0), "{key}");
    assert_eq!(finding_keys(&r), strs(get(e, "findings")), "{key}");
    o
}

/// AC-1: `clean/` with `key.csv` exits 0 and is byte-identical across runs.
#[test]
fn ac1_clean_fixture_passes() {
    let a = run_case("clean");
    let b = run(&["research-check", "--root", &fix("clean"), "--key", &fix("key.csv"), "--json"]);
    assert_eq!(a.stdout, b.stdout);
    let t = run(&["research-check", "--root", &fix("clean"), "--key", &fix("key.csv")]);
    assert_eq!(t.status.code(), Some(0));
    assert!(stdout(&t).contains("ok: no findings"), "{}", stdout(&t));
}

/// AC-2: `has-email/` exits 1 with `RESEARCH-PII email` and never echoes the e-mail address.
#[test]
fn ac2_email_pii_fails_without_echo() {
    let o = run_case("has-email");
    let out = stdout(&o);
    assert!(!out.contains("synthetic.participant@example.invalid"), "email leaked: {out}");
    let t = run(&["research-check", "--root", &fix("has-email"), "--key", &fix("key.csv")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("RESEARCH-PII email"), "{}", stdout(&t));
    assert!(!stdout(&t).contains("synthetic.participant@example.invalid"));
}

/// AC-3: `has-name/` exits 1 with `RESEARCH-PII name` and NEVER prints the matched name.
#[test]
fn ac3_name_pii_fails_without_echoing_key() {
    let o = run_case("has-name");
    let out = stdout(&o);
    let err = String::from_utf8_lossy(&o.stderr);
    for secret in ["Jordan", "Kestrel", "Nadia", "Oakhurst"] {
        assert!(!out.contains(secret), "key leaked in stdout: {out}");
        assert!(!err.contains(secret), "key leaked in stderr: {err}");
    }
    let t = run(&["research-check", "--root", &fix("has-name"), "--key", &fix("key.csv")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("RESEARCH-PII name"), "{}", stdout(&t));
    assert!(!stdout(&t).contains("Jordan"));
    assert!(!stdout(&t).contains("Kestrel"));
}

/// AC-4: `screenshot/` (PNG without capture sidecar) exits 1 with `RESEARCH-IMAGE-PROVENANCE`.
#[test]
fn ac4_screenshot_without_sidecar_fails() {
    run_case("screenshot");
    let t = run(&["research-check", "--root", &fix("screenshot")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("RESEARCH-IMAGE-PROVENANCE"), "{}", stdout(&t));
}

/// Additional error fixtures: phone PII, incumbent artefact, missing consent_ref.
#[test]
fn extra_error_fixtures() {
    let o = run_case("has-phone");
    assert!(!stdout(&o).contains("555-019-2834"), "phone leaked: {}", stdout(&o));
    run_case("incumbent");
    run_case("missing-consent");
}

/// Wider phone formats: `.` separators, 3-4 local numbers and trunk-prefixed national
/// groupings, each reported without echoing the number.
#[test]
fn phone_formats_dots_local_and_international() {
    for (case, number) in [
        ("phone-dots", "555.019.2834"),
        ("phone-local", "555-0192"),
        ("phone-uk", "07700 900123"),
        ("phone-de", "030 12345678"),
    ] {
        let o = run_case(case);
        assert!(!stdout(&o).contains(number), "{case}: phone leaked: {}", stdout(&o));
    }
}

/// A name part of 3+ characters from the key (here a surname alone) is a name finding, and is
/// never echoed.
#[test]
fn name_part_from_key_is_found() {
    let o = run_case("has-name-part");
    assert!(!stdout(&o).contains("Kestrel"), "{}", stdout(&o));
    let tmp = temp("name-part");
    std::fs::write(tmp.join("notes.md"), "Spoke with nadia about the probe.\n").unwrap();
    let o = run(&["research-check", "--root", tmp.to_str().unwrap(), "--key", &fix("key.csv"), "--json"]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert_eq!(finding_keys(&json_out(&o)), ["RESEARCH-PII name"]);
    // A part inside a longer word is not a match ("Jordanian" is not "Jordan").
    std::fs::write(tmp.join("notes.md"), "Jordanian standards were compared.\n").unwrap();
    let o = run(&["research-check", "--root", tmp.to_str().unwrap(), "--key", &fix("key.csv"), "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
}

/// Without `--key` the report says names were not checked: a notice in text output and on
/// stderr with `--json`, and `names_checked: false` in the JSON.
#[test]
fn running_without_key_says_names_were_not_checked() {
    let t = run(&["research-check", "--root", &fix("has-name")]);
    assert_eq!(t.status.code(), Some(0), "{}", stdout(&t));
    assert!(stdout(&t).contains("names were NOT checked"), "{}", stdout(&t));
    let j = run(&["research-check", "--root", &fix("has-name"), "--json"]);
    assert!(String::from_utf8_lossy(&j.stderr).contains("names were NOT checked"));
    assert_eq!(get(&json_out(&j), "names_checked"), &CValue::Bool(false));
    let k = run(&["research-check", "--root", &fix("has-name"), "--key", &fix("key.csv"), "--json"]);
    assert_eq!(get(&json_out(&k), "names_checked"), &CValue::Bool(true));
    assert!(!String::from_utf8_lossy(&k.stderr).contains("NOT checked"));
}

/// Shipped `design/research/{plan,protocol,consent,data-handling}.md` pass research-check and
/// cover every requirement REQ-001..005.
#[test]
fn shipped_research_docs_pass_and_cover_requirements() {
    let o = run(&["research-check", "--root", "design/research", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));

    let plan = std::fs::read_to_string(repo().join("design/research/plan.md")).unwrap();
    for needle in ["{{FAMILY_NAME}}", "telemetry", "opted in", "CONSTITUTION C7", "own words", "research-check"] {
        assert!(plan.to_lowercase().contains(&needle.to_lowercase()), "missing {needle:?} in plan.md");
    }

    let proto = std::fs::read_to_string(repo().join("design/research/protocol.md")).unwrap();
    for i in 1..=9 {
        let id = format!("JOB.PER.0{i}");
        assert!(proto.contains(&id), "missing persona {id} in protocol.md");
    }
    for needle in [
        "ux-research-protocol@1",
        "JOB.ACC.01",
        "JOB.ACC.02",
        "JOB.ACC.03",
        // Severity scale owner (its file is not published yet, so no path is linked).
        "UX.RESEARCH.USAB",
        "System Usability Scale",
        "P-<study>-<NN>",
    ] {
        assert!(proto.contains(needle), "missing {needle:?} in protocol.md");
    }

    let consent = std::fs::read_to_string(repo().join("design/research/consent.md")).unwrap();
    for needle in ["purpose", "recorded", "storage", "withdraw", "separate", "opt-in", "P-<study>-<NN>", "[unverified]"]
    {
        assert!(consent.to_lowercase().contains(&needle.to_lowercase()), "missing {needle:?} in consent.md");
    }

    let dh = std::fs::read_to_string(repo().join("design/research/data-handling.md")).unwrap();
    for needle in
        ["P-<study>-<NN>", "outside the public repository", "deletion", "telemetry", "capture.json", "research-check"]
    {
        assert!(dh.to_lowercase().contains(&needle.to_lowercase()), "missing {needle:?} in data-handling.md");
    }
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-research-check-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Path redaction, bad capture command, orphan sidecar, incumbent.txt and empty key fail closed.
#[test]
fn privacy_redaction_and_edge_cases() {
    // Path containing a participant name is redacted to `<withheld>` and never echoed.
    let d = temp("path-redact");
    std::fs::write(d.join("Jordan Kestrel.md"), "# Notes\n\nClean body.\n").unwrap();
    let o = run(&["research-check", "--root", d.to_str().unwrap(), "--key", &fix("key.csv"), "--json"]);
    assert_eq!(o.status.code(), Some(1));
    let out = stdout(&o);
    assert!(out.contains("<withheld>"), "{out}");
    assert!(!out.contains("Jordan"), "path leaked name: {out}");
    assert!(!out.contains("Kestrel"), "path leaked name: {out}");

    // Non-project capture command fails with RESEARCH-IMAGE-PROVENANCE.
    let d2 = temp("bad-cap");
    std::fs::copy(repo().join(FX).join("screenshot/shot.png"), d2.join("shot.png")).unwrap();
    std::fs::write(
        d2.join("shot.png.capture.json"),
        r#"{"command":"scrot shot.png","date":"2026-11-22","format":"research-capture@1","image":"shot.png"}"#,
    )
    .unwrap();
    let r2 = research::check(&d2, None).unwrap();
    assert_eq!(r2.findings.len(), 1);
    assert_eq!(r2.findings[0].code, research::IMAGE_PROVENANCE);

    // Orphan capture sidecar fails with RESEARCH-SCHEMA.
    let d3 = temp("orphan-cap");
    std::fs::write(
        d3.join("gone.png.capture.json"),
        r#"{"command":"fid viz capture","date":"2026-11-22","format":"research-capture@1","image":"gone.png"}"#,
    )
    .unwrap();
    let r3 = research::check(&d3, None).unwrap();
    assert_eq!(r3.findings[0].code, research::SCHEMA);

    // incumbent.txt lists a flagged file.
    let d4 = temp("inc-list");
    std::fs::write(d4.join("incumbent.txt"), "vendor-notes.md\n").unwrap();
    std::fs::write(d4.join("vendor-notes.md"), "Clean text.\n").unwrap();
    let r4 = research::check(&d4, None).unwrap();
    assert_eq!(r4.findings[0].code, research::INCUMBENT);
    assert_eq!(r4.findings[0].subject, "vendor-notes.md");

    // Empty key.csv fails closed with RESEARCH-SCHEMA.
    let d5 = temp("empty-key");
    let empty_key = d5.join("empty.csv");
    std::fs::write(&empty_key, "code,name\n").unwrap();
    let r5 = research::check(&repo().join(FX).join("clean"), Some(&empty_key)).unwrap();
    assert_eq!(r5.findings[0].code, research::SCHEMA);
}

/// Bad CLI usage and unreadable inputs exit 2.
#[test]
fn bad_usage_and_unreadable_exit_2() {
    let key = fix("key.csv");
    for args in [
        vec!["research-check"],
        vec!["research-check", "--key", &key],
        vec!["research-check", "--root", &fix("clean"), "--all"],
        vec!["research-check", "--root", &fix("clean"), "--from", "x"],
        vec!["pkg-check", "--root", &fix("clean"), "--key", &key],
        vec!["research-check", "--root", "no/such/dir"],
        vec!["research-check", "--root", &fix("clean"), "--key", "no/such/key.csv"],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
}
