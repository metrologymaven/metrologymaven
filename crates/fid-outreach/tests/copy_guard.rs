//! Acceptance criteria of spec node OUTR.BRAND.COPY (AC-1 .. AC-6) and error paths.
//! Expected results are hand-labelled from the spec's rules (the EXPECTED table in
//! `fixtures/outr/brand/copy/make_fixtures.py`), not produced by this crate. Every fixture is
//! project-authored text (REQ-OUTR.BRAND.COPY-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::copy_guard::{
    check_rules, declares_incumbent_text, is_refset_name, parse_policy, parse_record, scan_reference_sets, Code,
    Policy, Record, Revisit,
};
use std::path::Path;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::{Command, Output};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/brand/copy").join(rel)
}

fn s(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

fn run_with(name: &str, policy: &str, json: bool) -> Output {
    let d = fix(name);
    let mut args = vec![
        "copy-guard".to_string(),
        "--policy".into(),
        s(d.join(policy)),
        "--checklist".into(),
        s(d.join("checklist.toml")),
        "--records".into(),
        s(d.join("records")),
        "--root".into(),
        s(d.clone()),
    ];
    if json {
        args.push("--json".into());
    }
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(&args).output().unwrap()
}

fn run(name: &str) -> Output {
    run_with(name, "copy-policy.toml", false)
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Finding lines of a text-mode run (the summary line is dropped).
fn lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("copy-guard:")).map(str::to_string).collect()
}

#[test]
fn ac1_ok_exits_0_with_canonical_json() {
    let o = run_with("ok", "copy-policy.toml", true);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    assert_eq!(String::from_utf8(o.stdout).unwrap(), "{\"findings\":[],\"ok\":true}\n");
    let o = run("ok");
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    assert!(lines(&o).is_empty());
}

#[test]
fn ac2_checklist_without_copied_text_item() {
    let o = run("no-item");
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["copied_text_item_missing"]);
}

#[test]
fn ac3_found_copy_needs_revisit_entry() {
    let o = run("found-copy");
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["d013_revisit_due site-2027.03"]);

    let o = run_with("found-copy", "copy-policy.toml", true);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        "{\"findings\":[{\"code\":\"d013_revisit_due\",\"subject\":\"site-2027.03\"}],\"ok\":false}\n"
    );

    let o = run_with("found-copy", "copy-policy-revisited.toml", false);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
}

#[test]
fn ac4_mode_changed_without_counsel_review() {
    let o = run("mode-changed");
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["counsel_review_missing"]);
    // With a counsel review the mode change is allowed (REQ-001).
    let o = run_with("mode-changed", "copy-policy-reviewed.toml", false);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
}

#[test]
fn ac5_reference_set_file_present() {
    let o = run("refset");
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["reference_set_present brand/incumbents.refset.json"]);
}

#[test]
fn reference_set_declared_by_top_level_kind_key() {
    let o = run("kind-key");
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    // brand/nested.json has the kind below the top level only: not a declaration.
    assert_eq!(lines(&o), ["reference_set_present brand/samples.toml"]);
}

#[test]
fn ac6_missing_policy_exits_2_naming_path() {
    let o = run_with("ok", "no-such-policy.toml", true);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no-such-policy.toml"));
    let v = fid_canon::parse(&o.stdout).unwrap();
    let CValue::Obj(top) = v else { panic!("not an object") };
    let CValue::Obj(e) = &top["error"] else { panic!("no error object") };
    assert!(matches!(&e["path"], CValue::Str(p) if p.contains("no-such-policy.toml")));
}

#[test]
fn missing_records_dir_and_bad_usage_exit_2() {
    let d = fix("ok");
    let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
        .args(["copy-guard", "--policy"])
        .arg(d.join("copy-policy.toml"))
        .arg("--checklist")
        .arg(d.join("checklist.toml"))
        .arg("--records")
        .arg(d.join("no-records-here"))
        .arg("--root")
        .arg(&d)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no-records-here"));
    let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(["copy-guard", "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}

fn table(text: &str) -> toml::Table {
    text.parse().unwrap()
}

#[test]
fn policy_requires_mode_and_decision() {
    assert!(parse_policy(&table("decision = \"D-013\""), "p").is_err());
    assert!(parse_policy(&table("mode = \"review_only\""), "p").is_err());
    assert!(parse_policy(&table("mode = \"review_only\"\ndecision = \"D-999\""), "p").is_err());
    let p = parse_policy(&table("mode = \"review_only\"\ndecision = \"D-013\""), "p").unwrap();
    assert_eq!(p.mode, "review_only");
    // A revisit entry without a note does not count.
    let t = "mode = \"review_only\"\ndecision = \"D-013\"\n[[revisit]]\nrelease = \"r1\"\n";
    assert_eq!(parse_policy(&table(t), "p").unwrap_err().code(), "policy_parse");
}

#[test]
fn incomplete_counsel_review_does_not_count() {
    let t = "mode = \"hashed_shingles\"\ndecision = \"D-013\"\ncounsel_review = { date = \"2027-05-02\" }\n";
    let p = parse_policy(&table(t), "p").unwrap();
    assert!(p.counsel_review.is_none());
    let r = check_rules(&p, &BTreeSet::from(["copied_text".to_string()]), &[], &[]);
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, Code::CounselReviewMissing);
}

#[test]
fn reference_sets_are_ignored_outside_review_only_and_findings_sorted() {
    let items = BTreeSet::from(["colours".to_string()]);
    let refsets = vec!["b.refset".to_string(), "a.refset".to_string()];
    let records = vec![
        Record { release: "site-2027.09".into(), copied_text: Some("fail".into()) },
        Record { release: "site-2027.04".into(), copied_text: Some("fail".into()) },
        Record { release: "site-2027.05".into(), copied_text: Some("pass".into()) },
        Record { release: "site-2027.06".into(), copied_text: Some("fail".into()) },
    ];
    let revisits = vec![Revisit { release: "site-2027.06".into(), date: None, note: "n".into() }];
    let review_only = Policy { mode: "review_only".into(), counsel_review: None, revisits: revisits.clone() };
    let got: Vec<String> =
        check_rules(&review_only, &items, &records, &refsets).findings.iter().map(|f| f.to_string()).collect();
    assert_eq!(
        got,
        [
            "copied_text_item_missing",
            "d013_revisit_due site-2027.04",
            "d013_revisit_due site-2027.09",
            "reference_set_present a.refset",
            "reference_set_present b.refset",
        ]
    );
    let other = Policy { mode: "hashed_shingles".into(), counsel_review: None, revisits };
    let got: Vec<Code> = check_rules(&other, &items, &[], &refsets).findings.iter().map(|f| f.code).collect();
    assert_eq!(got, [Code::CounselReviewMissing, Code::CopiedTextItemMissing]);
}

#[test]
fn refset_name_parts() {
    assert!(is_refset_name("incumbents.refset.json"));
    assert!(is_refset_name("x.refset"));
    assert!(is_refset_name("x.REFSET.toml"));
    assert!(!is_refset_name("refset.json"));
    assert!(!is_refset_name("myrefset.json"));
    assert!(!is_refset_name("notes.refsets"));
}

#[test]
fn scan_is_sorted_and_fails_closed_on_unparsable_declaration() {
    let base = std::env::temp_dir().join(format!("fid-copy-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("z")).unwrap();
    std::fs::create_dir_all(base.join("a/b")).unwrap();
    std::fs::create_dir_all(base.join(".git")).unwrap();
    std::fs::write(base.join("z/one.refset"), "").unwrap();
    std::fs::write(base.join("a/b/two.refset.toml"), "").unwrap();
    std::fs::write(base.join(".git/skip.refset"), "").unwrap();
    std::fs::write(base.join("a/k.json"), "{\"kind\": \"incumbent_text\"}").unwrap();
    assert_eq!(scan_reference_sets(&base).unwrap(), ["a/b/two.refset.toml", "a/k.json", "z/one.refset"]);
    std::fs::write(base.join("a/broken.toml"), "kind = \"incumbent_text\n").unwrap();
    let e = scan_reference_sets(&base).unwrap_err();
    assert_eq!(e.code(), "input_parse");
    let _ = std::fs::remove_dir_all(&base);
}

/// A fresh empty directory under the system temp dir, unique per test.
fn scratch(test: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fid-copy-guard-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn build_dirs_are_skipped_only_at_the_scan_root() {
    let base = scratch("rootskip");
    for d in ["target", "node_modules", "brand/target", "brand/node_modules", "brand/.git"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    std::fs::write(base.join("target/a.refset.json"), "{}").unwrap();
    std::fs::write(base.join("node_modules/b.refset"), "").unwrap();
    std::fs::write(base.join("brand/target/x.refset.json"), "{}").unwrap();
    std::fs::write(base.join("brand/node_modules/y.refset"), "").unwrap();
    std::fs::write(base.join("brand/.git/z.refset"), "").unwrap();
    assert_eq!(scan_reference_sets(&base).unwrap(), ["brand/node_modules/y.refset", "brand/target/x.refset.json"]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn escaped_kind_declarations_are_parsed() {
    let p = Path::new("x");
    // `_` is `_`: the value spells incumbent_text without containing it literally.
    assert!(declares_incumbent_text(br#"{"kind":"incumbent_text"}"#, "json", p).unwrap());
    // Key and value both escaped: neither `kind` nor `incumbent_text` appears literally.
    assert!(declares_incumbent_text(br#"{"kind":"incumbent_text"}"#, "json", p).unwrap());
    assert!(declares_incumbent_text(br#"kind = "incumbent_text""#, "toml", p).unwrap());
    assert!(declares_incumbent_text(br#""kind" = "\U00000069ncumbent_text""#, "toml", p).unwrap());
    // Escapes spelling something else are not a declaration.
    assert!(!declares_incumbent_text(br#"{"kind":"café"}"#, "json", p).unwrap());
    // Unparsable with `kind` and an escape fails closed; with an escape only it is ignored.
    assert_eq!(declares_incumbent_text(br#"{"kind":"i"#, "json", p).unwrap_err().code(), "input_parse");
    assert!(!declares_incumbent_text(br#"{"a":"\u00"#, "json", p).unwrap());
    // Through the scan as well.
    let base = scratch("escaped");
    std::fs::write(base.join("e.json"), br#"{"kind":"incumbent_text"}"#).unwrap();
    assert_eq!(scan_reference_sets(&base).unwrap(), ["e.json"]);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn symlinked_toml_or_json_is_not_skipped_silently() {
    let base = scratch("symlink");
    std::fs::create_dir_all(base.join("outside")).unwrap();
    std::fs::create_dir_all(base.join("tree")).unwrap();
    std::fs::write(base.join("outside/decl.json"), r#"{"kind":"incumbent_text"}"#).unwrap();
    std::os::unix::fs::symlink(base.join("outside/decl.json"), base.join("tree/link.json")).unwrap();
    let e = scan_reference_sets(&base.join("tree")).unwrap_err();
    assert_eq!(e.code(), "input_unreadable");
    assert!(e.to_string().contains("link.json"));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn any_copied_text_result_other_than_pass_is_a_fail() {
    let items = BTreeSet::from(["copied_text".to_string()]);
    let policy = Policy { mode: "review_only".into(), counsel_review: None, revisits: vec![] };
    let rec = |release: &str, body: &str| {
        let text = format!("release = \"{release}\"\n[answers]\n{body}\n");
        parse_record(&table(&text), "r").unwrap()
    };
    let records = vec![
        rec("r-pass", "copied_text = { result = \"pass\" }"),
        rec("r-upper", "copied_text = { result = \"FAIL\" }"),
        rec("r-cap", "copied_text = { result = \"Pass\" }"),
        rec("r-skip", "copied_text = { result = \"skipped\" }"),
        rec("r-empty", "copied_text = { result = \"\" }"),
        rec("r-bool", "copied_text = { result = true }"),
        rec("r-noresult", "copied_text = { note = \"n\" }"),
        rec("r-absent", "colours = { result = \"pass\" }"),
    ];
    let got: Vec<String> = check_rules(&policy, &items, &records, &[]).findings.iter().map(|f| f.to_string()).collect();
    assert_eq!(
        got,
        [
            "d013_revisit_due r-bool",
            "d013_revisit_due r-cap",
            "d013_revisit_due r-empty",
            "d013_revisit_due r-noresult",
            "d013_revisit_due r-skip",
            "d013_revisit_due r-upper",
        ]
    );
}
