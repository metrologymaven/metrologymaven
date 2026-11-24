//! Acceptance criteria of spec node UX.SYSTEM.TOOL (AC-1 .. AC-4), shipped `design/sources.json`
//! and `design/CONTRIBUTING.md`, `--spec` verification, schema/guard checks and exit-2 error paths.
//! Expected results are hand-labelled in `fixtures/ux/system/tool/expected.json` (written by
//! `make_fixtures.py` and cross-checked there by an independent Python oracle).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::sources::{self, check, REPORT_FORMAT};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/system/tool";

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
    let o = run(&["sources-check", "--root", &fix(root_rel), "--json"]);
    let CValue::Num(code) = get(e, "exit") else { panic!("exit") };
    assert_eq!(f64::from(o.status.code().unwrap()), *code, "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(get(&r, "format"), &CValue::Str(REPORT_FORMAT.into()));
    assert_eq!(get(&r, "ok"), &CValue::Bool(*code == 0.0), "{key}");
    assert_eq!(get(&r, "tools"), get(e, "tools"), "{key}");
    assert_eq!(get(&r, "sources"), get(e, "sources"), "{key}");
    assert_eq!(get(&r, "exports_checked"), get(e, "exports_checked"), "{key}");
    assert_eq!(finding_keys(&r), strs(get(e, "findings")), "{key}");
    o
}

/// AC-1: `ok/` has two sources with committed SVG, PDF and tool exports and matching digests.
#[test]
fn ac1_ok_fixture_passes() {
    let a = run_case("ok");
    // Determinism (D0): repeated run produces identical canonical bytes.
    let b = run(&["sources-check", "--root", &fix("ok"), "--json"]);
    assert_eq!(a.stdout, b.stdout);
    // Text mode exits 0 and reports ok.
    let t = run(&["sources-check", "--root", &fix("ok")]);
    assert_eq!(t.status.code(), Some(0));
    assert!(stdout(&t).contains("ok: no findings"), "{}", stdout(&t));
    // Also passes when verified against the real spec/ directory.
    let s = run(&["sources-check", "--root", &fix("ok"), "--spec", "spec", "--json"]);
    assert_eq!(s.status.code(), Some(0), "{}", stdout(&s));
}

/// AC-2: `no-export/` and `missing-file/` exit 1 with `SOURCE-EXPORT-MISSING <id>`.
#[test]
fn ac2_export_missing() {
    run_case("no-export");
    let t = run(&["sources-check", "--root", &fix("no-export")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("SOURCE-EXPORT-MISSING screen-inspect-v1"), "{}", stdout(&t));

    run_case("missing-file");
    let t2 = run(&["sources-check", "--root", &fix("missing-file")]);
    assert_eq!(t2.status.code(), Some(1));
    assert!(stdout(&t2).contains("SOURCE-EXPORT-MISSING icon-missing-v1"), "{}", stdout(&t2));
}

/// AC-3: `stale-export/` exits 1 with `SOURCE-EXPORT-DIGEST <id>`.
#[test]
fn ac3_stale_export_digest() {
    run_case("stale-export");
    let t = run(&["sources-check", "--root", &fix("stale-export")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("SOURCE-EXPORT-DIGEST icon-stale-v1"), "{}", stdout(&t));
}

/// AC-4: `paid-tool/` exits 1 with `SOURCE-PAID-TOOL <id>`.
#[test]
fn ac4_paid_tool() {
    run_case("paid-tool");
    let t = run(&["sources-check", "--root", &fix("paid-tool")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("SOURCE-PAID-TOOL mockup-paid-v1"), "{}", stdout(&t));
}

/// Unknown UX spec node and unsupported export format error paths.
#[test]
fn unknown_node_and_bad_format() {
    run_case("unknown-node");
    run_case("bad-format");
}

/// REQ-UX.SYSTEM.TOOL-002: a source whose only export is the tool's own format fails.
#[test]
fn tool_format_only_export_is_missing_open_export() {
    run_case("tool-export-only");
    let t = run(&["sources-check", "--root", &fix("tool-export-only")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("SOURCE-EXPORT-MISSING report-penpot-only-v1"), "{}", stdout(&t));
}

/// A tool row declaring `fig`: `.fig` alone fails, `.fig` plus SVG passes.
#[test]
fn fig_tool_row_needs_an_open_export_too() {
    run_case("fig-tool");
    let r = check(&repo().join(FX).join("fig-tool"), None).unwrap();
    let keys: Vec<(String, String)> = r.findings.iter().map(|f| (f.code.to_string(), f.subject.clone())).collect();
    assert_eq!(keys, [(sources::EXPORT_MISSING.to_string(), "fig-only-v1".to_string())]);
}

/// Shipped `design/sources.json`, `design/CONTRIBUTING.md` and `.github/ISSUE_TEMPLATE/design-proposal.yml`.
#[test]
fn shipped_design_tool_files() {
    let o = run(&["sources-check", "--root", "design", "--spec", "spec", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    let r = json_out(&o);
    assert_eq!(get(&r, "ok"), &CValue::Bool(true));

    // REQ-001: Penpot is in the shipped tool table as open_source = true, requires_paid_account = false.
    let raw = std::fs::read_to_string(repo().join("design/sources.json")).unwrap();
    let doc = fid_canon::parse(raw.as_bytes()).unwrap();
    let CValue::Arr(tools) = get(&doc, "tools") else { panic!("tools array") };
    let penpot = tools
        .iter()
        .find(|t| get(t, "id") == &CValue::Str("penpot".into()))
        .expect("penpot must be listed in design/sources.json");
    assert_eq!(get(penpot, "open_source"), &CValue::Bool(true));
    assert_eq!(get(penpot, "requires_paid_account"), &CValue::Bool(false));

    // REQ-001 and REQ-005: contribution guide covers Penpot, source-of-truth, every artefact kind,
    // issue template, originality declaration, review record, text/SVG-only path and placeholder name.
    let guide = std::fs::read_to_string(repo().join("design/CONTRIBUTING.md")).unwrap();
    for needle in [
        "Penpot",
        "single source of truth",
        "design/tokens/",
        "design/sources.json",
        "design-sources@1",
        "design-proposal.yml",
        "originality@1",
        "design-review@1",
        "Token",
        "Icon",
        "Component",
        "Pattern",
        "Screen",
        "Report style",
        "text editor",
        "SVG",
        "{{FAMILY_NAME}}",
    ] {
        assert!(guide.to_lowercase().contains(&needle.to_lowercase()), "missing {needle:?} in design/CONTRIBUTING.md");
    }

    // Issue template exists.
    assert!(repo().join(".github/ISSUE_TEMPLATE/design-proposal.yml").is_file());
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-design-sources-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let t = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), t).unwrap();
        }
    }
}

/// `--spec` catches a syntactically valid UX node ID that does not exist in `spec/`.
#[test]
fn spec_dir_checks_exact_node_existence() {
    let d = temp("spec-missing-leaf");
    copy_dir(&repo().join(FX).join("ok"), &d);
    let p = d.join("sources.json");
    let text = std::fs::read_to_string(&p).unwrap().replace("UX.ICONS.CORESET", "UX.ICONS.NONEXISTENT");
    std::fs::write(&p, text).unwrap();

    // Without --spec, `UX.ICONS.NONEXISTENT` passes subsystem syntax check.
    let r_no_spec = check(&d, None).unwrap();
    assert!(r_no_spec.findings.is_empty(), "{:?}", r_no_spec.findings);

    // With --spec, it fails with SOURCE-NODE-UNKNOWN.
    let r_with_spec = check(&d, Some(&repo().join("spec"))).unwrap();
    assert_eq!(r_with_spec.findings.len(), 1);
    assert_eq!(r_with_spec.findings[0].code, sources::NODE_UNKNOWN);
    assert_eq!(r_with_spec.findings[0].subject, "icon-probe-v1");
}

/// Schema and security guard mutations on copies of `ok/`.
#[test]
fn schema_and_guard_rules() {
    let fresh = |tag: &str| {
        let d = temp(tag);
        copy_dir(&repo().join(FX).join("ok"), &d);
        d
    };
    let codes = |d: &Path| -> Vec<(String, String)> {
        check(d, None).unwrap().findings.into_iter().map(|f| (f.code.to_string(), f.subject)).collect()
    };

    // Invalid JSON.
    let d = fresh("bad-json");
    std::fs::write(d.join("sources.json"), "{not json").unwrap();
    assert_eq!(codes(&d), [(sources::SCHEMA.into(), "sources.json".into())]);

    // Duplicate JSON key refused.
    let d = fresh("dup-key");
    std::fs::write(
        d.join("sources.json"),
        r#"{"format":"design-sources@1","format":"design-sources@1","tools":[],"sources":[]}"#,
    )
    .unwrap();
    assert_eq!(codes(&d), [(sources::SCHEMA.into(), "sources.json".into())]);

    // Bidirectional control character refused.
    let d = fresh("bidi");
    let t = std::fs::read_to_string(d.join("sources.json")).unwrap().replace("Penpot", "Pen\u{202e}pot");
    std::fs::write(d.join("sources.json"), t).unwrap();
    assert_eq!(codes(&d), [(sources::SCHEMA.into(), "sources.json".into())]);

    // Unknown tool referenced by source.
    let d = fresh("unknown-tool");
    let t = std::fs::read_to_string(d.join("sources.json"))
        .unwrap()
        .replace("\"tool\": \"inkscape\"", "\"tool\": \"ghost\"");
    std::fs::write(d.join("sources.json"), t).unwrap();
    assert_eq!(codes(&d), [(sources::SCHEMA.into(), "icon-probe-v1".into())]);

    // Non-HTTPS URL rejected.
    let d = fresh("http-url");
    let t = std::fs::read_to_string(d.join("sources.json"))
        .unwrap()
        .replace("https://design.penpot.app/#/view/synthetic-report-summary", "http://example.com/x");
    std::fs::write(d.join("sources.json"), t).unwrap();
    assert_eq!(codes(&d), [(sources::SCHEMA.into(), "report-summary-v1".into())]);

    // Path traversal in export path rejected.
    let d = fresh("traversal");
    let t = std::fs::read_to_string(d.join("sources.json")).unwrap().replace("exports/icon-probe.svg", "../escape.svg");
    std::fs::write(d.join("sources.json"), t).unwrap();
    // The refused path was the source's only SVG, so it also has no open-format export left.
    let got = codes(&d);
    assert!(got.iter().all(|(c, s)| (c == sources::SCHEMA || c == sources::EXPORT_MISSING) && s == "icon-probe-v1"));
    assert!(got.iter().any(|(c, _)| c == sources::SCHEMA), "{got:?}");
    assert!(got.iter().any(|(c, _)| c == sources::EXPORT_MISSING), "{got:?}");

    // Duplicate source ID rejected.
    let d = fresh("dup-src");
    let t = std::fs::read_to_string(d.join("sources.json")).unwrap().replace("report-summary-v1", "icon-probe-v1");
    std::fs::write(d.join("sources.json"), t).unwrap();
    assert_eq!(codes(&d), [(sources::SCHEMA.into(), "icon-probe-v1".into())]);

    // Symbolic link export is refused (never followed).
    #[cfg(unix)]
    {
        let d = fresh("symlink-export");
        let target = d.join("exports/icon-probe.svg");
        let real = d.join("exports/real.svg");
        std::fs::rename(&target, &real).unwrap();
        std::os::unix::fs::symlink(&real, &target).unwrap();
        assert_eq!(codes(&d), [(sources::SCHEMA.into(), "icon-probe-v1".into())]);
    }
}

/// Bad CLI usage and unreadable inputs exit 2.
#[test]
fn bad_usage_and_unreadable_exit_2() {
    for args in [
        &["sources-check"][..],
        &["sources-check", "--spec", "spec"],
        &["sources-check", "--root", "design", "--all"],
        &["sources-check", "--root", "design", "--from", "x"],
        &["sources-check", "--root", "design", "--attribution", "ATTRIBUTION.md"],
        &["sources-check", "--root", "design", "--bogus"],
        &["pkg-check", "--root", "design", "--spec", "spec"],
    ] {
        assert_eq!(run(args).status.code(), Some(2), "{args:?}");
    }
    // Missing root or missing sources.json or bad --spec exit 2.
    assert_eq!(run(&["sources-check", "--root", "no/such/dir"]).status.code(), Some(2));
    assert_eq!(run(&["sources-check", "--root", "spec"]).status.code(), Some(2));
    assert_eq!(run(&["sources-check", "--root", "design", "--spec", "no/such/spec"]).status.code(), Some(2));
}
