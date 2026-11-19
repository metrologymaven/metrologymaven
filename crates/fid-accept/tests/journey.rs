//! Spec node FLOW.PERSONA.SCHEMA: REQ-001..REQ-005 and AC-1..AC-4.
//! Expected findings come from `fixtures/flow/journey/expected.txt`, labelled by hand from the
//! spec's rules (and cross-checked by the independent reference in `make_fixtures.py`); the
//! digest comes from `make_fixtures.py`, an independent standard-library Python reference.
//! Inputs too large to commit (over 1 MiB, 501 steps, deep nesting) are built here, with
//! outcomes labelled from the same rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_accept::journey::{
    is_node_id, map_digest, persona_mentions, validate, Actor, Code, Finding, Handoff, Kind, ScreenId, StepTarget,
    ACTORS, FORMAT, KINDS, MAX_DEPTH, MAX_MAP_BYTES, MAX_STEPS, PERSONA_ONLY_FIELD, REQUIRED_FIELDS,
    STEP_OPTIONAL_FIELDS, STEP_REQUIRED_FIELDS, TARGET_FIELDS,
};
use fid_accept::persona::PHASES;
use fid_canon::CValue;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    repo().join("fixtures/flow/journey")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

fn label(f: &Finding) -> String {
    match f.step {
        Some(k) => format!("{} {} step {} {}", f.code, f.map_id, k, f.field),
        None => format!("{} {} {}", f.code, f.map_id, f.field),
    }
}

fn labels(findings: &[Finding]) -> Vec<String> {
    findings.iter().map(label).collect()
}

/// Rows of `expected.txt` as (fixture, labels in file order), fixtures in file order.
fn expected() -> Vec<(String, Vec<String>)> {
    let text = std::fs::read_to_string(fixtures().join("expected.txt")).unwrap();
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (name, value) = line.split_once('\t').unwrap();
        if rows.last().map(|(n, _)| n.as_str()) != Some(name) {
            assert!(rows.iter().all(|(n, _)| n != name), "{name} is split in expected.txt");
            rows.push((name.to_string(), Vec::new()));
        }
        if value != "ok" {
            rows.last_mut().unwrap().1.push(value.to_string());
        }
    }
    rows
}

// ------------------------------------------------------------------ hand-labelled fixtures

#[test]
fn every_fixture_gives_its_hand_labelled_findings() {
    let rows = expected();
    let mut on_disk: Vec<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".json"))
        .collect();
    on_disk.sort();
    let mut listed: Vec<String> = rows.iter().map(|(n, _)| n.clone()).collect();
    listed.sort();
    assert_eq!(listed, on_disk, "expected.txt must list every fixture");
    for (name, want) in rows {
        let r = validate(&read(&name));
        assert_eq!(labels(&r.findings), want, "{name}");
        let blocking = r.findings.iter().any(|f| f.code != Code::AgentHumanOnly);
        assert_eq!(r.map.is_some(), !blocking, "{name}: map returned only without blocking findings (REQ-004)");
    }
}

/// AC-1: the 12-step job map validates and its digest matches the reference.
#[test]
fn ac1_valid_job_map_and_digest() {
    let bytes = read("valid-job.json");
    let r = validate(&bytes);
    assert!(r.is_ok(), "{:?}", r.findings);
    let want = std::fs::read_to_string(fixtures().join("valid-job.digest")).unwrap();
    assert_eq!(r.digest.unwrap().to_string(), want.trim());
    assert_eq!(map_digest(&bytes).unwrap().to_string(), want.trim());
    let map = r.map.unwrap();
    assert_eq!(map.format, FORMAT);
    assert_eq!(map.map_id, "j1");
    assert_eq!(map.kind, Kind::Job);
    assert_eq!(map.subject, "JOB.ACC.01");
    assert_eq!(map.exercised_by, None);
    assert_eq!(map.steps.len(), 12);
    for (i, s) in map.steps.iter().enumerate() {
        assert_eq!(s.n, i as u64 + 1);
        assert_eq!(s.actor, Actor::Human);
        assert!(matches!(s.target, StepTarget::Command(_)));
    }
    assert!(map.steps[3].human_only);
    assert!(map.steps[3].decision.is_some());
    assert_eq!(map.steps[0].handoff, Handoff::Step(2));
    assert!(matches!(map.steps[11].handoff, Handoff::Text(_)));
    match &map.steps[5].target {
        StepTarget::Command(c) => assert_eq!(c.as_str(), "align.run_bestfit@1"),
        StepTarget::Screen(_) => panic!("step 6 is a command"),
    }
}

/// AC-2: step 4 human-only and attributed to an agent.
#[test]
fn ac2_agent_on_human_only_step() {
    let r = validate(&read("agent-humanonly.json"));
    assert_eq!(r.findings.len(), 1);
    let f = &r.findings[0];
    assert_eq!(f.code, Code::AgentHumanOnly);
    assert_eq!(f.code.as_str(), "MAP-AGENT-HUMANONLY");
    assert_eq!((f.map_id.as_str(), f.step), ("j1", Some(4)));
}

/// AC-3: steps numbered 1, 2, 4.
#[test]
fn ac3_gap_in_step_numbers() {
    let r = validate(&read("gap.json"));
    assert!(r.map.is_none());
    assert_eq!(r.findings.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(), ["MAP-STEP-ORDER"]);
}

/// AC-4: a step with both a command and a screen.
#[test]
fn ac4_command_and_screen() {
    let r = validate(&read("both.json"));
    assert!(r.map.is_none());
    assert_eq!(r.findings.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(), ["MAP-SCHEMA"]);
}

#[test]
fn persona_map_keeps_screens_and_exercised_by() {
    let map = validate(&read("valid-persona.json")).map.unwrap();
    assert_eq!(map.kind, Kind::Persona);
    assert_eq!(map.exercised_by, Some(vec!["JOB.ACC.05".to_string()]));
    match &map.steps[0].target {
        StepTarget::Screen(s) => assert_eq!(s.as_str(), "SCR.KIOSK.LOGIN"),
        StepTarget::Command(_) => panic!("step 1 is a screen"),
    }
    assert_eq!(map.steps[1].phase.as_deref(), Some("P2"));
    assert_eq!(map.steps[2].actor, Actor::System);
}

/// Every journey map shipped under `acceptance/` validates (REQ-FLOW.PERSONA-003).
#[test]
fn shipped_maps_validate() {
    let mut seen = 0;
    let mut stack = vec![repo().join("acceptance")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "json") {
                let bytes = std::fs::read(&p).unwrap();
                let Ok(CValue::Obj(top)) = fid_canon::parse(&bytes) else { continue };
                if top.get("format") != Some(&CValue::Str(FORMAT.to_string())) {
                    continue;
                }
                let r = validate(&bytes);
                assert!(r.is_ok(), "{}: {:?}", p.display(), r.findings);
                seen += 1;
            }
        }
    }
    assert!(seen >= 9, "expected at least the nine persona maps, found {seen}");
}

// ------------------------------------------------------------------ size bounds (REQ-004)

fn job_map(steps: usize) -> String {
    let mut s = String::from(
        r#"{"format":"journey-map@1","map_id":"big","subject":"JOB.ACC.01","kind":"job","revision":1,"app":"inspect","phase":"P0","personas":[],"steps":["#,
    );
    for n in 1..=steps {
        if n > 1 {
            s.push(',');
        }
        let handoff = if n < steps { (n + 1).to_string() } else { "\"done\"".to_string() };
        s.push_str(&format!(
            r#"{{"n":{n},"actor":"system","command":"import.cad_step@1","owner":"IO.CAD","handoff":{handoff},"human_only":false}}"#
        ));
    }
    s.push_str("]}");
    s
}

#[test]
fn five_hundred_steps_pass_and_501_is_map_size() {
    assert!(validate(job_map(MAX_STEPS).as_bytes()).is_ok());
    let r = validate(job_map(MAX_STEPS + 1).as_bytes());
    assert_eq!(labels(&r.findings), ["MAP-SIZE big steps"]);
    assert!(r.map.is_none());
}

#[test]
fn over_one_mib_is_map_size() {
    let mut text = job_map(1).into_bytes();
    let pad = MAX_MAP_BYTES + 1 - text.len();
    text.extend(std::iter::repeat_n(b' ', pad));
    assert_eq!(text.len(), MAX_MAP_BYTES + 1);
    let r = validate(&text);
    assert_eq!(labels(&r.findings), ["MAP-SIZE - -"]);
    assert!(r.map.is_none() && r.digest.is_none());
    assert!(map_digest(&text).is_none());
    // Exactly 1 MiB is accepted.
    text.pop();
    assert!(validate(&text).is_ok());
}

#[test]
fn deep_nesting_is_map_size() {
    let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    // Within the depth bound it parses and then fails the schema.
    assert_eq!(labels(&validate(ok.as_bytes()).findings), ["MAP-SCHEMA - -"]);
    let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert_eq!(labels(&validate(deep.as_bytes()).findings), ["MAP-SIZE - -"]);
}

// ------------------------------------------------------------------ other rules

#[test]
fn malformed_input_does_not_panic() {
    for input in [&b""[..], b"null", b"[]", b"\"x\"", b"{", b"\xff\xfe", b"{\"steps\":[1,null,\"x\"]}"] {
        let r = validate(input);
        assert!(!r.is_ok());
        assert!(r.map.is_none());
        assert!(r.findings.iter().all(|f| f.code == Code::Schema), "{input:?}: {:?}", r.findings);
    }
}

#[test]
fn findings_are_sorted_and_repeatable() {
    for (name, _) in expected() {
        let bytes = read(&name);
        let a = validate(&bytes);
        let b = validate(&bytes);
        assert_eq!(a, b);
        let mut sorted = a.findings.clone();
        sorted.sort();
        assert_eq!(a.findings, sorted, "{name}");
        for f in &a.findings {
            assert!(!f.to_string().is_empty());
        }
    }
}

#[test]
fn digest_ignores_key_order_and_whitespace() {
    let a = br#"{"a":1,"b":[true,"x"]}"#;
    let b = b"{ \"b\" : [ true , \"x\" ] ,\n \"a\" : 1 }";
    assert_eq!(map_digest(a), map_digest(b));
    assert!(map_digest(a).is_some());
}

#[test]
fn integer_handoff_must_name_the_next_step() {
    let base = job_map(3);
    // Step 1 hands off to step 3, skipping step 2.
    let skip = base.replacen(r#""handoff":2"#, r#""handoff":3"#, 1);
    assert_eq!(labels(&validate(skip.as_bytes()).findings), ["MAP-STEP-ORDER big step 1 handoff"]);
    let zero = base.replacen(r#""handoff":2"#, r#""handoff":0"#, 1);
    assert_eq!(labels(&validate(zero.as_bytes()).findings), ["MAP-SCHEMA big step 1 handoff"]);
    let frac = base.replacen(r#""handoff":2"#, r#""handoff":2.5"#, 1);
    assert_eq!(labels(&validate(frac.as_bytes()).findings), ["MAP-SCHEMA big step 1 handoff"]);
}

#[test]
fn syntax_helpers() {
    assert!(is_node_id("CORE.JRN.CANON"));
    assert!(is_node_id("FLOW.INSPECT.J7GF"));
    assert!(!is_node_id("core.jrn"));
    assert!(!is_node_id("CORE..JRN"));
    assert!(!is_node_id("CORE.1JRN"));
    assert!(!is_node_id(""));
    assert!(ScreenId::is_valid("SCR.KIOSK.LOGIN"));
    assert!(!ScreenId::is_valid("1SCR"));
    assert!(!ScreenId::is_valid("SCR KIOSK"));
    assert!(!ScreenId::is_valid(""));
    assert_eq!(
        persona_mentions("JOB.PER.05 hands to JOB.PER.07; not XJOB.PER.01, JOB.PER.012 or JOB.PER.1"),
        ["JOB.PER.05", "JOB.PER.07"]
    );
    assert_eq!(persona_mentions("ends with JOB.PER.09"), ["JOB.PER.09"]);
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

fn sorted(items: impl IntoIterator<Item = impl ToString>) -> Vec<String> {
    let mut v: Vec<String> = items.into_iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

#[test]
fn published_schema_matches_the_validator() {
    let s = fid_canon::parse(&std::fs::read(repo().join("schema/journey-map-v1.json")).unwrap()).unwrap();
    assert_eq!(strings(obj(&s, "required")), REQUIRED_FIELDS.map(String::from).to_vec());
    assert_eq!(keys(obj(&s, "properties")), sorted(REQUIRED_FIELDS.iter().chain([PERSONA_ONLY_FIELD].iter())));
    let props = obj(&s, "properties");
    assert_eq!(obj(obj(props, "format"), "const"), &CValue::Str(FORMAT.to_string()));
    assert_eq!(strings(obj(obj(props, "kind"), "enum")), KINDS.map(|(k, _)| k.to_string()).to_vec());
    let steps = obj(props, "steps");
    assert_eq!(obj(steps, "maxItems"), &CValue::Num(MAX_STEPS as f64));
    assert_eq!(obj(steps, "minItems"), &CValue::Num(1.0));
    let defs = obj(&s, "$defs");
    assert_eq!(strings(obj(obj(defs, "phase"), "enum")), PHASES.map(String::from).to_vec());
    let step = obj(defs, "step");
    assert_eq!(strings(obj(step, "required")), STEP_REQUIRED_FIELDS.map(String::from).to_vec());
    assert_eq!(
        keys(obj(step, "properties")),
        sorted(STEP_REQUIRED_FIELDS.iter().chain(STEP_OPTIONAL_FIELDS.iter()).chain(TARGET_FIELDS.iter()))
    );
    assert_eq!(obj(step, "additionalProperties"), &CValue::Bool(false));
    assert_eq!(obj(&s, "additionalProperties"), &CValue::Bool(false));
    let step_props = obj(step, "properties");
    assert_eq!(strings(obj(obj(step_props, "actor"), "enum")), ACTORS.map(String::from).to_vec());
    assert_eq!(obj(obj(step_props, "screen"), "pattern"), &CValue::Str(ScreenId::PATTERN.to_string()));
    assert_eq!(obj(obj(step_props, "command"), "pattern"), &CValue::Str(fid_types::CommandId::PATTERN.to_string()));
    // Each kind's subject group is stated in the conditional rules.
    let CValue::Arr(rules) = obj(&s, "allOf") else { panic!("no allOf") };
    for (kind, group) in KINDS {
        let rule = rules
            .iter()
            .find(|r| obj(obj(obj(obj(r, "if"), "properties"), "kind"), "const") == &CValue::Str(kind.to_string()))
            .unwrap_or_else(|| panic!("no rule for {kind}"));
        let pattern = obj(obj(obj(obj(rule, "then"), "properties"), "subject"), "pattern");
        assert_eq!(pattern, &CValue::Str(format!(r"^JOB\.{group}\.[0-9]{{2}}$")), "{kind}");
    }
}
