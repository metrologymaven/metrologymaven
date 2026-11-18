//! Acceptance criteria of spec node ECO.RULES.VOCAB.ENGINE (AC-1 .. AC-7) and error paths.
//! Expected results are hand-labelled from the spec's rules in `fixtures/vocab/engine/`
//! (expected-report.json and the tables below), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_vocab::{lint, parse_units, test_rules, RuleSet, Severity, Suppression, Unit, VocabError};
use std::path::PathBuf;
use std::process::{Command, Output};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/vocab/engine").join(rel)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-vocab")).args(args).output().expect("run fid-vocab")
}

fn path(rel: &str) -> String {
    fix(rel).to_string_lossy().into_owned()
}

fn lint_json(rules: &str, units: &str) -> Output {
    run(&["lint", "--rules", &path(rules), "--units", &path(units), "--json"])
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn all_kinds() -> RuleSet {
    RuleSet::load(&fix("rules-all-kinds.toml")).unwrap()
}

fn units(rel: &str) -> Vec<Unit> {
    parse_units(&std::fs::read(fix(rel)).unwrap()).unwrap()
}

/// (unit, rule, matched) triples of a report, in report order.
fn triples(r: &fid_vocab::Report) -> Vec<(String, String, Vec<String>)> {
    r.findings.iter().map(|f| (f.unit.clone(), f.rule.clone(), f.matched.clone())).collect()
}

fn owned(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ac1_lint_all_kinds_matches_expected_report() {
    let out = lint_json("rules-all-kinds.toml", "units.jsonl");
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    let expected = fid_canon::canonicalize(&std::fs::read(fix("expected-report.json")).unwrap()).unwrap();
    assert_eq!(stdout(&out).trim_end(), String::from_utf8(expected).unwrap());
}

#[test]
fn ac2_test_rules_passes_and_names_rule_missing_fail_example() {
    let ok = run(&["test-rules", "--rules", &path("rules-all-kinds.toml")]);
    assert_eq!(ok.status.code(), Some(0), "stdout: {}", stdout(&ok));
    assert!(stdout(&ok).contains("engine-fixture-1"));

    let bad = run(&["test-rules", "--rules", &path("rules-missing-fail-example.toml")]);
    assert_eq!(bad.status.code(), Some(1));
    let text = stdout(&bad);
    assert!(text.contains("rule VC010: no fail example"), "{text}");
    assert!(!text.contains("rule VC001"), "{text}");

    let problems = test_rules(&RuleSet::load(&fix("rules-missing-fail-example.toml")).unwrap());
    assert_eq!(problems, vec!["rule VC010: no fail example".to_string()]);

    // lint refuses a rule file whose examples do not behave.
    let refused = lint_json("rules-missing-fail-example.toml", "units.jsonl");
    assert_eq!(refused.status.code(), Some(2));
    assert!(stderr(&refused).contains("VC010"));
    assert!(refused.stdout.is_empty());
}

#[test]
fn ac2_misbehaving_examples_are_reported() {
    let text = r#"
format = "fid-vocab-rules/1"
version = "t"
[[rule]]
id = "X1"
kind = "claim"
severity = "error"
message = "m"
words = ["certified"]
examples.pass = ["Certified parts"]
examples.fail = ["Recertified parts"]
"#;
    let rules = RuleSet::parse(text, None).unwrap();
    assert_eq!(
        test_rules(&rules),
        vec![
            "rule X1: pass example 1 yields a finding: \"Certified parts\"".to_string(),
            "rule X1: fail example 1 yields no finding: \"Recertified parts\"".to_string(),
        ]
    );
}

#[test]
fn ac3_whole_word_and_case_insensitive() {
    let rules = all_kinds();
    let r = lint(&rules, &[Unit::new("a", "Recertified parts")]);
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    let r = lint(&rules, &[Unit::new("b", "CERTIFIED parts")]);
    assert_eq!(triples(&r), vec![("b".into(), "VC001".into(), owned(&["CERTIFIED"]))]);
    assert_eq!(r.exit_code(), 1);
}

#[test]
fn ac4_suppression_is_not_counted_and_is_listed() {
    let rules = all_kinds();
    let mut u = Unit::new("u06", "This is \"Certified Measurement Report\" as titled by the customer.");
    u.suppress.push(Suppression { rule: "VC001".into(), reason: "quotes a customer document title".into() });
    let r = lint(&rules, &[u]);
    assert!(r.findings.is_empty());
    assert_eq!(r.exit_code(), 0);
    assert_eq!(r.suppressions.len(), 1);
    assert_eq!(r.suppressions[0].rule, "VC001");
    assert_eq!(r.suppressions[0].reason, "quotes a customer document title");
    assert_eq!(r.suppressions[0].suppressed, 1);

    // Same through the full fixture: u06 has no finding and its suppression is in the report.
    let full = lint(&rules, &units("units.jsonl"));
    assert!(full.findings.iter().all(|f| f.unit != "u06"));
    assert_eq!(full.suppressions.len(), 1);
    assert_eq!(full.suppressions[0].unit, "u06");

    // A suppression must carry a reason.
    let bad = br#"{"id":"x","text":"t","suppress":[{"rule":"VC001","reason":" "}]}"#;
    assert!(matches!(parse_units(bad), Err(VocabError::UnitParse { line: 1, .. })));
}

#[test]
fn ac5_unsupported_format_exits_2_naming_it() {
    let out = lint_json("rules-format-2.toml", "units.jsonl");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("fid-vocab-rules/2"), "{}", stderr(&out));
    assert!(out.stdout.is_empty());

    let t = run(&["test-rules", "--rules", &path("rules-format-2.toml")]);
    assert_eq!(t.status.code(), Some(2));

    let e = RuleSet::load(&fix("rules-format-2.toml")).unwrap_err();
    assert_eq!(e, VocabError::UnsupportedFormat("fid-vocab-rules/2".into()));
}

#[test]
fn ac6_outr_cl001_cl002_compatibility() {
    let rules = RuleSet::load(&fix("outr-cl001-cl002.toml")).unwrap();
    assert!(test_rules(&rules).is_empty(), "{:?}", test_rules(&rules));
    let r = lint(&rules, &units("outr-units.jsonl"));
    // OUTR.BRAND.LINT AC-1: one CL001 at line 1 naming `certified`; AC-2: none;
    // AC-3: one CL002 naming PolyWorks and Faster; AC-4: none.
    assert_eq!(
        triples(&r),
        vec![
            ("cl001-fail.md#1".into(), "CL001".into(), owned(&["certified"])),
            ("cl002-fail.md#1".into(), "CL002".into(), owned(&["Faster", "PolyWorks"])),
        ]
    );
    assert_eq!(r.findings[0].line, 1);
    assert_eq!(r.findings[0].file, "fixtures/outr/lint/cl001-fail.md");
    assert_eq!(r.exit_code(), 1);

    // Each OUTR fixture on its own gives the exit code OUTR's AC-1 .. AC-4 require.
    for (id, code) in [("cl001-fail.md#1", 1), ("cl001-pass.md#1", 0), ("cl002-fail.md#1", 1), ("cl002-pass.md#1", 0)] {
        let one: Vec<Unit> = units("outr-units.jsonl").into_iter().filter(|u| u.id == id).collect();
        assert_eq!(lint(&rules, &one).exit_code(), code, "{id}");
    }

    // CL001 needs both qualifiers: a designator with edition but no link still fails.
    let mut no_link = Unit::new("n", "Validated per ISO 10360-2:2009 for length.");
    assert_eq!(lint(&rules, &[no_link.clone()]).findings.len(), 1);
    no_link.links.push("https://metrologymaven.org/evidence/x".into());
    assert!(lint(&rules, &[no_link]).findings.is_empty());

    let cli = lint_json("outr-cl001-cl002.toml", "outr-units.jsonl");
    assert_eq!(cli.status.code(), Some(1));
    assert!(stdout(&cli).contains("\"rules_version\":\"outr-claim-rules-compat-1\""));
}

#[test]
fn ac7_unit_order_does_not_change_the_report() {
    let a = lint_json("rules-all-kinds.toml", "units.jsonl");
    let b = lint_json("rules-all-kinds.toml", "units-reordered.jsonl");
    assert_eq!(a.status.code(), Some(1));
    assert_eq!(a.stdout, b.stdout);

    let rules = all_kinds();
    let mut us = units("units.jsonl");
    let fwd = lint(&rules, &us).to_json().unwrap();
    us.reverse();
    assert_eq!(fwd, lint(&rules, &us).to_json().unwrap());
}

#[test]
fn nfc_normalisation_before_matching() {
    let text = "format = \"fid-vocab-rules/1\"\nversion = \"t\"\n[[rule]]\nid = \"N1\"\nkind = \"preferred\"\n\
                severity = \"warn\"\nmessage = \"m\"\nwords = [\"caf\u{e9}\"]\npreferred = \"coffee bar\"\n";
    let rules = RuleSet::parse(text, None).unwrap();
    // Decomposed e + combining acute in the unit; precomposed in the rule.
    let r = lint(&rules, &[Unit::new("x", "Meet at the CAFE\u{301} today.")]);
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].matched, owned(&["CAF\u{c9}"]));
    assert_eq!(r.findings[0].preferred.as_deref(), Some("coffee bar"));
    assert_eq!(r.findings[0].severity, Severity::Warn);
    assert_eq!(r.exit_code(), 0);
    // Not inside a longer word.
    assert!(lint(&rules, &[Unit::new("y", "cafeteria caf\u{e9}s")]).findings.is_empty());
}

#[test]
fn text_report_prints_rules_version() {
    let out = run(&["lint", "--rules", &path("rules-all-kinds.toml"), "--units", &path("units.jsonl")]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.starts_with("fid-vocab: rules version engine-fixture-1\n"), "{text}");
    assert!(text.contains("8 error(s), 1 warning(s), 1 suppression(s)"), "{text}");
}

#[test]
fn warnings_only_exit_0() {
    let r = lint(&all_kinds(), &[Unit::new("ui.menu.2", "Edit the dialect map.")]);
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.exit_code(), 0);
}

#[test]
fn error_paths() {
    // Unit stream: bad JSON on line 2, unknown key, duplicate ID, bad number.
    assert_eq!(parse_units(b"{\"id\":\"a\",\"text\":\"x\"}\n{oops").unwrap_err().code(), "unit_parse");
    assert!(matches!(parse_units(b"\n{\"id\":\"a\",\"txt\":\"x\"}"), Err(VocabError::UnitParse { line: 2, .. })));
    let dup = b"{\"id\":\"a\",\"text\":\"x\"}\n{\"id\":\"a\",\"text\":\"y\"}\n";
    assert!(matches!(parse_units(dup), Err(VocabError::UnitParse { line: 2, .. })));
    assert!(parse_units(b"{\"id\":\"a\",\"text\":\"x\",\"line\":1.5}").is_err());

    // Rule file: syntax error, missing version, unknown kind, duplicate ID, foreign field.
    let head = "format = \"fid-vocab-rules/1\"\nversion = \"t\"\n";
    let rule = |body: &str| format!("{head}[[rule]]\nid = \"R\"\nseverity = \"error\"\nmessage = \"m\"\n{body}");
    let parse = |t: &str| RuleSet::parse(t, None).map(|_| ()).map_err(|e| e.code());
    assert_eq!(parse("format = "), Err("rules_parse"));
    assert_eq!(parse("format = \"fid-vocab-rules/1\"\n[[rule]]\n"), Err("rules_parse"));
    assert_eq!(parse(&rule("kind = \"ban\"\nwords = [\"x\"]\n")), Err("rules_parse"));
    assert_eq!(parse(&rule("kind = \"claim\"\nwords = [\"x\"]\npreferred = \"y\"\n")), Err("rules_parse"));
    assert_eq!(parse(&rule("kind = \"preferred\"\nwords = [\"x\"]\n")), Err("rules_parse"));
    assert_eq!(parse(&rule("kind = \"cooccur\"\nwords = [\"x\"]\n")), Err("rules_parse"));
    assert_eq!(parse(&rule("kind = \"claim\"\nwords = [\"x\"]\nqualifiers = [\"designator\"]\n")), Ok(()));
    let twice = format!(
        "{}[[rule]]\nid = \"R\"\nkind = \"claim\"\nseverity = \"warn\"\nmessage = \"m\"\nwords = [\"y\"]\n",
        rule("kind = \"claim\"\nwords = [\"x\"]\n")
    );
    assert_eq!(parse(&twice), Err("rules_parse"));
    assert_eq!(parse(&rule("kind = \"name\"\nnames_file = \"n.txt\"\n")), Err("rules_parse"));

    // Size limit: a rule file over 1 MiB is refused.
    let big = format!("{head}# {}\n", "x".repeat(fid_vocab::MAX_RULES_BYTES));
    assert_eq!(parse(&big), Err("input_too_large"));

    // CLI: unreadable input and bad usage exit 2.
    let missing = run(&["lint", "--rules", &path("rules-all-kinds.toml"), "--units", &path("no-such.jsonl")]);
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(run(&["lint"]).status.code(), Some(2));
    assert_eq!(run(&[]).status.code(), Some(2));
}

#[test]
fn sentence_scope_and_nominative_contexts() {
    let rules = all_kinds();
    // Comparison word and name in different sentences: no VC003.
    let r = lint(&rules, &[Unit::new("import.x", "PolyWorks files open. The new solver is faster.")]);
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    // Same sentence without a benchmark link: VC003 (and VC002, the ID is not nominative).
    let r = lint(&rules, &[Unit::new("ui.x", "The new solver is faster than PC-DMIS here.")]);
    assert_eq!(
        triples(&r),
        vec![
            ("ui.x".into(), "VC002".into(), owned(&["PC-DMIS"])),
            ("ui.x".into(), "VC003".into(), owned(&["faster", "PC-DMIS"])),
        ]
    );
}

fn claim_rules(extra: &str) -> String {
    format!(
        "format = \"fid-vocab-rules/1\"\nversion = \"t\"\n[[rule]]\nid = \"C1\"\nkind = \"claim\"\nseverity = \"error\"\n\
         message = \"m\"\nwords = [\"certified\", \"approved\"]\nqualifiers = [\"designator\"]\nscope = \"sentence\"\n{extra}"
    )
}

#[test]
fn large_unit_stays_linear() {
    let mut text = String::new();
    for i in 0..100_000 {
        text.push_str(&format!("Part {i:06} is certified. "));
    }
    let mut rules_text = claim_rules("");
    rules_text.push_str(
        "[[rule]]\nid = \"N1\"\nkind = \"name\"\nseverity = \"warn\"\nmessage = \"m\"\nwords = [\"PolyWorks\"]\n\
         allow_phrases = [\"import from\"]\nscope = \"sentence\"\n",
    );
    let rules = RuleSet::parse(&rules_text, None).unwrap();
    let started = std::time::Instant::now();
    let r = lint(&rules, &[Unit::new("big", text)]);
    let elapsed = started.elapsed();
    assert_eq!(r.findings.len(), 100_000);
    // Each sentence is 26 characters; "certified" starts at character 15 of it.
    assert_eq!(r.findings[0].offset, 15);
    assert_eq!(r.findings[99_999].offset, 26 * 99_999 + 15);
    assert!(elapsed.as_secs() < 30, "took {elapsed:?}");
}

#[test]
fn names_file_must_stay_below_the_rule_directory() {
    let base = fix("");
    let name_rule = |file: &str| {
        format!(
            "format = \"fid-vocab-rules/1\"\nversion = \"t\"\n[[rule]]\nid = \"N\"\nkind = \"name\"\nseverity = \"error\"\n\
             message = \"m\"\nnames_file = {file:?}\n"
        )
    };
    assert!(RuleSet::parse(&name_rule("incumbents.txt"), Some(&base)).is_ok());
    assert!(RuleSet::parse(&name_rule("./incumbents.txt"), Some(&base)).is_ok());
    let abs = fix("incumbents.txt").to_string_lossy().into_owned();
    for bad in ["../engine/incumbents.txt", "sub/../incumbents.txt", "/etc/passwd", abs.as_str()] {
        let e = RuleSet::parse(&name_rule(bad), Some(&base)).unwrap_err();
        assert_eq!(e.code(), "rules_parse", "{bad}");
    }
}

#[test]
fn designator_pattern_limits_and_nfc() {
    let deep = format!("designator_pattern = '{}ISO{}'\n", "(".repeat(40), ")".repeat(40));
    assert_eq!(RuleSet::parse(&claim_rules(&deep), None).unwrap_err().code(), "rules_parse");
    let huge = "designator_pattern = '\\w{2000}\\w{2000}'\n";
    assert_eq!(RuleSet::parse(&claim_rules(huge), None).unwrap_err().code(), "rules_parse");

    // A decomposed pattern still matches precomposed text: both are NFC-normalised.
    let decomposed = "designator_pattern = 'NORME\u{301} \\d+'\n";
    let rules = RuleSet::parse(&claim_rules(decomposed), None).unwrap();
    assert!(lint(&rules, &[Unit::new("a", "Approved per NORM\u{c9} 12.")]).findings.is_empty());
    assert_eq!(lint(&rules, &[Unit::new("b", "Approved per NORM 12.")]).findings.len(), 1);
}

#[test]
fn rule_count_is_capped() {
    let mut text = "format = \"fid-vocab-rules/1\"\nversion = \"t\"\n".to_string();
    for i in 0..=fid_vocab::MAX_RULES {
        text.push_str(&format!(
            "[[rule]]\nid = \"R{i}\"\nkind = \"preferred\"\nseverity = \"warn\"\nmessage = \"m\"\nwords = [\"x\"]\n\
             preferred = \"y\"\n"
        ));
    }
    let e = RuleSet::parse(&text, None).unwrap_err();
    assert!(e.to_string().contains("1000"), "{e}");
}
