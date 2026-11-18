//! Spec node FLOW.STUDY.PROTO: REQ-002, REQ-005 and the library side of AC-1..AC-3.
//! Expected findings come from `fixtures/flow/study/tasks/expected.txt`, labelled by hand
//! from the spec's rules. The task list is checked against the shipped journey maps and
//! persona file. The `cargo xtask study-tasks-check` exit codes and output are tested in
//! `crates/fid-xtask/tests/study_tasks_check.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_accept::study::{
    check, check_files, is_domain_command, parse_map, parse_persona_ids, read_maps, Code, Report, FIRST_TIME_JOBS,
    RECORD_FORMAT,
};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    repo().join("fixtures/flow/study/tasks")
}

fn maps_dir() -> PathBuf {
    repo().join("acceptance/journeys/persona")
}

fn personas() -> PathBuf {
    repo().join("acceptance/personas/personas.json")
}

fn protocol() -> PathBuf {
    repo().join("docs/study/protocol.md")
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

#[test]
fn every_fixture_matches_its_hand_label() {
    for name in [
        "valid.json",
        "no-map.json",
        "bad-command.json",
        "unknown-persona.json",
        "unknown-map.json",
        "wrong-order.json",
        "extra-field.json",
        "missing-j3.json",
        "wrong-format.json",
    ] {
        let r = check_files(&fixtures().join(name), &maps_dir(), &personas(), None);
        assert_eq!(short(&r), expected(name), "{name}: {:#?}", r.findings);
        // D0: the same input gives the same report.
        assert_eq!(check_files(&fixtures().join(name), &maps_dir(), &personas(), None), r, "{name}");
    }
}

/// AC-1 (library side): the shipped task list, maps, persona file and protocol pass; every
/// job has a map and every command ID has the form `domain.verb_object@N`.
#[test]
fn ac1_shipped_task_list_passes() {
    let r = check_files(&repo().join("acceptance/study/tasks.json"), &maps_dir(), &personas(), Some(&protocol()));
    assert!(r.is_ok(), "{:#?}", r.findings);
    let jobs: Vec<&str> = r.tasks.iter().map(|t| t.job_id.as_str()).collect();
    for job in FIRST_TIME_JOBS {
        assert!(jobs.contains(&job), "{job} missing");
    }
    for t in &r.tasks {
        assert!(is_domain_command(&t.start_command), "{}", t.start_command);
        assert!(is_domain_command(&t.end_command), "{}", t.end_command);
    }
}

/// REQ-002: the end event of every studied job is the journaled export of the report.
#[test]
fn req2_end_event_is_report_export() {
    let r = check_files(&repo().join("acceptance/study/tasks.json"), &maps_dir(), &personas(), None);
    for t in &r.tasks {
        assert!(t.end_command.starts_with("report.export_"), "{}", t.end_command);
        assert!(t.start_command.starts_with("import."), "{}", t.start_command);
    }
}

/// The shipped task list is the base of the fixtures: `valid.json` equals it field for field.
#[test]
fn valid_fixture_is_the_shipped_list() {
    let maps = read_maps(&maps_dir(), &mut Vec::new());
    let ids = parse_persona_ids(&std::fs::read(personas()).unwrap()).unwrap();
    let shipped = check(&std::fs::read(repo().join("acceptance/study/tasks.json")).unwrap(), &maps, &ids, None);
    let fixture = check(&std::fs::read(fixtures().join("valid.json")).unwrap(), &maps, &ids, None);
    assert_eq!(shipped, fixture);
}

/// AC-2 (library side): job `JOB.ACC.99` gives `TASK-NO-MAP`.
#[test]
fn ac2_unmapped_job() {
    let r = check_files(&fixtures().join("no-map.json"), &maps_dir(), &personas(), None);
    assert!(r.findings.iter().any(|f| f.code == Code::NoMap && f.subject == "JOB.ACC.99"), "{:#?}", r.findings);
}

/// AC-3 (library side): the shipped protocol cites the record format; a protocol that does
/// not gives `TASK-DOC`.
#[test]
fn ac3_protocol_cites_record_format() {
    let doc = std::fs::read_to_string(protocol()).unwrap();
    assert!(doc.contains(RECORD_FORMAT));
    let r = check_files(
        &repo().join("acceptance/study/tasks.json"),
        &maps_dir(),
        &personas(),
        Some(&fixtures().join("protocol-no-record.md")),
    );
    assert_eq!(short(&r), vec!["TASK-DOC protocol".to_string()], "{:#?}", r.findings);
    assert!(r.findings[0].detail.contains(RECORD_FORMAT));
}

/// A missing protocol file is a `TASK-DOC` finding, not a crash.
#[test]
fn missing_protocol_is_a_finding() {
    let r = check_files(
        &repo().join("acceptance/study/tasks.json"),
        &maps_dir(),
        &personas(),
        Some(&fixtures().join("no-such.md")),
    );
    assert_eq!(short(&r), vec!["TASK-DOC protocol".to_string()]);
}

#[test]
fn schema_errors_do_not_panic() {
    let maps = read_maps(&maps_dir(), &mut Vec::new());
    let ids = parse_persona_ids(&std::fs::read(personas()).unwrap()).unwrap();
    let inputs: [&[u8]; 5] =
        [b"", b"[]", b"{}", b"{\"format\":\"study-tasks@1\",\"tasks\":[1,{}]}", b"{\"a\":1,\"a\":2}"];
    for input in inputs {
        let r = check(input, &maps, &ids, None);
        assert!(!r.is_ok());
        assert!(r.findings.iter().all(|f| f.code == Code::Schema), "{:#?}", r.findings);
    }
}

#[test]
fn map_parser_reads_commands_in_order() {
    let m = parse_map(&std::fs::read(maps_dir().join("per-01.json")).unwrap()).unwrap();
    assert_eq!(m.map_id, "per-01");
    assert!(m.exercised_by.contains("JOB.ACC.02"));
    let dmis = m.commands.iter().position(|c| c == "import.results_dmis@1").unwrap();
    let export = m.commands.iter().position(|c| c == "report.export_pdf@1").unwrap();
    assert!(dmis < export);
}
