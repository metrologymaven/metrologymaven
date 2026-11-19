//! Acceptance criteria of spec node CORE.REG.DEF.GUIDE (AC-1 .. AC-4). Fixtures and their
//! expected codes come from `fixtures/core/reg/def/guide/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::guide::{GuideStep, MAX_PURPOSE, MAX_STEPS};
use fid_registry::{
    check_guides, guides_from_value, guides_to_value, task_guide_schema, CommandDef, GuideError, TaskGuide,
};
use fid_types::CommandId;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> CValue {
    parse(&std::fs::read(root().join("fixtures/core/reg/def/guide").join(name)).unwrap()).unwrap()
}

fn commands() -> Vec<CommandDef> {
    let CValue::Arr(items) = fixture("commands.json") else { panic!("array expected") };
    items.into_iter().map(|v| CommandDef::from_value(v).unwrap()).collect()
}

fn ok_guides() -> Vec<TaskGuide> {
    guides_from_value(fixture("guides_ok.json")).unwrap()
}

fn align_scan() -> TaskGuide {
    ok_guides().into_iter().find(|g| g.id == "demo.align_scan").unwrap()
}

fn codes(errors: &[GuideError]) -> Vec<&'static str> {
    errors.iter().map(GuideError::code).collect()
}

/// AC-1: the fixture guides load, round-trip equal (sorted by ID, steps in order) and pass.
#[test]
fn ac1_round_trip_and_checks_pass() {
    let value = fixture("guides_ok.json");
    let guides = guides_from_value(value.clone()).unwrap();
    let g = align_scan();
    assert_eq!(g.steps.len(), 3);
    let order: Vec<&str> = g.steps.iter().map(|s| s.command.as_str()).collect();
    assert_eq!(order, ["demo.load_scan@1", "demo.fit_alignment@1", "demo.apply_alignment@1"]);
    assert_eq!(guides_to_value(&guides), value, "guides must round-trip byte for byte");
    assert_eq!(guides_from_value(guides_to_value(&guides)).unwrap(), guides);
    assert_eq!(check_guides(&guides, &commands()), Vec::new());
}

/// REQ-005: a list is written sorted by guide ID whatever the declaration order.
#[test]
fn list_is_sorted_by_id() {
    let mut guides = ok_guides();
    guides.reverse();
    assert_ne!(guides[0].id, "demo.align_scan");
    assert_eq!(guides_to_value(&guides), fixture("guides_ok.json"));
}

/// AC-2 .. AC-4 and the other error paths: each case fails with exactly the labelled codes.
#[test]
fn bad_cases_fail_with_labelled_codes() {
    let CValue::Arr(cases) = fixture("guides_bad.json") else { panic!() };
    assert!(cases.len() >= 30);
    let defs = commands();
    for c in cases {
        let CValue::Obj(mut c) = c else { panic!() };
        let Some(CValue::Str(name)) = c.remove("name") else { panic!() };
        let Some(CValue::Arr(expect)) = c.remove("expect") else { panic!("{name}: no expect") };
        let expect: Vec<String> = expect
            .into_iter()
            .map(|e| match e {
                CValue::Str(s) => s,
                _ => panic!(),
            })
            .collect();
        let got: Vec<String> = match guides_from_value(c.remove("guides").unwrap()) {
            Err(e) => vec![e.code().to_string()],
            Ok(guides) => check_guides(&guides, &defs).iter().map(|e| e.code().to_string()).collect(),
        };
        assert_eq!(got, expect, "case {name}");
    }
}

/// AC-2: the error names the guide and the step.
#[test]
fn ac2_ghost_command_names_guide_and_step() {
    let mut g = align_scan();
    g.steps[1].command = CommandId::parse("demo.ghost_cmd@1").unwrap();
    let errors = check_guides(&[g], &commands());
    assert_eq!(
        errors,
        [GuideError::UnknownCommand { guide: "demo.align_scan".into(), step: 1, command: "demo.ghost_cmd@1".into() }]
    );
    assert!(errors[0].to_string().contains("demo.align_scan"));
}

/// AC-3: an M1 step fails the build.
#[test]
fn ac3_move_axis_is_forbidden() {
    let mut g = align_scan();
    g.steps[0].command = CommandId::parse("demo.move_axis@1").unwrap();
    assert_eq!(codes(&check_guides(&[g], &commands())), ["guide_forbidden_step"]);
}

/// AC-4: a guide built in code (not read from JSON) is still size-checked by the build.
#[test]
fn ac4_too_large_in_code() {
    let defs = commands();
    let mut g = align_scan();
    let first = g.steps[0].clone();
    g.steps = vec![first; MAX_STEPS + 1];
    assert_eq!(codes(&check_guides(&[g], &defs)), ["guide_too_large"]);

    let mut g = align_scan();
    g.steps[2].purpose = "x".repeat(MAX_PURPOSE + 1);
    assert_eq!(codes(&check_guides(&[g.clone()], &defs)), ["guide_too_large"]);
    g.steps[2].purpose = "\u{e9}".repeat(MAX_PURPOSE);
    assert_eq!(codes(&check_guides(&[g], &defs)), Vec::<&str>::new(), "limits count characters, not bytes");
}

/// The macro form declares the same guide as the fixture.
#[test]
fn macro_declares_guides_in_source_order() {
    mod demo {
        fid_registry::fid_guides! {
            "demo.align_scan" {
                title: "Align a scan to CAD",
                summary: "Load a point cloud, fit it to the nominal CAD model and apply the alignment to the working document.",
                profile: "core",
                steps: [
                    "demo.load_scan@1" => "Load the point cloud to align.",
                    "demo.fit_alignment@1" => "Fit the scan to the CAD model and review the residuals.",
                    "demo.apply_alignment@1" => "Apply the accepted alignment to the working document.",
                ],
            },
            "demo.a_second" {
                title: "Second",
                summary: "Declared after the first.",
                profile: "core",
                steps: ["demo.load_scan@1" => "Load."],
            },
        }
    }
    mod broken {
        fid_registry::fid_guides! {
            "Demo.bad" { title: "t", summary: "s", profile: "core", steps: ["demo.load_scan@1" => "p"] }
        }
    }
    let guides = demo::guides().unwrap();
    assert_eq!(guides.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["demo.align_scan", "demo.a_second"]);
    assert_eq!(guides[0], align_scan());
    assert_eq!(broken::guides().unwrap_err().code(), "guide_bad_id");
}

#[test]
fn constructor_refuses_non_canonical_command() {
    let err = TaskGuide::new("demo.x", "t", "s", "core", &[("demo.load_scan@01", "p")]).unwrap_err();
    assert_eq!(err.code(), "bad_type");
    let step = GuideStep { command: CommandId::parse("demo.load_scan@1").unwrap(), purpose: "p".into() };
    assert_eq!(TaskGuide::new("demo.x", "t", "s", "core", &[("demo.load_scan@1", "p")]).unwrap().steps, [step]);
}

#[test]
fn json_bytes_entry_point() {
    let one = br#"{"id":"demo.x","title":"t","summary":"s","profile":"core","steps":[{"command":"demo.load_scan@1","purpose":"p"}]}"#;
    assert_eq!(TaskGuide::from_json(one).unwrap().id, "demo.x");
    assert!(TaskGuide::from_json(b"{").is_err());
}

/// REQ-004: the schema allows no field beyond the declared text fields and command references.
#[test]
fn schema_is_generated_and_closed() {
    let mut want = to_canonical_bytes(&task_guide_schema()).unwrap();
    want.push(b'\n');
    let have = std::fs::read(root().join("schema/task-guide.schema.json")).unwrap_or_default();
    assert!(
        have == want,
        "schema/task-guide.schema.json is out of date. Replace its contents with:\n{}",
        String::from_utf8_lossy(&want)
    );
    let CValue::Obj(schema) = task_guide_schema() else { panic!() };
    assert_eq!(schema["additionalProperties"], CValue::Bool(false));
    let CValue::Obj(props) = &schema["properties"] else { panic!() };
    assert_eq!(props.keys().map(String::as_str).collect::<Vec<_>>(), ["id", "profile", "steps", "summary", "title"]);
}
