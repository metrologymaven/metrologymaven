//! Acceptance criteria of spec node OUTR.BRAND.GUIDE.NAME (AC-1 .. AC-5) and edge cases.
//! Expected results are hand-labelled from the spec's rules (the EXPECTED table in
//! `fixtures/outr/brand/guide/make_fixtures.py`), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::guide_check::{code_spans, parse_name_state, scan_literal_names, scan_tokens, Code};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn fix(case: &str) -> PathBuf {
    repo("fixtures/outr/brand/guide").join(case)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn bin(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(args).output().unwrap()
}

fn check(root: &Path, name: &Path, extra: &[String]) -> Output {
    let mut a = vec!["guide-check".to_string(), "--root".into(), s(root), "--name".into(), s(name)];
    a.extend_from_slice(extra);
    bin(&a)
}

fn case_check(case: &str, extra: &[String]) -> Output {
    let d = fix(case);
    check(&d.join("guidelines"), &d.join("name.toml"), extra)
}

fn out_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-guide-check-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn publish_args(out: &Path) -> Vec<String> {
    vec!["--publish".into(), "--out".into(), s(out)]
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("guide-check:")).map(str::to_string).collect()
}

fn is_empty_or_absent(d: &Path) -> bool {
    match std::fs::read_dir(d) {
        Ok(mut it) => it.next().is_none(),
        Err(_) => true,
    }
}

#[test]
fn ac1_repository_guidelines_pass_json() {
    let o = check(&repo("brand/guidelines"), &repo("brand/name.toml"), &["--json".into()]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("\"ok\":true"), "{out}");
    assert!(out.contains("name-and-logo.md"), "{out}");
}

#[test]
fn ac1_unaccepted_check_exits_0() {
    let o = case_check("clean", &["--json".into()]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("\"files\":[\"sub/apps.md\",\"usage.md\"]"), "{out}");
}

#[test]
fn ac2_literal_name_at_file_and_line() {
    let o = case_check("literal", &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), vec!["literal_name usage.md:3 Fiducial".to_string()]);
}

#[test]
fn ac3_publish_refused_before_acceptance_writes_nothing() {
    let out = out_dir("ac3");
    let d = fix("unaccepted");
    let o = check(&d.join("guidelines"), &d.join("name.toml"), &publish_args(&out));
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let l = lines(&o);
    assert_eq!(l.len(), 1, "{l:?}");
    assert!(l[0].starts_with("name_not_accepted "), "{l:?}");
    assert!(is_empty_or_absent(&out));
}

#[test]
fn ac4_publish_after_acceptance_substitutes() {
    let out = out_dir("ac4");
    let o = case_check("accepted", &publish_args(&out));
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let usage = std::fs::read_to_string(out.join("usage.md")).unwrap();
    assert!(usage.contains("Refer to Examplo Inspect by its full name."), "{usage}");
    assert!(!usage.contains("{{"), "{usage}");
    let apps = std::fs::read_to_string(out.join("sub").join("apps.md")).unwrap();
    assert!(apps.contains("- Examplo Probe"), "{apps}");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn ac5_stray_token_refused() {
    let out = out_dir("ac5");
    let o = case_check("stray-token", &publish_args(&out));
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), vec!["token_left usage.md:3 {{TAGLINE}}".to_string()]);
    assert!(is_empty_or_absent(&out));
}

#[test]
fn repository_guidelines_publish_with_recorded_name() {
    let out = out_dir("repo");
    let o = check(&repo("brand/guidelines"), &repo("brand/name.toml"), &publish_args(&out));
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let text = std::fs::read_to_string(out.join("name-and-logo.md")).unwrap();
    for app in ["Inspect", "Probe", "Network", "Report"] {
        assert!(text.contains(&format!("Metrology Maven {app}")), "{app}");
    }
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn unreadable_and_bad_usage_exit_2() {
    let d = fix("clean");
    let o = check(&d.join("guidelines"), &d.join("missing.toml"), &[]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    let o = check(&d.join("guidelines"), &d.join("name.toml"), &["--publish".into()]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
}

#[test]
fn name_file_parse_errors() {
    assert!(parse_name_state("accepted = false\n", "n").is_ok());
    for bad in [
        "working_names = []\n",
        "accepted = \"yes\"\n",
        "accepted = false\nnickname = \"x\"\n",
        "accepted = false\nworking_names = [\"\"]\n",
        "accepted = true\nname = \"{{X}}\"\ndecision = \"D-002\"\n",
        "accepted = false\naccepted = true\n",
    ] {
        let e = parse_name_state(bad, "n").unwrap_err();
        assert_eq!(e.code(), "name_file_parse", "{bad}");
    }
    let st = parse_name_state("accepted = true\nname = \"Examplo\"\n", "n").unwrap();
    assert_eq!(st.accepted_name(), None, "decision is required");
}

#[test]
fn literal_scan_rules() {
    let names = ["Fiducial", "fid", "Examplo Two"];
    let text = "a `fid` b\nfid-outreach\nFiducials\n``x `Fiducial` y``\nunclosed `Fiducial\nsay Examplo Two.\n~~~\nfid\n~~~\nfid\n";
    let got: Vec<(usize, String)> =
        scan_literal_names("f.md", text, &names).into_iter().map(|f| (f.line, f.detail)).collect();
    let want: Vec<(usize, String)> =
        vec![(2, "fid".into()), (5, "Fiducial".into()), (6, "Examplo Two".into()), (10, "fid".into())];
    assert_eq!(got, want);
    assert!(scan_literal_names("f.md", "x", &names).iter().all(|f| f.code == Code::LiteralName));
}

#[test]
fn code_spans_and_tokens() {
    assert_eq!(code_spans("a `b` c"), vec![(2, 5)]);
    assert_eq!(code_spans("``a`b`` c"), vec![(0, 7)]);
    assert!(code_spans("a ` b").is_empty());
    let t: Vec<String> = scan_tokens("f", "{{A}} {B} {{ x }}\n{{\n}}").into_iter().map(|f| f.detail).collect();
    assert_eq!(t, vec!["{{A}}".to_string(), "{{ x }}".to_string()]);
}
