//! Spec node FLOW.PERSONA.PERS: REQ-001..REQ-004 and the library side of AC-1..AC-3.
//! Expected findings come from `fixtures/flow/personas/expected.txt`, labelled by hand
//! from the spec's rules. The `cargo xtask personas-check` exit codes and output are
//! tested in `crates/fid-xtask/tests/personas_check.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_accept::persona::{
    check, check_files, parse_app_ids, Code, Report, BUILTIN_PROFILES, ENVIRONMENTS, FORMAT, MAX_INPUT_BYTES,
    PERSONA_IDS, PHASES,
};
use fid_canon::CValue;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    repo().join("fixtures/flow/personas")
}

fn apps() -> BTreeSet<String> {
    parse_app_ids(&std::fs::read_to_string(fixtures().join("apps.toml")).unwrap()).unwrap()
}

fn short(r: &Report) -> Vec<String> {
    r.findings.iter().map(|f| format!("{} {}", f.code, f.subject)).collect()
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

fn check_fixture(name: &str) -> Report {
    let r = check_files(&fixtures().join(name), &fixtures().join("apps.toml"));
    assert_eq!(short(&r), expected(name), "{name}: {:#?}", r.findings);
    // D0: the same input gives the same report.
    assert_eq!(check_files(&fixtures().join(name), &fixtures().join("apps.toml")), r, "{name}");
    r
}

#[test]
fn every_fixture_matches_its_hand_label() {
    for name in [
        "valid.json",
        "missing-07.json",
        "bad-app.json",
        "bad-profile.json",
        "extra.json",
        "duplicate.json",
        "bad-schema.json",
        "auditor.json",
        "wrong-format.json",
    ] {
        check_fixture(name);
    }
}

/// AC-1 (library side): the shipped file passes and holds the nine personas, sorted by ID.
#[test]
fn ac1_shipped_file_has_nine_personas() {
    let r = check_files(&repo().join("acceptance/personas/personas.json"), &fixtures().join("apps.toml"));
    assert!(r.is_ok(), "{:#?}", r.findings);
    assert_eq!(r.set.format, FORMAT);
    let ids: Vec<&str> = r.set.personas.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, PERSONA_IDS);
}

/// The fixture `valid.json` is the shipped file, so fixtures stay in step with it.
#[test]
fn valid_fixture_is_the_shipped_file() {
    let shipped = std::fs::read(repo().join("acceptance/personas/personas.json")).unwrap();
    let valid = std::fs::read(fixtures().join("valid.json")).unwrap();
    assert_eq!(fid_canon::parse(&shipped).unwrap(), fid_canon::parse(&valid).unwrap());
}

/// The shipped records carry the persona-to-app, phase and profile proposals of the
/// spec's build prompt (hand-copied from the NODE.md).
#[test]
fn shipped_records_follow_the_build_prompt() {
    let r = check_files(&repo().join("acceptance/personas/personas.json"), &fixtures().join("apps.toml"));
    let want = [
        ("JOB.PER.01", "report", "P1a", "lean"),
        ("JOB.PER.02", "inspect", "P1b", "lean"),
        ("JOB.PER.03", "inspect", "P1b", "lean"),
        ("JOB.PER.04", "probe", "P2", "lean"),
        ("JOB.PER.05", "report", "P1a", "regulated"),
        ("JOB.PER.06", "verifier", "P0", "regulated"),
        ("JOB.PER.07", "report", "P1a", "regulated"),
        ("JOB.PER.08", "sdk", "P2", "lean"),
        ("JOB.PER.09", "network", "P3", "lean"),
    ];
    let got: Vec<(&str, &str, &str, &str)> = r
        .set
        .personas
        .iter()
        .map(|p| (p.id.as_str(), p.primary_app.as_str(), p.entry_phase.as_str(), p.governance_profile.as_str()))
        .collect();
    assert_eq!(got, want);
    // REQ-003: the auditor works in an office or a lab.
    let auditor = r.set.personas.iter().find(|p| p.id == "JOB.PER.06").unwrap();
    assert!(auditor.environment == "office" || auditor.environment == "lab");
    assert_eq!(r.set.personas[2].environment, "shop_floor");
}

/// AC-2 (library side).
#[test]
fn ac2_missing_07() {
    let r = check_fixture("missing-07.json");
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, Code::Missing);
    assert_eq!(r.findings[0].subject, "JOB.PER.07");
}

/// AC-3 (library side): `primary_app: "measure"` is not in the app list.
#[test]
fn ac3_bad_app() {
    let r = check_fixture("bad-app.json");
    assert_eq!(r.findings[0].code, Code::App);
    assert!(r.findings[0].detail.contains("measure"), "{}", r.findings[0]);
}

/// REQ-004: `verifier` and `sdk` need no app list entry; anything else does.
#[test]
fn verifier_and_sdk_bypass_the_app_list() {
    let shipped = std::fs::read(repo().join("acceptance/personas/personas.json")).unwrap();
    let r = check(&shipped, &BTreeSet::new());
    let got = short(&r);
    // With an empty app list, the seven app-based personas fail and 06/08 do not.
    assert_eq!(
        got,
        ["01", "02", "03", "04", "05", "07", "09"].iter().map(|n| format!("PERS-APP JOB.PER.{n}")).collect::<Vec<_>>()
    );
}

#[test]
fn oversized_and_malformed_input_fail_closed() {
    let big = vec![b' '; MAX_INPUT_BYTES + 1];
    let r = check(&big, &apps());
    assert_eq!(short(&r), ["PERS-SCHEMA -"]);
    for bad in [&b"not json"[..], &b"[]"[..], &b"{\"format\":\"persona-set@1\",\"personas\":{}}"[..], &b""[..]] {
        let r = check(bad, &apps());
        assert!(r.findings.iter().any(|f| f.code == Code::Schema), "{:?}", String::from_utf8_lossy(bad));
    }
    // Duplicate keys are rejected by the parser.
    let r = check(b"{\"format\":\"persona-set@1\",\"format\":\"x\",\"personas\":[]}", &apps());
    assert_eq!(short(&r), ["PERS-SCHEMA -"]);
}

#[test]
fn missing_files_fail() {
    let r = check_files(&fixtures().join("no-such.json"), &fixtures().join("apps.toml"));
    assert_eq!(short(&r), ["PERS-SCHEMA -"]);
    let r = check_files(&fixtures().join("valid.json"), &fixtures().join("no-such.toml"));
    assert_eq!(short(&r), ["PERS-APP -"]);
}

#[test]
fn app_list_ids() {
    let ids: Vec<String> = apps().into_iter().collect();
    assert_eq!(ids, ["inspect", "network", "probe", "report"]);
    assert!(parse_app_ids("app = 3").is_err());
    assert!(parse_app_ids("[[app]]\njob = \"x\"").is_err());
    assert!(parse_app_ids("not toml [").is_err());
    assert!(parse_app_ids("").unwrap().is_empty());
}

/// Record-level rules on a single record, with every other ID present.
#[test]
fn record_rules() {
    let base = std::fs::read_to_string(fixtures().join("valid.json")).unwrap();
    let cases: [(&str, &str, &str); 6] = [
        ("\"environment\": \"office\"", "\"environment\": 7", "PERS-SCHEMA"),
        ("\"governance_profile\": \"regulated\"", "\"governance_profile\": \"assistive_only\"", ""),
        ("\"entry_phase\": \"P0\"", "\"entry_phase\": \"later\"", ""),
        ("\"primary_app\": \"verifier\"", "\"primary_app\": \"sdk\"", "PERS-APP"),
        ("\"entry_phase\": \"P0\",", "", "PERS-SCHEMA"),
        (
            "\"environment\": \"office\",\n      \"primary_app\": \"verifier\"",
            "\"environment\": \"lab\",\n      \"primary_app\": \"verifier\"",
            "",
        ),
    ];
    for (from, to, code) in cases {
        // Each `from` is matched in the auditor record (the only `verifier`/`P0` record);
        // the first and last cases use its `office` environment.
        let at = base.find("\"id\": \"JOB.PER.06\"").unwrap();
        let (head, tail) = base.split_at(at);
        assert!(tail.contains(from), "{from}");
        let text = format!("{head}{}", tail.replacen(from, to, 1));
        let r = check(text.as_bytes(), &apps());
        let got = short(&r);
        if code.is_empty() {
            assert!(got.is_empty(), "{to}: {got:?}");
        } else {
            assert_eq!(got, [format!("{code} JOB.PER.06")], "{to}");
        }
    }
}

/// The published JSON Schema lists the same values as the check.
#[test]
fn schema_matches_the_check() {
    let text = std::fs::read(repo().join("schema/persona-set-v1.schema.json")).unwrap();
    let CValue::Obj(top) = fid_canon::parse(&text).unwrap() else { panic!("schema is not an object") };
    let CValue::Obj(defs) = &top["$defs"] else { panic!("no $defs") };
    let CValue::Obj(persona) = &defs["persona"] else { panic!("no persona") };
    let CValue::Obj(props) = &persona["properties"] else { panic!("no properties") };
    let strings = |key: &str| -> Vec<String> {
        let CValue::Obj(p) = &props[key] else { panic!("{key}") };
        let CValue::Arr(items) = &p["enum"] else { panic!("{key} enum") };
        items
            .iter()
            .map(|v| match v {
                CValue::Str(s) => s.clone(),
                _ => panic!("{key}"),
            })
            .collect()
    };
    assert_eq!(strings("environment"), ENVIRONMENTS);
    assert_eq!(strings("entry_phase"), PHASES);
    assert_eq!(strings("governance_profile"), BUILTIN_PROFILES);
    let CValue::Obj(format) = &top["properties"] else { panic!() };
    let CValue::Obj(f) = &format["format"] else { panic!() };
    assert_eq!(f["const"], CValue::Str(FORMAT.to_string()));
}
