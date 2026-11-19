//! Acceptance criteria of spec node GDT.DRF.DATUM (AC-1 .. AC-5) plus the error paths, branch
//! rules and limits of the leaf's Design. The plan fixture, the profile and the expected event
//! and result bytes come from the independent stdlib-Python reference
//! `fixtures/gdt/drf/make_cases.py` (`expected.json`); error cases are hand-labelled from the
//! leaf's rules. All data is synthetic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{parse_profile, Branch, DprofState, KeyVocabulary};
use fid_gdt::chars::spec::{GoverningSpec, InheritedFrom, SpecState, Standards, StdRef};
use fid_gdt::dmt_flow::roster::Dialect;
use fid_gdt::drf::datum::{
    active_label_pattern, command_def, invoke_cli, invoke_mcp, load_project, project_bytes, replay, DatumError,
    DatumEvent, DatumLabel, DatumModel, DefineRequest, Defined, FeatureType, LabelPattern, TargetKind, TargetRequest,
    DEFINE_COMMAND, DEFINE_DATUM_CLI, DEFINE_DATUM_MCP,
};
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;

const H: Principal = Principal::Human;
const A: Principal = Principal::Agent;
const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/gdt/drf/");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{name}")).unwrap()
}

fn exp(key: &str) -> String {
    let CValue::Obj(mut o) = fid_canon::parse(&read("expected.json")).unwrap() else { panic!("expected") };
    let Some(CValue::Str(s)) = o.remove(key) else { panic!("{key}") };
    s
}

fn plan() -> DatumModel {
    load_project(&read("block_plan.fidproj")).unwrap()
}

fn pat() -> LabelPattern {
    LabelPattern::parse(&exp("label_pattern")).unwrap()
}

fn cli(m: &mut DatumModel, p: Principal, args: &[&str]) -> Result<Defined, DatumError> {
    let mut all = vec![DEFINE_DATUM_CLI];
    all.extend_from_slice(args);
    invoke_cli(m, p, &pat(), &all)
}

fn err(r: Result<Defined, DatumError>) -> String {
    r.unwrap_err().to_string()
}

fn ev(d: &Defined) -> String {
    String::from_utf8(d.event().unwrap().canonical_bytes().unwrap()).unwrap()
}

fn snapshot(m: &DatumModel) -> Vec<u8> {
    project_bytes(m).unwrap()
}

const AC1: [&str; 9] = ["--label", "A", "--feature", "f1", "--faces", "face:12", "--type", "plane", "--json"];
const AC3: [&str; 15] = [
    "--label",
    "A",
    "--feature",
    "f1",
    "--faces",
    "face:12",
    "--type",
    "plane",
    "--target",
    "A2,point,pt:2",
    "--target",
    "A1,point,pt:1",
    "--target",
    "A3,area,ar:1",
    "--replace",
];

fn define(
    m: &mut DatumModel,
    p: Principal,
    label: &str,
    feature: &str,
    faces: &[&str],
    t: &str,
) -> Result<Defined, DatumError> {
    let req = DefineRequest { label, feature, faces, feature_type: t, targets: &[], replace: false };
    m.define_datum(p, &pat(), &req, false)
}

fn with_targets(m: &mut DatumModel, targets: &[TargetRequest<'_>]) -> Result<Defined, DatumError> {
    let req = DefineRequest {
        label: "A",
        feature: "f1",
        faces: &["face:12"],
        feature_type: "plane",
        targets,
        replace: false,
    };
    m.define_datum(H, &pat(), &req, false)
}

fn t<'a>(id: &'a str, kind: &'a str, geometry: &'a str) -> TargetRequest<'a> {
    TargetRequest { id, kind, geometry }
}

// ------------------------------------------------------------------ AC-1 .. AC-5

#[test]
fn ac1_define_datum_a_stores_it_and_journals_one_event_with_the_full_definition() {
    let mut m = plan();
    let d = cli(&mut m, H, &AC1).unwrap();
    assert_eq!(d.branch(), Branch::Working);
    assert!(!d.dry_run());
    assert_eq!(ev(&d), exp("ac1_event"));
    assert_eq!(String::from_utf8(d.to_json_bytes().unwrap()).unwrap(), exp("ac1_result"));
    let a = m.datum(H, "A").unwrap();
    assert_eq!(a.feature_ref().as_str(), "f1");
    assert_eq!(a.nominal_faces()[0].as_str(), "face:12");
    assert_eq!(a.feature_type(), FeatureType::Plane);
    assert!(a.targets().is_empty());
    assert_eq!(m.datums(Branch::Working).count(), 1);
    // The event reads back to the same bytes.
    let back = DatumEvent::from_canonical_bytes(exp("ac1_event").as_bytes()).unwrap();
    assert_eq!(&back, d.event().unwrap());
}

#[test]
fn ac1_cli_and_mcp_give_the_same_result_and_event() {
    let mut m1 = plan();
    let mut m2 = plan();
    let c = cli(&mut m1, H, &AC1).unwrap();
    let params = br#"{"label":"A","feature":"f1","faces":["face:12"],"type":"plane"}"#;
    let p = invoke_mcp(&mut m2, H, &pat(), DEFINE_DATUM_MCP, params).unwrap();
    assert_eq!(c, p);
    assert_eq!(snapshot(&m1), snapshot(&m2));
}

#[test]
fn ac2_a_second_label_a_without_replace_is_datum_label_exists_and_journals_nothing() {
    let mut m = plan();
    cli(&mut m, H, &AC1).unwrap();
    let before = snapshot(&m);
    assert_eq!(err(cli(&mut m, H, &AC1)), "datum_label_exists{A}");
    // Also with another feature: the label alone decides.
    assert_eq!(err(define(&mut m, H, "A", "f2", &["face:13"], "plane")), "datum_label_exists{A}");
    assert_eq!(snapshot(&m), before);
}

#[test]
fn ac3_replace_records_old_and_new_definitions() {
    let mut m = plan();
    cli(&mut m, H, &AC1).unwrap();
    let d = cli(&mut m, H, &AC3).unwrap();
    assert_eq!(ev(&d), exp("ac3_event"));
    let e = d.event().unwrap();
    assert!(e.replace());
    assert!(e.old().unwrap().targets().is_empty());
    assert_eq!(e.new_definition().targets().len(), 3);
    assert_eq!(String::from_utf8(snapshot(&m)).unwrap(), exp("plan_after_ac3"));
    // Targets are held ascending by ID whatever order they were given in.
    let ids: Vec<&str> = m.datum(H, "A").unwrap().targets().iter().map(|t| t.id()).collect();
    assert_eq!(ids, ["A1", "A2", "A3"]);
}

#[test]
fn replace_of_an_undeclared_label_defines_it_with_no_old() {
    let mut m = plan();
    let mut args = AC1.to_vec();
    args.push("--replace");
    let d = cli(&mut m, H, &args).unwrap();
    assert!(d.event().unwrap().replace());
    assert!(d.old().is_none());
}

#[test]
fn ac4_a_missing_face_or_feature_is_reference_missing_naming_the_id() {
    let mut m = plan();
    let before = snapshot(&m);
    let args = ["--label", "A", "--feature", "f1", "--faces", "face:999", "--type", "plane"];
    assert_eq!(err(cli(&mut m, H, &args)), "reference_missing{face:999}");
    let e = cli(&mut m, H, &args).unwrap_err();
    assert_eq!(e, DatumError::ReferenceMissing { id: "face:999".into() });
    assert_eq!(err(define(&mut m, H, "A", "f9", &["face:12"], "plane")), "reference_missing{f9}");
    // A second, missing face after a valid one is named too.
    assert_eq!(err(define(&mut m, H, "A", "f1", &["face:12", "face:998"], "plane")), "reference_missing{face:998}");
    // Exact IDs only: a case variant of an existing ID is missing.
    assert_eq!(err(define(&mut m, H, "A", "F1", &["face:12"], "plane")), "reference_missing{F1}");
    assert_eq!(snapshot(&m), before);
}

#[test]
fn ac5_exact_replay_of_ac1_and_ac3_is_identical() {
    let start = plan();
    let mut m = start.clone();
    let e1 = cli(&mut m, H, &AC1).unwrap().event().unwrap().canonical_bytes().unwrap();
    let e3 = cli(&mut m, H, &AC3).unwrap().event().unwrap().canonical_bytes().unwrap();
    assert_eq!(e1, exp("ac1_event").into_bytes());
    assert_eq!(e3, exp("ac3_event").into_bytes());
    let r = replay(&start, &[e1.clone(), e3.clone()]);
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps, [ReplayOutcome::Identical, ReplayOutcome::Identical]);
    assert_eq!(snapshot(&r.model), snapshot(&m));

    // AC-3 alone from the fixture: the recorded old definition does not exist, so it diverges.
    assert_eq!(replay(&start, std::slice::from_ref(&e3)).outcome, ReplayOutcome::Divergent);
    // AC-1 twice: the second now fails (label exists).
    let r = replay(&start, &[e1.clone(), e1.clone()]);
    assert_eq!(r.steps, [ReplayOutcome::Identical, ReplayOutcome::Divergent]);
    // Unreadable bytes.
    assert_eq!(replay(&start, &[b"{}".to_vec()]).outcome, ReplayOutcome::NotReplayable);
    // Another plan.
    let other = String::from_utf8(e1.clone()).unwrap().replace("block-1", "block-2");
    assert_eq!(replay(&start, &[other.into_bytes()]).outcome, ReplayOutcome::Divergent);
    // The agent event replays onto the agent branch.
    let r = replay(&start, &[exp("agent_event").into_bytes()]);
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert!(r.model.datum(H, "B").is_none());
    assert!(r.model.datum(A, "B").is_some());
}

// ------------------------------------------------------------------ REQ-003, feature type

#[test]
fn req003_types_that_are_unknown_or_not_datum_capable_are_refused() {
    let mut m = plan();
    for ty in ["cone", "sphere", "point", "line", "circle", "torus", "width", "surface", "Plane", "", "planar"] {
        let e = define(&mut m, H, "A", "f1", &["face:12"], ty).unwrap_err();
        assert_eq!(e.code(), "unsupported_datum_feature_type", "{ty}");
    }
    assert!(FeatureType::Plane.is_datum_capable());
    assert!(FeatureType::Cylinder.is_datum_capable());
    assert!(!FeatureType::Sphere.is_datum_capable());
}

#[test]
fn a_type_inconsistent_with_the_bound_feature_is_feature_type_mismatch() {
    let mut m = plan();
    m.add_face("face:20").unwrap();
    m.add_feature("bore1", "cylinder", &["face:20"]).unwrap();
    assert_eq!(err(define(&mut m, H, "A", "f1", &["face:12"], "cylinder")), "feature_type_mismatch{f1}");
    assert_eq!(err(define(&mut m, H, "A", "bore1", &["face:20"], "plane")), "feature_type_mismatch{bore1}");
    // A non-capable measured feature can never be a datum feature.
    m.add_face("face:21").unwrap();
    m.add_feature("ball", "sphere", &["face:21"]).unwrap();
    assert_eq!(err(define(&mut m, H, "A", "ball", &["face:21"], "sphere")), "unsupported_datum_feature_type{sphere}");
    assert_eq!(err(define(&mut m, H, "A", "ball", &["face:21"], "plane")), "feature_type_mismatch{ball}");
    let d = define(&mut m, H, "B", "bore1", &["face:20"], "cylinder").unwrap();
    assert_eq!(d.definition().feature_type(), FeatureType::Cylinder);
}

#[test]
fn faces_must_belong_to_the_feature_and_be_given_once() {
    let mut m = plan();
    assert_eq!(err(define(&mut m, H, "A", "f1", &["face:13"], "plane")), "face_not_on_feature{face:13}");
    assert_eq!(err(define(&mut m, H, "A", "f1", &[], "plane")), "invalid_field{faces:none}");
    assert_eq!(err(define(&mut m, H, "A", "f1", &["face:12", "face:12"], "plane")), "invalid_field{faces:duplicate}");
    assert_eq!(err(define(&mut m, H, "A", "f1", &["face 12"], "plane")), "invalid_field{faces:bad_id}");
    assert_eq!(err(define(&mut m, H, "A", "", &["face:12"], "plane")), "invalid_field{feature:bad_id}");
    let many: Vec<String> = (0..257).map(|i| format!("face:{i}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(err(define(&mut m, H, "A", "f1", &many, "plane")), "invalid_field{faces:too_many}");
}

// ------------------------------------------------------------------ datum targets

#[test]
fn datum_targets_are_validated_against_the_feature_and_its_faces() {
    let mut m = plan();
    let before = snapshot(&m);
    let cases: [(&[TargetRequest<'_>], &str); 12] = [
        (&[t("A1", "line", "pt:1")], "datum_target_invalid{A1:kind_mismatch}"),
        (&[t("A1", "point", "pt:3")], "datum_target_invalid{A1:not_on_datum_faces}"),
        (&[t("B1", "point", "pt:1")], "datum_target_invalid{B1:wrong_label}"),
        (&[t("A1", "point", "pt:1"), t("A1", "point", "pt:2")], "datum_target_invalid{A1:duplicate_id}"),
        (&[t("A1", "point", "pt:1"), t("A2", "point", "pt:1")], "datum_target_invalid{A2:duplicate_geometry}"),
        (&[t("A0", "point", "pt:1")], "datum_target_invalid{A0:bad_id}"),
        (&[t("A01", "point", "pt:1")], "datum_target_invalid{A01:bad_id}"),
        (&[t("A100", "point", "pt:1")], "datum_target_invalid{A100:bad_id}"),
        (&[t("A", "point", "pt:1")], "datum_target_invalid{A:bad_id}"),
        (&[t("A1", "circle", "pt:1")], "datum_target_invalid{A1:bad_kind}"),
        (&[t("A1", "point", "pt:9")], "reference_missing{pt:9}"),
        (&[t("A1", "point", "")], "invalid_field{targets:bad_geometry_id}"),
    ];
    for (targets, want) in cases {
        assert_eq!(err(with_targets(&mut m, targets)), want);
    }
    assert_eq!(snapshot(&m), before);
    let d = with_targets(&mut m, &[t("A12", "line", "ln:1"), t("A9", "point", "pt:1")]).unwrap();
    let kinds: Vec<TargetKind> = d.definition().targets().iter().map(|t| t.kind()).collect();
    assert_eq!(kinds, [TargetKind::Line, TargetKind::Point]);
    let too_many: Vec<String> = (1..=65).map(|i| format!("A{i}")).collect();
    let reqs: Vec<TargetRequest<'_>> = too_many.iter().map(|id| t(id, "point", "pt:1")).collect();
    assert_eq!(err(with_targets(&mut m, &reqs)), "invalid_field{targets:too_many}");
}

#[test]
fn target_kinds_allowed_per_feature_type() {
    for k in [TargetKind::Point, TargetKind::Line, TargetKind::Area] {
        assert!(k.allowed_on(FeatureType::Plane));
        assert!(k.allowed_on(FeatureType::Cylinder));
        assert!(!k.allowed_on(FeatureType::Point));
    }
    assert!(!TargetKind::Line.allowed_on(FeatureType::Sphere));
    assert!(TargetKind::Point.allowed_on(FeatureType::Sphere));
}

// ------------------------------------------------------------------ labels

#[test]
fn labels_are_one_to_three_ascii_capitals_matching_the_pattern() {
    let mut m = plan();
    let cases = [
        ("", "bad_label{empty}"),
        ("AAAA", "bad_label{too_long}"),
        ("a", "bad_label{not_ascii_capital}"),
        ("A1", "bad_label{not_ascii_capital}"),
        ("\u{FF21}", "bad_label{not_ascii_capital}"),  // full-width A
        ("\u{0410}", "bad_label{not_ascii_capital}"),  // Cyrillic capital A
        ("\u{0391}", "bad_label{not_ascii_capital}"),  // Greek capital alpha
        ("A\u{0301}", "bad_label{not_ascii_capital}"), // A + combining acute
        ("\u{FF21}\u{FF21}\u{FF21}\u{FF21}", "bad_label{too_long}"),
    ];
    for (label, want) in cases {
        assert_eq!(err(define(&mut m, H, label, "f1", &["face:12"], "plane")), want, "{label:?}");
    }
    // A pattern narrower than the base rule.
    let p = LabelPattern::parse("[A-H]{1,2}").unwrap();
    assert_eq!(p.label("J").unwrap_err().to_string(), "bad_label{pattern}");
    assert_eq!(p.label("ABC").unwrap_err().to_string(), "bad_label{pattern}");
    assert_eq!(p.label("HA").unwrap().as_str(), "HA");
    let req = DefineRequest {
        label: "J",
        feature: "f1",
        faces: &["face:12"],
        feature_type: "plane",
        targets: &[],
        replace: false,
    };
    assert_eq!(m.define_datum(H, &p, &req, false).unwrap_err().to_string(), "bad_label{pattern}");
    assert!(DatumLabel::parse("ABC").is_ok());
}

#[test]
fn look_alike_and_case_variants_never_reach_the_uniqueness_check() {
    let mut m = plan();
    define(&mut m, H, "A", "f1", &["face:12"], "plane").unwrap();
    // Variants of an existing label are refused as bad labels, not accepted as new datums.
    for l in ["a", "\u{FF21}", "\u{0410}"] {
        assert_eq!(define(&mut m, H, l, "f2", &["face:13"], "plane").unwrap_err().code(), "bad_label");
    }
    assert_eq!(m.datums(Branch::Working).count(), 1);
}

#[test]
fn label_pattern_grammar() {
    for ok in ["[A-Z]{1,3}", "[A-HJ-NPR-Z]{1,2}", "[AB]{1}", "[A-A]{3,3}", "[ZA-C]{2,3}"] {
        assert_eq!(LabelPattern::parse(ok).unwrap().as_str(), ok);
    }
    let bad = [
        ("", "no_class"),
        ("A-Z{1,3}", "no_class"),
        ("[]{1,3}", "no_class"),
        ("[a-z]{1,3}", "bad_class"),
        ("[A-]{1,3}", "bad_range"),
        ("[Z-A]{1,3}", "bad_range"),
        ("[A-Z]", "bad_bounds"),
        ("[A-Z]{0,3}", "bad_bounds"),
        ("[A-Z]{1,4}", "bad_bounds"),
        ("[A-Z]{3,1}", "bad_bounds"),
        ("[A-Z]{1,3}x", "bad_bounds"),
        ("[A-Z]{1, 3}", "bad_bounds"),
        ("[\u{FF21}]{1}", "bad_class"),
    ];
    for (p, reason) in bad {
        assert_eq!(LabelPattern::parse(p).unwrap_err(), DatumError::LabelPatternInvalid { reason }, "{p}");
    }
    let long = format!("[{}]{{1}}", "A".repeat(70));
    assert_eq!(LabelPattern::parse(&long).unwrap_err().reason(), Some("too_long"));
}

// ------------------------------------------------------------------ active defaults profile

fn vocab() -> KeyVocabulary {
    KeyVocabulary::new(["gdt.datum.label_pattern", "placeholder.synthetic"]).unwrap()
}

fn spec_for(profile: &str) -> GoverningSpec {
    let std = StdRef::parse("std:asme:y14.5:2018").unwrap();
    let r = fid_gdt::chars::dprof::ProfileRef::parse(profile).unwrap();
    GoverningSpec::new(Dialect::Asme, Standards::new(vec![std]).unwrap(), r, InheritedFrom::Explicit).unwrap()
}

#[test]
fn the_label_pattern_comes_from_the_plans_active_defaults_profile() {
    let mut dp = DprofState::default();
    let doc = parse_profile(&read("datum_profile.json"), &vocab()).unwrap();
    let r = dp.publish(doc, H, "root://imports/datum_profile.json", false).unwrap().reference().clone();
    let mut st = SpecState::new();
    assert_eq!(
        active_label_pattern(&st, &dp, H, "block-1").unwrap_err().to_string(),
        "label_pattern_unavailable{no_governing_spec}"
    );
    st.set_plan_default(&dp, H, "block-1", Some(spec_for(&r.to_string()))).unwrap();
    let p = active_label_pattern(&st, &dp, H, "block-1").unwrap();
    assert_eq!(p.as_str(), "[A-H]{1,2}");
    assert_eq!(active_label_pattern(&st, &dp, A, "block-1").unwrap(), p);
    assert_eq!(
        active_label_pattern(&st, &dp, H, "bad plan").unwrap_err().to_string(),
        "label_pattern_unavailable{bad_plan_id}"
    );
    // The tenant default applies to plans without their own default.
    st.set_tenant_default(&dp, H, Some(spec_for(&r.to_string()))).unwrap();
    assert_eq!(active_label_pattern(&st, &dp, H, "plan-2").unwrap(), p);

    // A profile without the key fails closed.
    let doc = br#"{"schema":"fid.defaults-profile/1","dialect":"asme","edition":"y14.5-2018","version":"1.1.0","keys":{"placeholder.synthetic":true}}"#;
    let r2 = dp.publish(parse_profile(doc, &vocab()).unwrap(), H, "root://imports/x.json", false).unwrap();
    let mut st2 = SpecState::new();
    st2.set_plan_default(&dp, H, "block-1", Some(spec_for(&r2.reference().to_string()))).unwrap();
    assert_eq!(
        active_label_pattern(&st2, &dp, H, "block-1").unwrap_err().to_string(),
        "label_pattern_unavailable{key_missing}"
    );
    // A malformed or non-text value fails closed.
    for (v, want) in [(r#""[a]{1}""#, "label_pattern_invalid{bad_class}"), ("true", "label_pattern_invalid{not_text}")]
    {
        let doc = format!(
            r#"{{"schema":"fid.defaults-profile/1","dialect":"asme","edition":"y14.5-2018","version":"1.2.{}","keys":{{"gdt.datum.label_pattern":{v}}}}}"#,
            v.len()
        );
        let r3 =
            dp.publish(parse_profile(doc.as_bytes(), &vocab()).unwrap(), H, "root://imports/y.json", false).unwrap();
        let mut st3 = SpecState::new();
        st3.set_plan_default(&dp, H, "block-1", Some(spec_for(&r3.reference().to_string()))).unwrap();
        assert_eq!(active_label_pattern(&st3, &dp, H, "block-1").unwrap_err().to_string(), want);
    }
}

#[test]
fn an_agent_published_profile_is_not_active_for_a_person() {
    let mut dp = DprofState::default();
    let doc = parse_profile(&read("datum_profile.json"), &vocab()).unwrap();
    let r = dp.publish(doc, A, "root://imports/datum_profile.json", false).unwrap().reference().clone();
    // A person cannot even make it a plan default, so it never becomes their active profile.
    let mut st = SpecState::new();
    assert!(st.set_plan_default(&dp, H, "block-1", Some(spec_for(&r.to_string()))).is_err());
    assert_eq!(
        active_label_pattern(&st, &dp, H, "block-1").unwrap_err().to_string(),
        "label_pattern_unavailable{no_governing_spec}"
    );
    assert!(dp.resolve(Branch::Working, &r).is_err());
    assert!(dp.resolve(Branch::Agent, &r).is_ok());
}

// ------------------------------------------------------------------ branches

#[test]
fn agent_work_never_reaches_or_blocks_the_persons_branch() {
    let mut m = plan();
    let b = cli(
        &mut m,
        A,
        &["--label", "B", "--feature", "f2", "--faces", "face:13", "--type", "plane", "--target", "B1,point,pt:3"],
    )
    .unwrap();
    assert_eq!(b.branch(), Branch::Agent);
    assert_eq!(ev(&b), exp("agent_event"));
    assert!(m.datum(H, "B").is_none());
    assert_eq!(m.datums(Branch::Working).count(), 0);
    // The person can still declare B on the working branch, and sees their own.
    let hb = define(&mut m, H, "B", "f3", &["face:14"], "plane").unwrap();
    assert_eq!(hb.branch(), Branch::Working);
    assert_eq!(m.datum(H, "B").unwrap().feature_ref().as_str(), "f3");
    // The agent sees its overlay first.
    assert_eq!(m.datum(A, "B").unwrap().feature_ref().as_str(), "f2");

    // The agent sees the person's A: no silent duplicate.
    cli(&mut m, H, &AC1).unwrap();
    assert_eq!(err(define(&mut m, A, "A", "f2", &["face:13"], "plane")), "datum_label_exists{A}");
    // An agent replace shadows A in the overlay; the working branch is unchanged.
    let working_before: Vec<_> = m.datums(Branch::Working).cloned().collect();
    let req = DefineRequest {
        label: "A",
        feature: "f2",
        faces: &["face:13"],
        feature_type: "plane",
        targets: &[],
        replace: true,
    };
    let d = m.define_datum(A, &pat(), &req, false).unwrap();
    assert_eq!(d.branch(), Branch::Agent);
    assert_eq!(d.old().unwrap().feature_ref().as_str(), "f1");
    assert_eq!(m.datum(H, "A").unwrap().feature_ref().as_str(), "f1");
    assert_eq!(m.datum(A, "A").unwrap().feature_ref().as_str(), "f2");
    let working_after: Vec<_> = m.datums(Branch::Working).cloned().collect();
    assert_eq!(working_before, working_after);
    // Scripts are agents for branch purposes.
    let s = define(&mut m, Principal::Script, "C", "f3", &["face:14"], "plane").unwrap();
    assert_eq!(s.branch(), Branch::Agent);
    assert_eq!(s.event().unwrap().branch(), Branch::Agent);
}

#[test]
fn dry_runs_change_nothing_and_journal_nothing() {
    let mut m = plan();
    let before = snapshot(&m);
    let mut args = AC1.to_vec();
    args.push("--dry-run");
    let d = cli(&mut m, H, &args).unwrap();
    assert!(d.dry_run());
    assert!(d.event().is_none());
    assert_eq!(snapshot(&m), before);
    // Dry runs still apply every check.
    cli(&mut m, H, &AC1).unwrap();
    assert_eq!(err(cli(&mut m, H, &args)), "datum_label_exists{A}");
}

// ------------------------------------------------------------------ events

#[test]
fn the_event_reader_is_strict() {
    let good = exp("ac3_event");
    assert!(DatumEvent::from_canonical_bytes(good.as_bytes()).is_ok());
    let reason = |s: &str| DatumEvent::from_canonical_bytes(s.as_bytes()).unwrap_err().reason().unwrap();
    assert_eq!(reason(&good.replace(r#""branch":"working""#, r#""branch":"agent""#)), "branch_principal_mismatch");
    assert_eq!(reason(&good.replace(r#""replace":true"#, r#""replace":false"#)), "bad_shape");
    assert_eq!(reason(&good.replace("gdt.define_datum@1", "gdt.define_datum@2")), "unknown_command");
    assert_eq!(
        reason(&good.replace(r#""label_pattern":"[A-Z]{1,3}""#, r#""label_pattern":"[B-Z]{1,3}""#)),
        "label_not_in_pattern"
    );
    assert_eq!(reason(&good.replace(r#""label_pattern":"[A-Z]{1,3}""#, r#""label_pattern":"A""#)), "bad_label_pattern");
    assert_eq!(reason(&good.replace(r#""replace":true"#, r#""replace":true,"x":1"#)), "bad_keys");
    assert_eq!(reason(&good.replace(r#""replace":true"#, r#""replace":"yes""#)), "wrong_type");
    assert_eq!(reason(&good.replace(r#""principal":"human""#, r#""principal":"root""#)), "bad_principal");
    // Non-canonical target order and a non-capable type in a definition.
    let swapped = exp("ac3_event").replace(
        r#"{"geometry":"pt:1","id":"A1","kind":"point"},{"geometry":"pt:2","id":"A2","kind":"point"}"#,
        r#"{"geometry":"pt:2","id":"A2","kind":"point"},{"geometry":"pt:1","id":"A1","kind":"point"}"#,
    );
    assert_eq!(reason(&swapped), "not_canonical");
    assert_eq!(
        reason(&exp("ac1_event").replace(r#""feature_type":"plane""#, r#""feature_type":"cone""#)),
        "bad_definition"
    );
    // Old and new must share the label.
    let relabel = good.replacen(r#""label":"A","targets":[]"#, r#""label":"B","targets":[]"#, 1);
    assert_eq!(reason(&relabel), "bad_shape");
    assert_eq!(reason(&format!("{good}{}", " ".repeat(70_000))), "too_large");
    let b = good.as_bytes();
    for i in 0..b.len() {
        let _ = DatumEvent::from_canonical_bytes(&b[..i]);
        let mut f = b.to_vec();
        f[i] ^= 0x20;
        let _ = DatumEvent::from_canonical_bytes(&f);
    }
}

// ------------------------------------------------------------------ project file

#[test]
fn the_project_writer_matches_the_python_canonical_form_and_round_trips() {
    let m = plan();
    assert_eq!(String::from_utf8(snapshot(&m)).unwrap(), exp("plan_canonical"));
    assert_eq!(load_project(&snapshot(&m)).unwrap(), m);
    let mut m = plan();
    cli(&mut m, H, &AC1).unwrap();
    cli(&mut m, A, &["--label", "B", "--feature", "f2", "--faces", "face:13", "--type", "plane"]).unwrap();
    assert_eq!(load_project(&snapshot(&m)).unwrap(), m);
}

#[test]
fn the_project_loader_is_strict() {
    let good = exp("plan_canonical");
    let reason = |s: &str| load_project(s.as_bytes()).unwrap_err().to_string();
    assert_eq!(
        reason(&good.replace(r#""plan":"block-1""#, r#""plan":"block-1","plan":"x""#)),
        "project_invalid{duplicate_key}"
    );
    assert_eq!(reason(&good.replace(r#""plan":"block-1""#, r#""plan":1"#)), "project_invalid{bad_number}");
    assert_eq!(reason(&good.replace("fid.gdt-datum-plan/1", "fid.gdt-datum-plan/2")), "project_invalid{bad_schema}");
    assert_eq!(reason(&good.replace(r#""face:14"]"#, r#""face:14","FACE:12"]"#)), "invalid_field{face:case_duplicate}");
    assert_eq!(reason(&good.replace(r#""face:14"]"#, r#""face:14","face:12"]"#)), "invalid_field{face:duplicate}");
    assert_eq!(
        reason(
            &good
                .replace(r#""f3":{"faces":["face:14"],"type":"plane"}"#, r#""f3":{"faces":["face:14"],"type":"blob"}"#)
        ),
        "invalid_field{feature_type:unknown}"
    );
    assert_eq!(
        reason(&good.replace(r#""f3":{"faces":["face:14"]"#, r#""f3":{"faces":["face:99"]"#)),
        "reference_missing{face:99}"
    );
    assert_eq!(reason(&good.replace(r#""f3":{"#, r#""face:12":{"#)), "invalid_field{feature:duplicate}");
    assert_eq!(reason(&good.replace(r#""kind":"area""#, r#""kind":"blob""#)), "invalid_field{kind:unknown}");
    let def = r#"{"faces":["face:12"],"feature":"f1","feature_type":"plane","label":"A","targets":[]}"#;
    let with = |key: &str, d: &str| good.replace(r#""working":{}"#, &format!(r#""working":{{"{key}":{d}}}"#));
    assert!(load_project(with("A", def).as_bytes()).is_ok());
    assert_eq!(reason(&with("B", def)), "project_invalid{label_key_mismatch}");
    assert_eq!(reason(&with("A", &def.replace("face:12", "face:13"))), "face_not_on_feature{face:13}");
    assert_eq!(reason(&with("A", &def.replace(r#""f1""#, r#""f7""#))), "reference_missing{f7}");
    assert_eq!(
        reason(&with("A", &def.replace(r#""targets":[]"#, r#""targets":[],"x":"y""#))),
        "project_invalid{bad_keys}"
    );
    let b = good.as_bytes();
    for i in 0..b.len() {
        let _ = load_project(&b[..i]);
        let mut f = b.to_vec();
        f[i] ^= 0x01;
        let _ = load_project(&f);
    }
}

#[test]
fn model_builder_ids_share_one_case_insensitive_namespace() {
    let mut m = DatumModel::new("p").unwrap();
    assert_eq!(DatumModel::new("").unwrap_err().to_string(), "invalid_field{plan:bad_id}");
    m.add_face("face:1").unwrap();
    assert_eq!(
        m.add_feature("Face:1", "plane", &["face:1"]).unwrap_err().to_string(),
        "invalid_field{feature:case_duplicate}"
    );
    assert_eq!(
        m.add_feature("f", "plane", &["face:1", "face:1"]).unwrap_err().to_string(),
        "invalid_field{faces:duplicate}"
    );
    assert_eq!(m.add_geometry("g", "point", "face:2").unwrap_err().to_string(), "reference_missing{face:2}");
    m.add_feature("f", "plane", &["face:1"]).unwrap();
    assert_eq!(
        m.add_geometry("F", "point", "face:1").unwrap_err().to_string(),
        "invalid_field{geometry:case_duplicate}"
    );
}

// ------------------------------------------------------------------ surfaces and registry

#[test]
fn cli_and_mcp_refuse_malformed_calls() {
    let mut m = plan();
    let p = pat();
    let mut c = |args: &[&str]| err(invoke_cli(&mut m, H, &p, args));
    assert_eq!(c(&[]), "bad_params{missing subcommand}");
    assert_eq!(c(&["define-datums"]), "bad_params{unknown subcommand}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "--label", "A"]), "bad_params{missing parameter}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "--label"]), "bad_params{option needs a value}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "--label", "A", "--label", "B"]), "bad_params{repeated option}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "--replace", "--replace"]), "bad_params{repeated option}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "--json", "--json"]), "bad_params{repeated option}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "--x", "1"]), "bad_params{unknown option}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "stray"]), "bad_params{unexpected argument}");
    assert_eq!(c(&[DEFINE_DATUM_CLI, "--target", "A1,point"]), "bad_params{target is not <id>,<kind>,<geometry>}");
    assert_eq!(c(&["--json"; 200]), "bad_params{too many arguments}");

    let mut j = |raw: &[u8]| err(invoke_mcp(&mut m, H, &p, DEFINE_DATUM_MCP, raw));
    assert_eq!(j(b"[]"), "bad_params{params is not an object}");
    assert_eq!(j(br#"{"label":"A","label":"B"}"#), "bad_params{duplicate key}");
    assert_eq!(
        j(br#"{"label":"A","feature":"f1","faces":["face:12"],"type":"plane","x":1}"#),
        "bad_params{unknown parameter}"
    );
    assert_eq!(
        j(br#"{"label":1,"feature":"f1","faces":["face:12"],"type":"plane"}"#),
        "bad_params{parameter is not a string}"
    );
    assert_eq!(
        j(br#"{"label":"A","feature":"f1","faces":"face:12","type":"plane"}"#),
        "bad_params{faces is not a string array}"
    );
    assert_eq!(
        j(br#"{"label":"A","feature":"f1","faces":["face:12"],"type":"plane","replace":"yes"}"#),
        "bad_params{parameter is not a boolean}"
    );
    assert_eq!(
        j(br#"{"label":"A","feature":"f1","faces":["face:12"],"type":"plane","targets":[{"id":"A1"}]}"#),
        "bad_params{target is not {id, kind, geometry}}"
    );
    assert_eq!(j(br#"{"a":{"b":{"c":{}}}}"#), "bad_params{params too large or too deep}");
    assert_eq!(err(invoke_mcp(&mut m, H, &p, "gdt_define_datums", b"{}")), "bad_params{unknown tool}");
    // A valid MCP call with targets and dry_run.
    let d = invoke_mcp(
        &mut m,
        H,
        &p,
        DEFINE_DATUM_MCP,
        br#"{"label":"A","feature":"f1","faces":["face:12"],"type":"plane","targets":[{"id":"A1","kind":"point","geometry":"pt:1"}],"dry_run":true}"#,
    )
    .unwrap();
    assert!(d.dry_run());
    assert_eq!(d.definition().targets().len(), 1);
    assert!(m.datum(H, "A").is_none());
}

#[test]
fn command_definition_is_w2_d0_on_cli_and_mcp() {
    let d = command_def().unwrap();
    assert_eq!(format!("{}@{}", d.id, d.major), DEFINE_COMMAND);
    assert_eq!(d.side_effect.as_str(), "W2");
    assert_eq!(d.determinism.as_str(), "D0");
    assert_eq!(d.effects, [DEFINE_COMMAND]);
}
