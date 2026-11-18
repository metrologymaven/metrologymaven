//! Spec node FLOW.RUNNER.PKG: REQ-001..REQ-006 and AC-1..AC-5, plus the hostile-input rules.
//! Expected findings come from `fixtures/flow/pkg/expected.txt` and `paths.txt`, labelled by
//! hand from the spec's rules; digests come from `fixtures/flow/pkg/make_fixtures.py`, an
//! independent standard-library Python reference. Hostile inputs that would be too large or
//! not portable to commit (oversized files, deep nesting, symbolic links) are built here,
//! with outcomes labelled from the same rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_accept::package::{
    check_manifest, check_path, package_digest, param_specs, parse_suite, validate_dir, validate_dir_with,
    validate_suite, Catalogue, Code, Gating, Limits, Metric, Report, CHECK_KINDS, FORMAT, MANIFEST, MAX_MANIFEST_BYTES,
    OPTIONAL_FIELDS, REQUIRED_FIELDS,
};
use fid_canon::CValue;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    repo().join("fixtures/flow/pkg")
}

fn catalogue() -> Catalogue {
    Catalogue::read(&fixtures().join("catalogue.toml")).unwrap()
}

fn short(findings: &[fid_accept::package::Finding]) -> Vec<String> {
    findings.iter().map(|f| format!("{} {}", f.code, f.subject)).collect()
}

fn expected(name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(fixtures().join("expected.txt")).unwrap();
    let rows: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(f, _)| *f == name)
        .map(|(_, v)| v)
        .collect();
    assert!(!rows.is_empty(), "{name} is not in expected.txt");
    rows.into_iter().filter(|v| *v != "ok").map(String::from).collect()
}

fn fixture_names() -> Vec<String> {
    let text = std::fs::read_to_string(fixtures().join("expected.txt")).unwrap();
    let mut names: Vec<String> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(f, _)| f.to_string())
        .filter(|f| !f.ends_with(".json"))
        .collect();
    names.dedup();
    names
}

fn validate_fixture(name: &str) -> Report {
    let r = validate_dir(&fixtures().join(name), &catalogue());
    assert_eq!(short(&r.findings), expected(name), "{name}: {:#?}", r.findings);
    // D0: the same input gives the same report.
    assert_eq!(validate_dir(&fixtures().join(name), &catalogue()), r, "{name}");
    assert_eq!(r.package.is_some(), r.is_ok(), "{name}");
    r
}

fn digest_file(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).unwrap().trim().to_string()
}

/// A fresh, empty scratch folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fid-accept-pkg-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Copies the files of a fixture package into a scratch folder.
fn copy_fixture(name: &str, to: &str) -> PathBuf {
    let dir = scratch(to);
    for entry in std::fs::read_dir(fixtures().join(name)).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
    }
    dir
}

// ------------------------------------------------------------------ acceptance criteria

#[test]
fn ac1_valid_package_is_accepted_with_reference_digest() {
    let r = validate_fixture("valid");
    assert_eq!(r.digest.unwrap().as_str(), digest_file("valid.digest"));
    assert_eq!(r.journal_entries, Some(3));
    let p = r.package.unwrap();
    assert_eq!(p.job_id, "JOB.ACC.01");
    assert_eq!(p.variant_id, "demo-unassisted");
    assert_eq!(p.gating, Gating::Gating);
    assert_eq!(p.datasets.len(), 1);
    assert_eq!(p.checks.len(), 2);
    assert_eq!(p.checks[1].kind, "value_within_u");
    assert_eq!(p.time_targets[0].metric, Metric::TaskS);
    assert_eq!(p.time_targets[0].seconds, 1200.0);
    assert_eq!(p.kill_after, None);
    // The digest is the same as the one the pure manifest check and package_digest give.
    let bytes = std::fs::read(fixtures().join("valid").join(MANIFEST)).unwrap();
    assert_eq!(package_digest(&bytes).unwrap().as_str(), digest_file("valid.digest"));
    let pure = check_manifest(&bytes, &catalogue());
    assert!(pure.is_ok(), "{:#?}", pure.findings);
    assert_eq!(pure.digest, r.digest);
}

#[test]
fn ac2_missing_gating() {
    let r = validate_fixture("no-gating");
    assert_eq!(r.findings[0].code, Code::Schema);
    assert_eq!(r.findings[0].to_string(), "PKG-SCHEMA gating: missing");
}

#[test]
fn ac3_unknown_check_kind() {
    let r = validate_fixture("bad-check");
    assert_eq!(r.findings[0].code, Code::Check);
}

#[test]
fn ac4_kill_after_without_survive_check() {
    let r = validate_fixture("kill-no-check");
    assert_eq!(r.findings[0].code, Code::Kill);
}

#[test]
fn ac5_changing_gating_changes_the_digest() {
    let dir = copy_fixture("valid", "ac5");
    let path = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("\"gating\": \"gating\"").count(), 1);
    std::fs::write(&path, text.replace("\"gating\": \"gating\"", "\"gating\": \"tracked\"")).unwrap();
    let r = validate_dir(&dir, &catalogue());
    assert!(r.is_ok(), "{:#?}", r.findings);
    let d = r.digest.unwrap();
    assert_ne!(d.as_str(), digest_file("valid.digest"));
    assert_eq!(d.as_str(), digest_file("valid-tracked.digest"));
    assert_eq!(r.package.unwrap().gating, Gating::Tracked);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn digest_covers_milestones_and_time_targets() {
    let base = std::fs::read_to_string(fixtures().join("valid").join(MANIFEST)).unwrap();
    let d0 = package_digest(base.as_bytes()).unwrap();
    for (from, to) in
        [("\"launch-1\"", "\"launch-2\""), ("1200", "1300"), ("\"verified\": false", "\"verified\": true")]
    {
        let changed = base.replacen(from, to, 1);
        assert_ne!(changed, base);
        assert_ne!(package_digest(changed.as_bytes()).unwrap(), d0, "{from}");
    }
    // Whitespace and key order do not change the digest (RFC 8785).
    let compact: String = base.lines().map(str::trim).collect();
    assert_eq!(package_digest(compact.as_bytes()).unwrap(), d0);
}

// ------------------------------------------------------------------ every fixture

#[test]
fn every_fixture_matches_its_hand_labels() {
    let names = fixture_names();
    assert!(names.len() >= 20, "{names:?}");
    for name in names {
        validate_fixture(&name);
    }
}

#[test]
fn every_fixture_folder_is_labelled() {
    let labelled = fixture_names();
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            let name = entry.file_name().into_string().unwrap();
            assert!(labelled.contains(&name), "{name} has no row in expected.txt");
        }
    }
}

#[test]
fn file_reference_rules_match_hand_labels() {
    let text = std::fs::read_to_string(fixtures().join("paths.txt")).unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let (verdict, path) = line.split_once('\t').unwrap();
        let ok = match verdict {
            "ok" => true,
            "bad" => false,
            other => panic!("bad verdict {other:?}"),
        };
        assert_eq!(check_path(path).is_ok(), ok, "{path:?}: {:?}", check_path(path));
        n += 1;
    }
    assert!(n >= 30);
    // Length bounds.
    let long = "a".repeat(100);
    assert!(check_path(&long).is_ok());
    assert!(check_path(&"a".repeat(101)).is_err());
    let at_limit = format!("{long}/{long}/{}", "b".repeat(53));
    assert_eq!(at_limit.len(), 255);
    assert!(check_path(&at_limit).is_ok());
    assert!(check_path(&format!("{at_limit}c")).is_err());
}

// ------------------------------------------------------------------ check-kind closed set

#[test]
fn every_kind_has_a_parameter_schema_and_nothing_else_does() {
    assert_eq!(CHECK_KINDS.len(), 22);
    for kind in CHECK_KINDS {
        assert!(param_specs(kind).is_some(), "{kind}");
    }
    for kind in ["value_close", "", "Replay_outcome", "replay_outcome ", "committed_survive_kill2"] {
        assert!(param_specs(kind).is_none(), "{kind}");
    }
}

fn manifest_with_check(kind: &str, params: &str) -> String {
    let base = std::fs::read_to_string(fixtures().join("valid").join(MANIFEST)).unwrap();
    let old = "\"kind\": \"value_within_u\",\n      \"params\": {\n        \"truth\": \"truth.json\"\n      }";
    assert!(base.contains(old));
    base.replace(old, &format!("\"kind\": \"{kind}\",\n      \"params\": {params}"))
}

fn manifest_findings(text: &str) -> Vec<String> {
    short(&check_manifest(text.as_bytes(), &catalogue()).findings)
}

#[test]
fn parameters_are_checked_per_kind() {
    let ok: &[(&str, &str)] = &[
        ("all_characteristics_resolved", "{}"),
        ("value_within_u", r#"{"truth": "truth.json", "items": ["c1", "c2"]}"#),
        ("decision_equals", r#"{"truth": "t/d.json"}"#),
        ("result_count", r#"{"count": 80}"#),
        ("ors_case", r#"{"cases": ["ors-01"]}"#),
        ("lexical_identical", r#"{"artefact": "report.pdf", "expected": "expected/report.pdf"}"#),
        ("lint_findings_equal", r#"{"expected": "lint.json"}"#),
        ("sheet_cells_equal", r#"{"artefact": "results.xlsx"}"#),
        ("verify_verdict", r#"{"expected": "valid"}"#),
        ("records_equal", r#"{"against": "j2-unassisted"}"#),
        ("file_digest", r#"{"artefact": "report.pdf"}"#),
        ("file_digest", &format!(r#"{{"artefact": "report.pdf", "sha256": "sha256:{}"}}"#, "0".repeat(64))),
        ("no_edit_reachable", r#"{"allowed_commands": []}"#),
        ("no_edit_reachable", r#"{"allowed_commands": ["kiosk.enter_serial@1"], "steps": [{"from": 1, "to": 3}]}"#),
        ("no_pointer_events", r#"{"steps": [{"from": 1, "to": 2}, {"from": 3, "to": 3}]}"#),
    ];
    for (kind, params) in ok {
        let f = manifest_findings(&manifest_with_check(kind, params));
        assert!(f.is_empty(), "{kind} {params}: {f:?}");
    }
    let bad: &[(&str, &str, &str)] = &[
        ("replay_outcome", r#"{"accept": ["identical"]}"#, "PKG-CHECK checks[1].params.accept"),
        ("value_within_u", r#"{"truth": "truth.json", "items": []}"#, "PKG-CHECK checks[1].params.items"),
        ("value_within_u", r#"{"truth": "truth.json", "items": ["c1", "c1"]}"#, "PKG-CHECK checks[1].params.items"),
        ("decision_equals", r#"{"truth": "..\\t.json"}"#, "PKG-CHECK checks[1].params.truth"),
        ("decision_equals", r#"{"truth": 7}"#, "PKG-CHECK checks[1].params.truth"),
        ("result_count", r#"{"count": -1}"#, "PKG-CHECK checks[1].params.count"),
        ("result_count", r#"{"count": 1.5}"#, "PKG-CHECK checks[1].params.count"),
        ("result_count", r#"{"count": "80"}"#, "PKG-CHECK checks[1].params.count"),
        ("ors_case", r#"{"cases": "ors-01"}"#, "PKG-CHECK checks[1].params.cases"),
        ("lexical_identical", r#"{"artefact": "", "expected": "e.pdf"}"#, "PKG-CHECK checks[1].params.artefact"),
        ("verify_verdict", r#"{"expected": "a\nb"}"#, "PKG-CHECK checks[1].params.expected"),
        ("records_equal", r#"{"against": "J2"}"#, "PKG-CHECK checks[1].params.against"),
        ("file_digest", r#"{"artefact": "r.pdf", "sha256": "sha256:ABC"}"#, "PKG-CHECK checks[1].params.sha256"),
        (
            "no_edit_reachable",
            r#"{"allowed_commands": ["not a command"]}"#,
            "PKG-CHECK checks[1].params.allowed_commands",
        ),
        (
            "no_edit_reachable",
            r#"{"allowed_commands": ["a.b@1", "a.b@1"]}"#,
            "PKG-CHECK checks[1].params.allowed_commands",
        ),
        ("no_pointer_events", r#"{"steps": []}"#, "PKG-CHECK checks[1].params.steps"),
        ("no_pointer_events", r#"{"steps": [{"from": 0, "to": 2}]}"#, "PKG-CHECK checks[1].params.steps"),
        ("no_pointer_events", r#"{"steps": [{"from": 3, "to": 2}]}"#, "PKG-CHECK checks[1].params.steps"),
        ("no_pointer_events", r#"{"steps": [{"from": 1, "to": 2, "x": 1}]}"#, "PKG-CHECK checks[1].params.steps"),
        ("no_pointer_events", "[]", "PKG-CHECK checks[1].params"),
    ];
    for (kind, params, want) in bad {
        let f = manifest_findings(&manifest_with_check(kind, params));
        assert_eq!(f, vec![want.to_string()], "{kind} {params}");
    }
}

#[test]
fn step_ranges_must_lie_inside_the_journal() {
    let dir = copy_fixture("valid", "steps");
    let text = manifest_with_check("no_pointer_events", r#"{"steps": [{"from": 2, "to": 4}]}"#);
    std::fs::write(dir.join(MANIFEST), &text).unwrap();
    let r = validate_dir(&dir, &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-CHECK checks[1].params.steps"]);
    let text = manifest_with_check("no_pointer_events", r#"{"steps": [{"from": 2, "to": 3}]}"#);
    std::fs::write(dir.join(MANIFEST), &text).unwrap();
    assert!(validate_dir(&dir, &catalogue()).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kill_after_inside_the_journal_is_accepted() {
    let base = std::fs::read_to_string(fixtures().join("kill-out-of-range").join(MANIFEST)).unwrap();
    let dir = copy_fixture("kill-out-of-range", "kill-ok");
    std::fs::write(dir.join(MANIFEST), base.replace("\"kill_after\": 4", "\"kill_after\": 3")).unwrap();
    let r = validate_dir(&dir, &catalogue());
    assert!(r.is_ok(), "{:#?}", r.findings);
    assert_eq!(r.package.unwrap().kill_after, Some(3));
    for bad in ["0", "-1", "2.5", "\"2\""] {
        let f = manifest_findings(&base.replace("\"kill_after\": 4", &format!("\"kill_after\": {bad}")));
        let code = if bad == "0" { "PKG-KILL kill_after" } else { "PKG-SCHEMA kill_after" };
        assert_eq!(f, vec![code.to_string()], "{bad}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ schema rules (REQ-001/002)

#[test]
fn every_required_field_is_named_when_missing() {
    let base = std::fs::read_to_string(fixtures().join("valid").join(MANIFEST)).unwrap();
    let CValue::Obj(m) = fid_canon::parse(base.as_bytes()).unwrap() else { panic!() };
    for field in REQUIRED_FIELDS {
        let mut m2 = m.clone();
        assert!(m2.remove(field).is_some(), "{field}");
        let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(m2)).unwrap();
        let f = manifest_findings(std::str::from_utf8(&bytes).unwrap());
        assert!(f.contains(&format!("PKG-SCHEMA {field}")), "{field}: {f:?}");
    }
}

#[test]
fn malformed_fields_are_named() {
    let base = std::fs::read_to_string(fixtures().join("valid").join(MANIFEST)).unwrap();
    let cases: &[(&str, &str, &str)] = &[
        ("\"acceptance-job@1\"", "\"acceptance-job@2\"", "PKG-SCHEMA format"),
        ("\"JOB.ACC.01\"", "\"JOB.ACC.1\"", "PKG-SCHEMA job_id"),
        ("\"JOB.ACC.01\"", "\"job.acc.01\"", "PKG-SCHEMA job_id"),
        ("\"demo-unassisted\"", "\"Demo\"", "PKG-SCHEMA variant_id"),
        ("\"inspect\"", "\"\"", "PKG-SCHEMA app"),
        ("\"launch-1\"", "\"launch 1\"", "PKG-SCHEMA milestones[0]"),
        ("\"gating\": \"gating\"", "\"gating\": true", "PKG-SCHEMA gating"),
        ("\"unassisted\"", "\"agent\"", "PKG-SCHEMA assistance"),
        ("\"any\"", "\"mouse_only\"", "PKG-SCHEMA input_mode"),
        ("\"file\": \"journal.jsonl\"", "\"file\": 3", "PKG-SCHEMA journal.file"),
        ("\"sha256:0dbe", "\"sha256:0DBE", "PKG-SCHEMA journal.sha256"),
        ("\"id\": \"flow-pkg-demo-part\"", "\"id\": \"X\"", "PKG-SCHEMA datasets[0].id"),
        ("\"id\": \"values\"", "\"id\": \"Values\"", "PKG-SCHEMA checks[1].id"),
        ("\"metric\": \"machine_s\"", "\"metric\": \"wall_s\"", "PKG-TARGET time_targets[1].metric"),
        ("\"metric\": \"machine_s\"", "\"metric\": \"task_s\"", "PKG-TARGET time_targets[1].metric"),
        ("\"seconds\": 60", "\"seconds\": 0", "PKG-TARGET time_targets[1].seconds"),
        ("\"seconds\": 60", "\"seconds\": \"60\"", "PKG-SCHEMA time_targets[1].seconds"),
        ("\"seconds\": 60", "\"seconds\": 1e9", "PKG-TARGET time_targets[1].seconds"),
    ];
    for (from, to, want) in cases {
        let text = base.replacen(from, to, 1);
        assert_ne!(text, base, "{from}");
        assert_eq!(manifest_findings(&text), vec![want.to_string()], "{from} -> {to}");
    }
    // A tracked package needs no task_s target; an empty list is allowed.
    let tracked = base.replace("\"gating\": \"gating\"", "\"gating\": \"tracked\"");
    let CValue::Obj(mut m) = fid_canon::parse(tracked.as_bytes()).unwrap() else { panic!() };
    m.insert("time_targets".into(), CValue::Arr(vec![]));
    let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(m)).unwrap();
    assert!(manifest_findings(std::str::from_utf8(&bytes).unwrap()).is_empty());
}

#[test]
fn optional_fields_are_known_and_journey_map_is_checked() {
    assert_eq!(OPTIONAL_FIELDS, ["kill_after", "journey_map"]);
    assert_eq!(FORMAT, "acceptance-job@1");
    let base = std::fs::read_to_string(fixtures().join("valid").join(MANIFEST)).unwrap();
    let with = |v: &str| base.replacen("\"format\"", &format!("\"journey_map\": {v},\n  \"format\""), 1);
    assert!(manifest_findings(&with("\"j1\"")).is_empty());
    assert_eq!(manifest_findings(&with("\"../map.json\"")), vec!["PKG-SCHEMA journey_map"]);
}

// ------------------------------------------------------------------ hostile inputs

#[test]
fn oversized_manifest_is_refused_before_parsing() {
    let dir = copy_fixture("valid", "big");
    let base = std::fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let pad = " ".repeat(MAX_MANIFEST_BYTES + 1 - base.len());
    std::fs::write(dir.join(MANIFEST), format!("{base}{pad}")).unwrap();
    let r = validate_dir(&dir, &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-SCHEMA package.json"]);
    assert!(r.digest.is_none());
    // Exactly at the bound it is accepted.
    std::fs::write(dir.join(MANIFEST), format!("{base}{}", &pad[1..])).unwrap();
    assert!(validate_dir(&dir, &catalogue()).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deep_nesting_and_bad_utf8_are_refused() {
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert_eq!(manifest_findings(&deep), vec!["PKG-SCHEMA package.json"]);
    let r = check_manifest(b"{\"format\": \"\xff\"}", &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-SCHEMA package.json"]);
    assert_eq!(manifest_findings("[]"), vec!["PKG-SCHEMA package.json"]);
    assert_eq!(manifest_findings(""), vec!["PKG-SCHEMA package.json"]);
}

#[test]
fn oversized_journal_is_refused_while_streaming() {
    let dir = fixtures().join("valid");
    let small = Limits { journal_bytes: 100, ..Limits::default() };
    let r = validate_dir_with(&dir, &catalogue(), &small);
    assert_eq!(short(&r.findings), vec!["PKG-DIGEST journal.file"]);
    assert!(r.findings[0].detail.contains("exceeds 100 bytes"));
    let len = std::fs::metadata(dir.join("journal.jsonl")).unwrap().len();
    let exact = Limits { journal_bytes: len, ..Limits::default() };
    assert!(validate_dir_with(&dir, &catalogue(), &exact).is_ok());
}

#[test]
fn crowded_folder_is_refused() {
    let dir = fixtures().join("valid");
    let tight = Limits { dir_entries: 1, ..Limits::default() };
    let r = validate_dir_with(&dir, &catalogue(), &tight);
    assert!(!r.is_ok());
    assert!(r.findings.iter().all(|f| f.detail.contains("more than 1 entries")), "{:#?}", r.findings);
}

#[test]
fn malformed_journals_are_refused() {
    for (name, body) in [
        ("empty", &b""[..]),
        ("blank", &b"{\"seq\":\"1\"}\n\n{\"seq\":\"2\"}\n"[..]),
        ("unterminated", &b"{\"seq\":\"1\"}\n{\"seq\":\"2\"}"[..]),
    ] {
        let dir = copy_fixture("valid", &format!("journal-{name}"));
        std::fs::write(dir.join("journal.jsonl"), body).unwrap();
        let r = validate_dir(&dir, &catalogue());
        // The digest no longer matches either.
        assert_eq!(short(&r.findings), vec!["PKG-DIGEST journal.sha256", "PKG-SCHEMA journal.file"], "{name}");
        assert_eq!(r.journal_entries, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn referenced_folder_is_not_a_file() {
    let dir = copy_fixture("valid", "dir-ref");
    std::fs::remove_file(dir.join("truth.json")).unwrap();
    std::fs::create_dir(dir.join("truth.json")).unwrap();
    let r = validate_dir(&dir, &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-CHECK checks[1].params.truth"]);
    assert_eq!(r.findings[0].detail, "is not a regular file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_not_followed() {
    let dir = copy_fixture("valid", "symlink");
    std::fs::remove_file(dir.join("truth.json")).unwrap();
    std::os::unix::fs::symlink(fixtures().join("valid").join("truth.json"), dir.join("truth.json")).unwrap();
    let r = validate_dir(&dir, &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-CHECK checks[1].params.truth"]);
    assert_eq!(r.findings[0].detail, "is or passes through a symbolic link");
    let link = scratch("symlink-dir");
    std::os::unix::fs::symlink(fixtures().join("valid"), link.join("pkg")).unwrap();
    let r = validate_dir(&link.join("pkg"), &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-SCHEMA -"]);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&link);
}

#[test]
fn missing_folder_and_manifest() {
    let r = validate_dir(&fixtures().join("no-such-package"), &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-SCHEMA -"]);
    let dir = scratch("no-manifest");
    let r = validate_dir(&dir, &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-SCHEMA package.json"]);
    // A manifest spelled with other letter case is not the manifest, on any platform.
    std::fs::copy(fixtures().join("valid").join(MANIFEST), dir.join("Package.json")).unwrap();
    let r = validate_dir(&dir, &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-SCHEMA package.json"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ catalogue

#[test]
fn catalogue_reading() {
    let c = catalogue();
    assert_eq!(c.len(), 1);
    assert!(c.get("flow-pkg-demo-part").is_some());
    // The repository catalogue parses too.
    let repo_cat = Catalogue::read(&repo().join("datasets.toml")).unwrap();
    assert!(!repo_cat.is_empty());
    for (text, why) in [
        ("[[dataset]]\nsha256 = \"00\"\n", "no string `id`"),
        ("[[dataset]]\nid = \"abc\"\nsha256 = \"00\"\n", "no 64-hex"),
        (
            "[[dataset]]\nid = \"abc\"\nsha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n[[dataset]]\nid = \"abc\"\nsha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n",
            "twice",
        ),
        ("dataset = 3\n", "not an array"),
        ("not toml [", "not TOML"),
    ] {
        let e = Catalogue::parse_toml(text).unwrap_err();
        assert!(e.contains(why), "{text}: {e}");
    }
    assert!(Catalogue::parse_toml("").unwrap().is_empty());
}

// ------------------------------------------------------------------ suite index

fn suite_short(name: &str) -> (Vec<String>, fid_accept::package::SuiteReport) {
    let bytes = std::fs::read(fixtures().join(name)).unwrap();
    let r = validate_suite(&fixtures(), &bytes, &catalogue());
    (short(&r.findings), r)
}

#[test]
fn suite_index_lists_registered_packages_with_digests() {
    let (f, r) = suite_short("suite.json");
    assert_eq!(f, expected("suite.json"));
    assert!(r.is_ok());
    let ix = r.index.unwrap();
    assert_eq!(ix.packages.len(), 1);
    assert_eq!(ix.packages[0].path, "valid");
    assert_eq!(ix.packages[0].digest.as_str(), digest_file("valid.digest"));
    assert_eq!(r.packages.len(), 1);
    assert!(r.packages[0].1.is_ok());
}

#[test]
fn suite_index_with_stale_digest() {
    let (f, r) = suite_short("suite-stale.json");
    assert_eq!(f, expected("suite-stale.json"));
    assert!(!r.is_ok());
}

#[test]
fn suite_index_hostile_entries() {
    let (f, r) = suite_short("suite-bad.json");
    assert_eq!(f, expected("suite-bad.json"));
    assert!(r.index.is_none());
    assert!(r.packages.is_empty());
    let (ix, f) = parse_suite(br#"{"format": "acceptance-suite@1", "packages": [], "packages": []}"#);
    assert!(ix.is_none());
    assert_eq!(short(&f), vec!["PKG-SCHEMA suite"]);
    let (ix, f) = parse_suite(br#"{"format": "acceptance-suite@2", "packages": [], "extra": 1}"#);
    assert!(ix.is_none());
    assert_eq!(short(&f), vec!["PKG-SCHEMA suite.extra", "PKG-SCHEMA suite.format"]);
}

#[test]
fn suite_entry_that_is_not_a_package_folder() {
    let d = digest_file("valid.digest");
    let index = format!(
        r#"{{"format": "acceptance-suite@1", "packages": [{{"path": "valid.digest", "digest": "{d}"}}, {{"path": "nowhere", "digest": "{d}"}}]}}"#
    );
    let r = validate_suite(&fixtures(), index.as_bytes(), &catalogue());
    assert_eq!(short(&r.findings), vec!["PKG-SCHEMA packages[0].path", "PKG-SCHEMA packages[1].path"]);
}

#[test]
fn repository_suite_index_is_valid() {
    let bytes = std::fs::read(repo().join("acceptance/suite.json")).unwrap();
    let cat = Catalogue::read(&repo().join("datasets.toml")).unwrap();
    let r = validate_suite(&repo(), &bytes, &cat);
    assert!(r.is_ok(), "{:#?} {:#?}", r.findings, r.packages);
}

// ------------------------------------------------------------------ published schema

fn obj<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(key).unwrap_or_else(|| panic!("no {key}")),
        _ => panic!("not an object at {key}"),
    }
}

fn strings(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                _ => panic!("not a string"),
            })
            .collect(),
        _ => panic!("not an array"),
    }
}

fn keys(v: &CValue) -> Vec<String> {
    match v {
        CValue::Obj(m) => m.keys().cloned().collect(),
        _ => panic!("not an object"),
    }
}

#[test]
fn published_schema_matches_the_validator() {
    let bytes = std::fs::read(repo().join("schema/acceptance-job-v1.json")).unwrap();
    let s = fid_canon::parse(&bytes).unwrap();
    assert_eq!(strings(obj(&s, "required")), REQUIRED_FIELDS.map(String::from).to_vec());
    let mut fields: Vec<String> = REQUIRED_FIELDS.iter().chain(OPTIONAL_FIELDS.iter()).map(|f| f.to_string()).collect();
    fields.sort();
    assert_eq!(keys(obj(&s, "properties")), fields);
    assert_eq!(
        obj(obj(&s, "properties"), "format"),
        &CValue::Obj([("const".to_string(), CValue::Str(FORMAT.into()))].into())
    );
    let defs = obj(&s, "$defs");
    let check = obj(defs, "check");
    let kinds = strings(obj(obj(obj(check, "properties"), "kind"), "enum"));
    assert_eq!(kinds, CHECK_KINDS.map(String::from).to_vec());
    // Each kind's parameter schema in the file has the validator's names and required set.
    let CValue::Arr(rules) = obj(check, "allOf") else { panic!() };
    let mut covered: Vec<String> = Vec::new();
    for rule in rules {
        let cond = obj(obj(obj(rule, "if"), "properties"), "kind");
        let names = match cond {
            CValue::Obj(m) if m.contains_key("const") => vec![match &m["const"] {
                CValue::Str(s) => s.clone(),
                _ => panic!(),
            }],
            _ => strings(obj(cond, "enum")),
        };
        let reference = match obj(obj(obj(obj(rule, "then"), "properties"), "params"), "$ref") {
            CValue::Str(r) => r.clone(),
            _ => panic!(),
        };
        let def = obj(obj(defs, "params"), reference.trim_start_matches("#/$defs/params/"));
        let mut want_all: Vec<String> = keys(obj(def, "properties"));
        let mut want_req: Vec<String> = strings(obj(def, "required"));
        want_all.sort();
        want_req.sort();
        for kind in names {
            let specs = param_specs(&kind).unwrap();
            let mut all: Vec<String> = specs.iter().map(|p| p.name.to_string()).collect();
            let mut req: Vec<String> = specs.iter().filter(|p| p.required).map(|p| p.name.to_string()).collect();
            all.sort();
            req.sort();
            assert_eq!(all, want_all, "{kind}");
            assert_eq!(req, want_req, "{kind}");
            covered.push(kind);
        }
    }
    covered.sort();
    let mut every: Vec<String> = CHECK_KINDS.map(String::from).to_vec();
    every.sort();
    assert_eq!(covered, every);
}
