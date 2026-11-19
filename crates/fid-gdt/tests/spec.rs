//! Acceptance criteria of spec node GDT.CHAR.SPEC (AC-1 .. AC-7) plus the error paths, branch
//! rules and limits pinned in the leaf's Design. Project fixtures, profile references and the
//! expected event and canonical bytes come from the independent stdlib-Python reference
//! `fixtures/gdt/spec/make_fixtures.py` (`expected.json`); error cases are hand-labelled from
//! the leaf's rules. All profile content is synthetic placeholder data.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{parse_profile, Branch, CharId, DprofEvent, DprofState, KeyVocabulary, ProfileRef};
use fid_gdt::chars::spec::types::{dialect_wire, parse_dialect_wire, MAX_STANDARDS};
use fid_gdt::chars::spec::{
    load_project, project_bytes, replay, GoverningSpec, InheritedFrom, PlanId, SetRequest, SpecError, SpecEvent,
    SpecState, Standards, StdRef,
};
use fid_gdt::dmt_flow::roster::Dialect;
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use std::path::{Path, PathBuf};

const H: Principal = Principal::Human;
const A: Principal = Principal::Agent;
const ASME_STD: &str = "std:asme:y14.5:2018";
const ISO_STD: &str = "std:iso:1101:2017";

fn dir(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt").join(sub)
}

fn read(sub: &str, name: &str) -> Vec<u8> {
    std::fs::read(dir(sub).join(name)).unwrap()
}

fn text(name: &str) -> String {
    String::from_utf8(read("spec", name)).unwrap()
}

fn vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof", "vocabulary.json")).unwrap();
    let CValue::Obj(o) = v else { panic!("vocabulary") };
    let Some(CValue::Arr(keys)) = o.get("keys") else { panic!("keys") };
    let keys: Vec<&str> = keys
        .iter()
        .map(|k| match k {
            CValue::Str(s) => s.as_str(),
            _ => panic!("key"),
        })
        .collect();
    KeyVocabulary::new(keys).unwrap()
}

/// A string field of `expected.json` (from the Python reference).
fn exp(path: &[&str]) -> String {
    let mut v = fid_canon::parse(&read("spec", "expected.json")).unwrap();
    for k in path {
        let CValue::Obj(mut o) = v else { panic!("{k}") };
        v = o.remove(*k).unwrap();
    }
    let CValue::Str(s) = v else { panic!("string") };
    s
}

fn asme_ref() -> String {
    exp(&["refs", "asme_1.0.0"])
}
fn asme_mod_ref() -> String {
    exp(&["refs", "asme_1.0.0_modified"])
}
fn iso_ref() -> String {
    exp(&["refs", "iso_1.0.0"])
}

fn load(name: &str) -> (SpecState, DprofState) {
    load_project(&read("spec", name), &vocab()).unwrap()
}

fn load_text(t: &str) -> Result<(SpecState, DprofState), SpecError> {
    load_project(t.as_bytes(), &vocab())
}

fn rf(s: &str) -> ProfileRef {
    ProfileRef::parse(s).unwrap()
}

fn cid(s: &str) -> CharId {
    CharId::parse(s).unwrap()
}

fn spec_of(dialect: Dialect, stds: &[&str], profile: &str, from: InheritedFrom) -> GoverningSpec {
    let refs = stds.iter().map(|s| StdRef::parse(s).unwrap()).collect();
    GoverningSpec::new(dialect, Standards::new(refs).unwrap(), rf(profile), from).unwrap()
}

fn set(
    st: &mut SpecState,
    dp: &mut DprofState,
    p: Principal,
    c: &str,
    dialect: &str,
    stds: &[&str],
    profile: &str,
) -> Result<fid_gdt::chars::spec::SetSpec, SpecError> {
    let req = SetRequest { char: c, dialect, standards: stds, defaults_profile: profile };
    st.set_governing_spec(dp, p, &req, false)
}

fn bytes(e: &SpecEvent) -> String {
    String::from_utf8(e.canonical_bytes().unwrap()).unwrap()
}

/// Publishes a profile document (text) on `p`'s branch.
fn publish_doc(dp: &mut DprofState, p: Principal, doc: &[u8]) -> ProfileRef {
    let doc = parse_profile(doc, &vocab()).unwrap();
    dp.publish(doc, p, "root://imports/test.json", false).unwrap().reference().clone()
}

/// AC-1 state: plan_asme.fidproj with `flat-1` created by a human in `plan-1`.
fn ac1() -> (SpecState, DprofState, Vec<Vec<u8>>) {
    let (mut st, mut dp) = load("plan_asme.fidproj");
    let c = st.create_characteristic(&mut dp, H, "plan-1", "flat-1", false).unwrap();
    let journal = vec![c.event().unwrap().canonical_bytes().unwrap()];
    (st, dp, journal)
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_create_copies_the_plan_default_and_journals_inherited_from_plan() {
    let (mut st, mut dp) = load("plan_asme.fidproj");
    let plan_default = st.plan_default(&PlanId::parse("plan-1").unwrap()).unwrap().clone();
    let c = st.create_characteristic(&mut dp, H, "plan-1", "flat-1", false).unwrap();
    let spec = c.spec().unwrap();
    assert!(spec.same_rules(&plan_default));
    assert_eq!(spec.inherited_from(), InheritedFrom::Plan);
    assert_eq!(spec.defaults_profile().to_string(), asme_ref());
    assert_eq!(spec.standards()[0].to_string(), ASME_STD);
    assert_eq!(c.branch(), Branch::Working);
    let e = c.event().unwrap();
    assert_eq!(e.inherited_from(), InheritedFrom::Plan);
    assert_eq!(bytes(e), exp(&["ac1_event"]));
    // The profile store binds the new characteristic (the DPROF `bind` hook) and the gate passes.
    assert_eq!(dp.binding(Branch::Working, &cid("flat-1")).unwrap().to_string(), asme_ref());
    assert!(st.require_governing_spec(H, "flat-1", &dp).unwrap().same_rules(&plan_default));
    // --json result.
    let json = String::from_utf8(fid_canon::to_canonical_bytes(&c.to_cvalue()).unwrap()).unwrap();
    assert!(json.starts_with(r#"{"branch":"working","char":"flat-1","dry_run":false,"governing_spec":{"#));
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_explicit_set_replaces_the_spec_and_journals_old_and_new() {
    let (mut st, mut dp, mut journal) = ac1();
    let r = set(&mut st, &mut dp, H, "flat-1", "ISO", &[ISO_STD], &iso_ref()).unwrap();
    journal.push(r.event().unwrap().canonical_bytes().unwrap());
    assert_eq!(journal.len(), 2);
    assert_eq!(bytes(r.event().unwrap()), exp(&["ac2_event"]));
    assert_eq!(r.old().unwrap().dialect(), Dialect::Asme);
    assert_eq!(r.new_spec().inherited_from(), InheritedFrom::Explicit);
    let now = st.require_governing_spec(H, "flat-1", &dp).unwrap();
    assert_eq!(now.dialect(), Dialect::Iso);
    assert_eq!(now.defaults_profile().to_string(), iso_ref());
    assert_eq!(dp.binding(Branch::Working, &cid("flat-1")).unwrap().to_string(), iso_ref());
    let json = String::from_utf8(fid_canon::to_canonical_bytes(&r.to_cvalue()).unwrap()).unwrap();
    assert!(json.contains(r#""dialect":"ISO""#) && json.contains(r#""inherited_from":"explicit""#));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_asme_with_an_iso_designator_is_dialect_mismatch_and_journals_nothing() {
    let (mut st, mut dp, journal) = ac1();
    let (st0, dp0) = (st.clone(), dp.clone());
    let e = set(&mut st, &mut dp, H, "flat-1", "ASME", &[ISO_STD], &asme_ref()).unwrap_err();
    assert_eq!(e.code(), "dialect_mismatch");
    assert_eq!(e.to_string(), "dialect_mismatch{std:iso:1101:2017}");
    assert_eq!(journal.len(), 1);
    assert_eq!((st, dp), (st0, dp0));
}

#[test]
fn a_profile_of_the_other_dialect_is_dialect_mismatch() {
    let (mut st, mut dp, _) = ac1();
    let e = set(&mut st, &mut dp, H, "flat-1", "ASME", &[ASME_STD], &iso_ref()).unwrap_err();
    assert_eq!(e.code(), "dialect_mismatch");
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_evaluation_without_a_governing_spec_names_dialect_and_emits_no_result() {
    let (st, dp) = load("no_spec.fidproj");
    let mut results: Vec<&str> = Vec::new();
    let e = st
        .evaluate(H, "c-flat-1", &dp, |_| {
            results.push("ResultRecord");
        })
        .unwrap_err();
    assert_eq!(e, SpecError::GoverningSpecMissing { field: "dialect" });
    assert_eq!(e.to_string(), "governing_spec_missing{dialect}");
    assert!(results.is_empty());
    // The agent branch sees the same characteristic and refuses it the same way.
    assert_eq!(st.require_governing_spec(A, "c-flat-1", &dp).unwrap_err().reason(), Some("dialect"));
}

#[test]
fn incomplete_specs_name_the_first_missing_field() {
    let base = text("plan_asme.fidproj");
    let cases = [
        ("\"dialect\": \"ASME\",\n", "", "dialect"),
        ("\"standards\": [\n    \"std:asme:y14.5:2018\"\n   ],\n", "", "standards"),
        ("\",\n   \"inherited_from\": \"explicit\"", "\"", "inherited_from"),
    ];
    for (cut, with, field) in cases {
        assert_eq!(base.matches(cut).count(), 1, "{field}");
        let t = base.replace(cut, with);
        let e = load_text(&t).map(|_| ()).unwrap_err();
        assert_eq!(e, SpecError::GoverningSpecMissing { field }, "{field}");
    }
    let e = load_text(&base.replace("\"defaults_profile\"", "\"x_profile\"")).map(|_| ()).unwrap_err();
    assert_eq!(e, SpecError::ProjectInvalid { reason: "spec_unknown_field" });
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_a_digest_that_differs_from_the_stored_profile_is_refused_by_the_gate() {
    let (st, dp) = load("digest_mismatch.fidproj");
    let mut results = 0;
    let e = st.evaluate(H, "c-flat-1", &dp, |_| results += 1).unwrap_err();
    assert_eq!(e.code(), "defaults_profile_digest_mismatch");
    assert_eq!(e.to_string(), format!("defaults_profile_digest_mismatch{{{}}}", asme_mod_ref()));
    assert_eq!(results, 0);
    // The characteristic stays fixable: an explicit set with the stored profile clears it.
    let (mut st, mut dp) = (st, dp);
    set(&mut st, &mut dp, H, "c-flat-1", "ASME", &[ASME_STD], &asme_ref()).unwrap();
    assert!(st.require_governing_spec(H, "c-flat-1", &dp).is_ok());
}

#[test]
fn the_gate_checks_the_store_it_is_given() {
    let (st, _, _) = ac1();
    // A store holding the modified 1.0.0: same version, other digest.
    let mut other = DprofState::new();
    publish_doc(&mut other, H, &read("dprof", "asme_placeholder_1.0.0_modified.json"));
    let e = st.require_governing_spec(H, "flat-1", &other).unwrap_err();
    assert_eq!(e.code(), "defaults_profile_digest_mismatch");
    // A store without that version at all.
    let e = st.require_governing_spec(H, "flat-1", &DprofState::new()).unwrap_err();
    assert_eq!(e.code(), "defaults_profile_unknown");
    // A store holding the right profile but not bound to the characteristic.
    let mut unbound = DprofState::new();
    publish_doc(&mut unbound, H, &read("dprof", "asme_placeholder_1.0.0.json"));
    let e = st.require_governing_spec(H, "flat-1", &unbound).unwrap_err();
    assert_eq!(e, SpecError::DefaultsProfileBindingMismatch { char: "flat-1".into() });
}

// ------------------------------------------------------------------ AC-6, REQ-005

#[test]
fn ac6_changing_the_plan_default_leaves_existing_characteristics_unchanged() {
    let (mut st, mut dp, _) = ac1();
    let iso = spec_of(Dialect::Iso, &[ISO_STD], &iso_ref(), InheritedFrom::Explicit);
    st.set_plan_default(&dp, H, "plan-1", Some(iso.clone())).unwrap();
    let still = st.require_governing_spec(H, "flat-1", &dp).unwrap();
    assert_eq!(still.dialect(), Dialect::Asme);
    assert_eq!(still.defaults_profile().to_string(), asme_ref());
    // A characteristic created afterwards copies the new default.
    let c = st.create_characteristic(&mut dp, H, "plan-1", "flat-2", false).unwrap();
    assert!(c.spec().unwrap().same_rules(&iso));
}

#[test]
fn req005_changing_the_tenant_default_leaves_existing_characteristics_unchanged() {
    let (mut st, mut dp) = load("plan_asme.fidproj");
    let asme = spec_of(Dialect::Asme, &[ASME_STD], &asme_ref(), InheritedFrom::Explicit);
    st.set_tenant_default(&dp, H, Some(asme)).unwrap();
    let c = st.create_characteristic(&mut dp, H, "plan-2", "t-1", false).unwrap();
    assert_eq!(c.spec().unwrap().inherited_from(), InheritedFrom::Tenant);
    let e = c.event().unwrap();
    assert_eq!(e.inherited_from(), InheritedFrom::Tenant);
    assert_eq!(e.plan().unwrap().as_str(), "plan-2");
    let iso = spec_of(Dialect::Iso, &[ISO_STD], &iso_ref(), InheritedFrom::Explicit);
    st.set_tenant_default(&dp, H, Some(iso)).unwrap();
    assert_eq!(st.require_governing_spec(H, "t-1", &dp).unwrap().dialect(), Dialect::Asme);
    // The plan default wins over the tenant default.
    let c = st.create_characteristic(&mut dp, H, "plan-1", "p-1", false).unwrap();
    assert_eq!(c.spec().unwrap().inherited_from(), InheritedFrom::Plan);
}

#[test]
fn req004_no_default_means_no_governing_spec_and_no_event() {
    let (mut st, mut dp) = load("plan_asme.fidproj");
    let c = st.create_characteristic(&mut dp, H, "plan-9", "bare-1", false).unwrap();
    assert!(c.spec().is_none() && c.event().is_none());
    assert_eq!(st.governing_spec(Branch::Working, &cid("bare-1")), Some(None));
    assert_eq!(dp.binding(Branch::Working, &cid("bare-1")), None);
    let e = st.require_governing_spec(H, "bare-1", &dp).unwrap_err();
    assert_eq!(e.reason(), Some("dialect"));
    // An explicit set then makes it evaluable.
    set(&mut st, &mut dp, H, "bare-1", "ISO", &[ISO_STD], &iso_ref()).unwrap();
    assert!(st.require_governing_spec(H, "bare-1", &dp).is_ok());
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_exact_replay_of_ac1_and_ac2_is_identical() {
    let (start_st, start_dp) = load("plan_asme.fidproj");
    let (mut st, mut dp, mut journal) = ac1();
    let r = set(&mut st, &mut dp, H, "flat-1", "ISO", &[ISO_STD], &iso_ref()).unwrap();
    journal.push(r.event().unwrap().canonical_bytes().unwrap());
    let out = replay(&start_st, &start_dp, &journal, &start_dp, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Identical);
    assert_eq!(out.steps, vec![ReplayOutcome::Identical; 2]);
    assert_eq!(out.spec, st);
    assert_eq!(out.dprof, dp);
    // The recorded bytes are the reference's bytes.
    assert_eq!(String::from_utf8(journal[0].clone()).unwrap(), exp(&["ac1_event"]));
    assert_eq!(String::from_utf8(journal[1].clone()).unwrap(), exp(&["ac2_event"]));
}

#[test]
fn replay_detects_divergence_and_unreadable_events() {
    let (mut start_st, start_dp) = load("plan_asme.fidproj");
    let journal = vec![exp(&["ac1_event"]).into_bytes(), exp(&["ac2_event"]).into_bytes()];
    // Changing the plan default before replay makes the inherited copy differ.
    let iso = spec_of(Dialect::Iso, &[ISO_STD], &iso_ref(), InheritedFrom::Explicit);
    start_st.set_plan_default(&start_dp, H, "plan-1", Some(iso)).unwrap();
    let out = replay(&start_st, &start_dp, &journal, &start_dp, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Divergent);
    assert_eq!(out.steps.len(), 1);
    let (st0, dp0) = load("plan_asme.fidproj");
    let out = replay(&st0, &dp0, &[b"{}".to_vec()], &dp0, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::NotReplayable);
    // The explicit event alone fails: the characteristic does not exist yet.
    let out = replay(&st0, &dp0, &journal[1..], &dp0, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Divergent);
}

// ------------------------------------------------------------------ DPROF AC-6 through SPEC

#[test]
fn dprof_ac6_agent_profiles_are_refused_on_the_working_branch_until_adopted() {
    let (mut st, mut dp, _) = ac1();
    let agent_ref = publish_doc(&mut dp, A, &read("dprof", "asme_placeholder_1.1.0.json"));
    let r = agent_ref.to_string();
    let (st0, dp0) = (st.clone(), dp.clone());
    let e = set(&mut st, &mut dp, H, "flat-1", "ASME", &[ASME_STD], &r).unwrap_err();
    assert_eq!(e.code(), "defaults_profile_unknown");
    assert_eq!((&st, &dp), (&st0, &dp0));
    // The agent may select it on its own branch; the working branch is untouched.
    let a = set(&mut st, &mut dp, A, "flat-1", "ASME", &[ASME_STD], &r).unwrap();
    assert_eq!(a.branch(), Branch::Agent);
    assert_eq!(a.event().unwrap().branch(), Branch::Agent);
    assert_eq!(st.require_governing_spec(A, "flat-1", &dp).unwrap().defaults_profile(), &agent_ref);
    let w = st.require_governing_spec(H, "flat-1", &dp).unwrap();
    assert_eq!(w.defaults_profile().to_string(), asme_ref());
    assert_eq!(dp.binding(Branch::Working, &cid("flat-1")).unwrap().to_string(), asme_ref());
    // A human cannot set a plan default to it either; after adoption, both work.
    let agent_spec = spec_of(Dialect::Asme, &[ASME_STD], &r, InheritedFrom::Explicit);
    assert_eq!(
        st.set_plan_default(&dp, H, "plan-1", Some(agent_spec.clone())).unwrap_err().code(),
        "defaults_profile_unknown"
    );
    dp.adopt(H, &agent_ref).unwrap();
    set(&mut st, &mut dp, H, "flat-1", "ASME", &[ASME_STD], &r).unwrap();
    st.set_plan_default(&dp, H, "plan-1", Some(agent_spec)).unwrap();
}

#[test]
fn agent_work_never_changes_or_blocks_the_working_branch() {
    let (mut st, mut dp, _) = ac1();
    let (w_st, w_dp) = (
        st.characteristics(Branch::Working).map(|(c, s)| (c.clone(), s.cloned())).collect::<Vec<_>>(),
        dp.binding(Branch::Working, &cid("flat-1")).cloned(),
    );
    let c = st.create_characteristic(&mut dp, A, "plan-1", "agent-1", false).unwrap();
    assert_eq!(c.branch(), Branch::Agent);
    assert_eq!(c.event().unwrap().branch(), Branch::Agent);
    assert_eq!(st.require_governing_spec(H, "agent-1", &dp).unwrap_err().code(), "char_unknown");
    assert!(st.require_governing_spec(A, "agent-1", &dp).is_ok());
    assert_eq!(dp.binding(Branch::Working, &cid("agent-1")), None);
    let after: Vec<_> = st.characteristics(Branch::Working).map(|(c, s)| (c.clone(), s.cloned())).collect();
    assert_eq!(after, w_st);
    assert_eq!(dp.binding(Branch::Working, &cid("flat-1")).cloned(), w_dp);
    // A human may still create the same ID, or a case variant of it, on the working branch.
    st.create_characteristic(&mut dp, H, "plan-1", "AGENT-1", false).unwrap();
    // The agent may not re-create a working characteristic.
    assert_eq!(st.create_characteristic(&mut dp, A, "plan-1", "flat-1", false).unwrap_err().code(), "char_exists");
    // Seen from the agent branch, `agent-1` and `AGENT-1` now collide: refused, not guessed.
    assert_eq!(st.require_governing_spec(A, "AGENT-1", &dp).unwrap_err().code(), "char_case_conflict");
    assert!(st.require_governing_spec(H, "AGENT-1", &dp).is_ok());
    // Defaults are human-only.
    assert_eq!(st.set_plan_default(&dp, A, "plan-1", None).unwrap_err(), SpecError::HumanOnly);
    assert_eq!(st.set_tenant_default(&dp, Principal::Script, None).unwrap_err(), SpecError::HumanOnly);
}

// ------------------------------------------------------------------ case-only duplicates

#[test]
fn case_only_duplicates_are_refused() {
    let (mut st, mut dp, _) = ac1();
    let e = st.create_characteristic(&mut dp, H, "plan-1", "Flat-1", false).unwrap_err();
    assert_eq!(e, SpecError::CharCaseConflict { char: "Flat-1".into() });
    let e = set(&mut st, &mut dp, H, "FLAT-1", "ASME", &[ASME_STD], &asme_ref()).unwrap_err();
    assert_eq!(e.code(), "char_case_conflict");
    assert_eq!(st.require_governing_spec(H, "FLAT-1", &dp).unwrap_err().code(), "char_case_conflict");
    assert_eq!(st.create_characteristic(&mut dp, H, "plan-1", "flat-1", false).unwrap_err().code(), "char_exists");
    let e = st.create_characteristic(&mut dp, H, "PLAN-1", "x-1", false).unwrap_err();
    assert_eq!(e, SpecError::BadRequest { reason: "plan_case_conflict" });
    let asme = spec_of(Dialect::Asme, &[ASME_STD], &asme_ref(), InheritedFrom::Explicit);
    assert_eq!(st.set_plan_default(&dp, H, "Plan-1", Some(asme)).unwrap_err().reason(), Some("plan_case_conflict"));
}

#[test]
fn projects_with_case_only_or_exact_duplicate_keys_are_refused() {
    let base = text("no_spec.fidproj");
    let t = base.replace("\"c-flat-1\": null", "\"c-flat-1\": null, \"C-FLAT-1\": null");
    assert_eq!(load_text(&t).map(|_| ()).unwrap_err(), SpecError::ProjectInvalid { reason: "case_duplicate" });
    let t = base.replace("\"c-flat-1\": null", "\"c-flat-1\": null, \"c-flat-1\": null");
    assert_eq!(load_text(&t).map(|_| ()).unwrap_err(), SpecError::ProjectInvalid { reason: "duplicate_key" });
    let t = base.replace("\"c-flat-1\": null", "\"c-flat-1\": null, \"c-fl\\u0061t-1\": null");
    assert_eq!(load_text(&t).map(|_| ()).unwrap_err(), SpecError::ProjectInvalid { reason: "duplicate_key" });
    let plan = text("plan_asme.fidproj");
    let i = plan.find("\"plan-1\": {").unwrap();
    let j = plan[i..].find("\n  }").unwrap() + i + 4;
    let entry = &plan[i..j];
    let t = plan.replace(entry, &format!("{entry},\n{}", entry.replace("\"plan-1\"", "\"PLAN-1\"")));
    assert_eq!(load_text(&t).map(|_| ()).unwrap_err(), SpecError::ProjectInvalid { reason: "case_duplicate" });
    // The same ID on both branches is the agent overlay, not a duplicate.
    let t = base.replace("\"agent\": {}\n }\n}", "\"agent\": {\"C-FLAT-1\": null}\n }\n}");
    assert_ne!(t, base);
    assert!(load_text(&t).is_ok());
}

// ------------------------------------------------------------------ numbers

#[test]
fn non_integer_numbers_nan_infinity_and_subnormals_are_refused() {
    let base = text("plan_asme.fidproj");
    assert!(base.contains("\"placeholder.count\": 7,"));
    for lit in ["7.0", "7e0", "1e-310", "4.9e-324", "2.2250738585072014e-308", "-0.0", "1E400"] {
        let t = base.replace("\"placeholder.count\": 7,", &format!("\"placeholder.count\": {lit},"));
        let e = load_text(&t).map(|_| ()).unwrap_err();
        assert!(e == SpecError::ProjectInvalid { reason: "bad_number" } || e.reason() == Some("json"), "{lit}: {e}");
    }
    for lit in ["NaN", "Infinity", "-Infinity", "nan", "inf"] {
        let t = base.replace("\"placeholder.count\": 7,", &format!("\"placeholder.count\": {lit},"));
        assert_eq!(load_text(&t).map(|_| ()).unwrap_err(), SpecError::ProjectInvalid { reason: "json" }, "{lit}");
    }
    // A number where a spec holds text is the wrong type.
    let t = base.replace("\"dialect\": \"ASME\"", "\"dialect\": 1");
    assert_eq!(load_text(&t).map(|_| ()).unwrap_err(), SpecError::ProjectInvalid { reason: "spec_wrong_type" });
}

// ------------------------------------------------------------------ grammars and type invariants

#[test]
fn standard_reference_grammar() {
    for ok in [ASME_STD, ISO_STD, "std:iso:14405-1:2016", "std:iso:5459:2011", "std:asme:y14.5.1:2019"] {
        let r = StdRef::parse(ok).unwrap();
        assert_eq!(r.to_string(), ok);
        assert_eq!(StdRef::new(r.body(), r.designator(), r.edition()).unwrap(), r);
    }
    assert_eq!(StdRef::parse(ASME_STD).unwrap().profile_edition(), "y14.5-2018");
    let long = format!("std:asme:{}:2018", "y".repeat(33));
    let too_long = format!("std:asme:y14.5:{}", "1".repeat(80));
    for bad in [
        "",
        "STD:asme:y14.5:2018",
        "std:ASME:y14.5:2018",
        "std:asme:Y14.5:2018",
        "std:ansi:y14.5:2018",
        "std:asme:y14.5",
        "std:asme:y14.5:2018:x",
        "std:asme::2018",
        "std:asme:y14.5:",
        "std:asme:y14..5:2018",
        "std:asme:-y14.5:2018",
        "std:asme:y14.5-:2018",
        "std:asme:y14 5:2018",
        "std:asme:y14.5:2018 ",
        " std:asme:y14.5:2018",
        "std:asme:y14\u{2024}5:2018",
        "std:iso:1101:2017\n",
        long.as_str(),
        too_long.as_str(),
    ] {
        let e = StdRef::parse(bad).unwrap_err();
        assert_eq!(e.code(), "bad_std_ref", "{bad:?}");
    }
    assert_eq!(StdRef::new(Dialect::Asme, "Y14.5", "2018").unwrap_err().code(), "bad_std_ref");
    // The error echo is bounded.
    let huge = "std:".repeat(10_000);
    let SpecError::BadStdRef { text } = StdRef::parse(&huge).unwrap_err() else { panic!("bad_std_ref") };
    assert!(text.len() <= 160);
}

#[test]
fn standards_lists_are_sorted_unique_and_unambiguous() {
    let p = |s: &str| StdRef::parse(s).unwrap();
    let a = Standards::new(vec![p("std:iso:5459:2011"), p(ISO_STD)]).unwrap();
    let b = Standards::new(vec![p(ISO_STD), p("std:iso:5459:2011")]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.as_slice()[0].to_string(), ISO_STD);
    assert_eq!(Standards::new(vec![]).unwrap_err(), SpecError::GoverningSpecMissing { field: "standards" });
    assert_eq!(Standards::new(vec![p(ISO_STD), p(ISO_STD)]).unwrap_err().reason(), Some("duplicate_standard"));
    let e = Standards::new(vec![p(ISO_STD), p("std:iso:1101:2012")]).unwrap_err();
    assert_eq!(e.reason(), Some("conflicting_editions"));
    let many: Vec<StdRef> = (0..=MAX_STANDARDS).map(|i| p(&format!("std:iso:{}:2017", 1000 + i))).collect();
    assert_eq!(Standards::new(many).unwrap_err().reason(), Some("too_many_standards"));
}

#[test]
fn governing_spec_constructor_applies_the_loader_rules() {
    let p = |s: &str| StdRef::parse(s).unwrap();
    let asme = Standards::new(vec![p(ASME_STD)]).unwrap();
    let mixed = Standards::new(vec![p(ASME_STD), p(ISO_STD)]).unwrap();
    let e = GoverningSpec::new(Dialect::Asme, mixed, rf(&asme_ref()), InheritedFrom::Explicit).unwrap_err();
    assert_eq!(e.to_string(), "dialect_mismatch{std:iso:1101:2017}");
    let e = GoverningSpec::new(Dialect::Asme, asme.clone(), rf(&iso_ref()), InheritedFrom::Explicit).unwrap_err();
    assert_eq!(e.code(), "dialect_mismatch");
    let y2009 = Standards::new(vec![p("std:asme:y14.5:2009")]).unwrap();
    let e = GoverningSpec::new(Dialect::Asme, y2009, rf(&asme_ref()), InheritedFrom::Explicit).unwrap_err();
    assert_eq!(e.code(), "defaults_profile_edition_mismatch");
    let ok = GoverningSpec::new(Dialect::Asme, asme, rf(&asme_ref()), InheritedFrom::Explicit).unwrap();
    let copy = ok.with_inherited_from(InheritedFrom::Tenant);
    assert!(copy.same_rules(&ok) && copy != ok);
    // Every spec the constructor builds reads back equal through the strict reader.
    let back = GoverningSpec::from_cvalue(&copy.to_cvalue(), |r| SpecError::ProjectInvalid { reason: r }).unwrap();
    assert_eq!(back, copy);
    assert_eq!(parse_dialect_wire(dialect_wire(Dialect::Iso)), Some(Dialect::Iso));
    for bad in ["asme", "Asme", "iso", "ISO ", ""] {
        assert_eq!(parse_dialect_wire(bad), None, "{bad:?}");
    }
    assert!(PlanId::parse("plan-1").is_some() && PlanId::parse("-plan").is_none() && PlanId::parse("").is_none());
}

#[test]
fn set_checks_run_in_the_pinned_order() {
    let (mut st, mut dp, _) = ac1();
    let (st0, dp0) = (st.clone(), dp.clone());
    let a = asme_ref();
    let modified = asme_mod_ref();
    let unknown = format!("dp:asme-y14.5-2018@9.0.0#sha256:{}", "0".repeat(64));
    let seventeen: Vec<String> = (0..=MAX_STANDARDS).map(|i| format!("std:asme:y{i}:2018")).collect();
    let seventeen: Vec<&str> = seventeen.iter().map(String::as_str).collect();
    let cases: Vec<(&str, &str, Vec<&str>, &str, &str)> = vec![
        ("", "ASME", vec![ASME_STD], &a, "bad_request{bad_char_id}"),
        ("flat-1", "", vec![ASME_STD], &a, "governing_spec_missing{dialect}"),
        ("flat-1", "asme", vec![ASME_STD], &a, "bad_request{bad_dialect}"),
        ("flat-1", "ASME", vec![], &a, "governing_spec_missing{standards}"),
        ("flat-1", "ASME", seventeen, &a, "bad_request{too_many_standards}"),
        ("flat-1", "ASME", vec!["std:asme:Y14.5:2018"], &a, "bad_std_ref{std:asme:Y14.5:2018}"),
        ("flat-1", "ASME", vec![ASME_STD, ASME_STD], &a, "bad_request{duplicate_standard}"),
        ("flat-1", "ASME", vec![ASME_STD, "std:asme:y14.5:2009"], &a, "bad_request{conflicting_editions}"),
        ("flat-1", "ASME", vec![ASME_STD], "", "governing_spec_missing{defaults_profile}"),
        ("flat-1", "ASME", vec![ASME_STD], "dp:asme", "bad_profile_ref"),
        ("flat-1", "ASME", vec!["std:asme:y14.5:2009"], &a, "defaults_profile_edition_mismatch"),
        ("nope", "ASME", vec![ASME_STD], &a, "char_unknown{nope}"),
        ("flat-1", "ASME", vec![ASME_STD], &unknown, "defaults_profile_unknown"),
        ("flat-1", "ASME", vec![ASME_STD], &modified, "defaults_profile_digest_mismatch"),
    ];
    for (c, d, stds, prof, want) in cases {
        let e = set(&mut st, &mut dp, H, c, d, &stds, prof).unwrap_err();
        assert!(e.to_string().starts_with(want), "{c} {d} {stds:?}: {e} (want {want})");
    }
    assert_eq!((st, dp), (st0, dp0));
}

#[test]
fn dry_runs_change_nothing_and_journal_nothing() {
    let (mut st, mut dp, _) = ac1();
    let (st0, dp0) = (st.clone(), dp.clone());
    let req = SetRequest { char: "flat-1", dialect: "ISO", standards: &[ISO_STD], defaults_profile: &iso_ref() };
    let r = st.set_governing_spec(&mut dp, H, &req, true).unwrap();
    assert!(r.dry_run() && r.event().is_none());
    assert_eq!(r.new_spec().dialect(), Dialect::Iso);
    let c = st.create_characteristic(&mut dp, H, "plan-1", "flat-3", true).unwrap();
    assert!(c.dry_run() && c.event().is_none() && c.spec().is_some());
    assert_eq!((st, dp), (st0, dp0));
}

#[test]
fn defaults_must_be_explicit_and_resolve_on_the_working_branch() {
    let (mut st, dp) = load("plan_asme.fidproj");
    let planned = spec_of(Dialect::Asme, &[ASME_STD], &asme_ref(), InheritedFrom::Plan);
    assert_eq!(
        st.set_plan_default(&dp, H, "plan-2", Some(planned)).unwrap_err().reason(),
        Some("default_not_explicit")
    );
    let modified = spec_of(Dialect::Asme, &[ASME_STD], &asme_mod_ref(), InheritedFrom::Explicit);
    let e = st.set_tenant_default(&dp, H, Some(modified)).unwrap_err();
    assert_eq!(e.code(), "defaults_profile_digest_mismatch");
    assert_eq!(st.set_plan_default(&dp, H, "plan 2", None).unwrap_err().reason(), Some("bad_plan_id"));
    st.set_plan_default(&dp, H, "plan-1", None).unwrap();
    assert!(st.plan_default(&PlanId::parse("plan-1").unwrap()).is_none());
    assert!(st.tenant_default().is_none());
}

// ------------------------------------------------------------------ re-point stays in step

const ASME_2009: &str = r#"{"schema":"fid.defaults-profile/1","dialect":"asme","edition":"y14.5-2009","version":"1.0.0","keys":{"placeholder.synthetic":true}}"#;

#[test]
fn repoint_moves_bindings_and_governing_specs_together() {
    let (mut st, mut dp, _) = ac1();
    st.create_characteristic(&mut dp, H, "plan-1", "flat-2", false).unwrap();
    let to = publish_doc(&mut dp, H, &read("dprof", "asme_placeholder_1.1.0.json"));
    let r = st.repoint_defaults_profile(&mut dp, H, &["flat-1"], &to.to_string(), false).unwrap();
    let Some(DprofEvent::Repoint(e)) = &r.event() else { panic!("repoint event") };
    assert_eq!(e.changes().len(), 1);
    let s1 = st.require_governing_spec(H, "flat-1", &dp).unwrap();
    assert_eq!((s1.defaults_profile(), s1.inherited_from()), (&to, InheritedFrom::Explicit));
    let s2 = st.require_governing_spec(H, "flat-2", &dp).unwrap();
    assert_eq!(s2.defaults_profile().to_string(), asme_ref());
    // A target whose edition none of the standards name is refused before anything moves.
    let y2009 = publish_doc(&mut dp, H, ASME_2009.as_bytes());
    let (st0, dp0) = (st.clone(), dp.clone());
    let e = st.repoint_defaults_profile(&mut dp, H, &["flat-2"], &y2009.to_string(), false).unwrap_err();
    assert_eq!(e.code(), "defaults_profile_edition_mismatch");
    assert_eq!((&st, &dp), (&st0, &dp0));
    // DPROF's own refusals pass through unchanged.
    let e = st.repoint_defaults_profile(&mut dp, H, &["flat-1"], &iso_ref(), false).unwrap_err();
    assert_eq!(e.code(), "dialect_mismatch");
    // A dry run changes nothing.
    st.repoint_defaults_profile(&mut dp, H, &["flat-2"], &to.to_string(), true).unwrap();
    assert_eq!((&st, &dp), (&st0, &dp0));
    // The result round-trips through the project file.
    let back = load_project(&project_bytes(&st, &dp).unwrap(), &vocab()).unwrap();
    assert_eq!(back, (st, dp));
}

#[test]
fn a_repoint_behind_the_specs_back_is_caught_by_the_gate_and_the_writer() {
    let (st, mut dp, _) = ac1();
    let to = publish_doc(&mut dp, H, &read("dprof", "asme_placeholder_1.1.0.json"));
    dp.repoint(H, &["flat-1"], &to.to_string(), false).unwrap();
    let e = st.require_governing_spec(H, "flat-1", &dp).unwrap_err();
    assert_eq!(e.to_string(), "defaults_profile_binding_mismatch{flat-1}");
    assert_eq!(project_bytes(&st, &dp).unwrap_err().reason(), Some("bindings_out_of_sync"));
}

// ------------------------------------------------------------------ project file

#[test]
fn project_writer_matches_the_python_canonical_form_and_round_trips() {
    let (st, dp) = load("plan_asme.fidproj");
    let out = project_bytes(&st, &dp).unwrap();
    assert_eq!(String::from_utf8(out.clone()).unwrap(), exp(&["plan_asme_canonical"]));
    assert_eq!(load_project(&out, &vocab()).unwrap(), (st, dp));
    // After AC-1, AC-2 and agent work, the file still round-trips to equal states.
    let (mut st, mut dp, _) = ac1();
    set(&mut st, &mut dp, H, "flat-1", "ISO", &[ISO_STD], &iso_ref()).unwrap();
    st.create_characteristic(&mut dp, A, "plan-1", "agent-1", false).unwrap();
    set(&mut st, &mut dp, A, "flat-1", "ASME", &[ASME_STD], &asme_ref()).unwrap();
    let back = load_project(&project_bytes(&st, &dp).unwrap(), &vocab()).unwrap();
    assert_eq!(back, (st, dp));
    for name in ["no_spec.fidproj", "digest_mismatch.fidproj"] {
        let (st, dp) = load(name);
        assert_eq!(load_project(&project_bytes(&st, &dp).unwrap(), &vocab()).unwrap(), (st, dp), "{name}");
    }
}

#[test]
fn project_loader_is_strict() {
    let base = text("plan_asme.fidproj");
    let bad = |t: &str| load_text(t).map(|_| ()).unwrap_err();
    let pi = |r: &'static str| SpecError::ProjectInvalid { reason: r };
    assert_eq!(bad(&base.replace("fid.gdt-spec-project/2", "fid.gdt-spec-project/1")), pi("bad_schema"));
    // The embedded store pins the vocabulary each profile was published under (from the
    // Python reference); a loader given another vocabulary refuses rather than re-checking.
    assert!(base.contains(&exp(&["vocabulary"])));
    assert_eq!(exp(&["vocabulary"]), vocab().digest().as_str());
    let other = KeyVocabulary::new(["placeholder.synthetic"]).unwrap();
    let e = load_project(base.as_bytes(), &other).map(|_| ()).unwrap_err();
    assert_eq!(e.reason(), Some("vocabulary_unknown"));
    assert_eq!(bad(&base.replacen("{\n", "{\n \"extra\": 1,\n", 1)), pi("bad_keys"));
    assert_eq!(bad(&base.replace("\"tenant_default\": null,\n", "")), pi("bad_keys"));
    assert_eq!(bad(&base.replace("\"plan_defaults\": {", "\"plan_defaults\": [1], \"x\": {")), pi("bad_keys"));
    assert_eq!(bad("[]"), pi("wrong_type"));
    assert_eq!(bad(""), pi("json"));
    let bound =
        base.replace("\"bindings\": {\n   \"working\": {}", "\"bindings\": {\n   \"working\": {\"flat-1\": \"x\"}");
    assert_ne!(bound, base);
    assert_eq!(bad(&bound), pi("store_bindings"));
    assert_eq!(bad(&base.replace("\"plan-1\"", "\"plan 1\"")), pi("bad_plan_id"));
    assert_eq!(bad(&base.replace("\"dialect\": \"ASME\"", "\"dialect\": \"asme\"")).reason(), Some("bad_dialect"));
    let t = base.replace("\"inherited_from\": \"explicit\"", "\"inherited_from\": \"plan\"");
    assert_eq!(bad(&t), pi("default_not_explicit"));
    let t = base.replace("\"inherited_from\": \"explicit\"", "\"inherited_from\": \"Explicit\"");
    assert_eq!(bad(&t), pi("bad_inherited_from"));
    let t = base.replacen(&asme_ref(), &asme_mod_ref(), 2).replacen(&asme_mod_ref(), &asme_ref(), 1);
    assert_eq!(bad(&t).code(), "defaults_profile_digest_mismatch");
    let ns = text("no_spec.fidproj");
    assert_eq!(bad(&ns.replace("\"c-flat-1\"", "\"c flat\"")), pi("bad_char_id"));
    // The profile store's own loader still runs (a profile ref that does not match its content).
    let t = ns.replacen(&asme_ref(), &asme_mod_ref(), 1);
    assert_eq!(bad(&t).code(), "store_invalid");
    // Nesting deeper than the format needs.
    let t = base.replace("\"b\",", "[[\"b\"]],");
    assert_eq!(bad(&t), pi("json"));
}

#[test]
fn every_truncation_and_byte_flip_of_a_project_is_handled_without_panic() {
    let base = read("spec", "plan_asme.fidproj");
    let v = vocab();
    // Every proper prefix is refused, except the one that only drops the final newline.
    for n in 0..base.len() - 1 {
        assert!(load_project(&base[..n], &v).is_err());
    }
    for i in (0..base.len()).step_by(3) {
        let mut b = base.clone();
        b[i] ^= 0x20;
        let _ = load_project(&b, &v);
    }
}

// ------------------------------------------------------------------ events

#[test]
fn events_round_trip_strictly() {
    for k in ["ac1_event", "ac2_event"] {
        let t = exp(&[k]);
        let e = SpecEvent::from_canonical_bytes(t.as_bytes()).unwrap();
        assert_eq!(bytes(&e), t);
    }
    let ac1 = exp(&["ac1_event"]);
    let ac2 = exp(&["ac2_event"]);
    let ev = |reason: &'static str| SpecError::Event { reason };
    let bad = |t: &str| SpecEvent::from_canonical_bytes(t.as_bytes()).unwrap_err();
    assert_eq!(bad(&ac1.replace("\"branch\":\"working\"", "\"branch\":\"agent\"")), ev("branch_principal_mismatch"));
    assert_eq!(bad(&ac1.replace("\"plan\":\"plan-1\"", "\"plan\":null")), ev("bad_shape"));
    assert_eq!(bad(&ac2.replace("\"plan\":null", "\"plan\":\"plan-1\"")), ev("bad_shape"));
    assert_eq!(
        bad(&ac2.replacen("\"inherited_from\":\"explicit\"", "\"inherited_from\":\"plan\"", 1)),
        ev("inherited_from_mismatch")
    );
    assert_eq!(bad(&ac1.replace("gdt.set_governing_spec@1", "gdt.set_governing_spec@2")), ev("unknown_command"));
    assert_eq!(bad(&ac1.replacen("{", "{\"x\":null,", 1)), ev("bad_keys"));
    assert_eq!(bad(&ac1.replace("\"ASME\"", "\"asme\"")), ev("bad_spec"));
    assert_eq!(bad(&ac1.replace("\"char\":\"flat-1\"", "\"char\":\"\"")), ev("bad_char_id"));
    assert_eq!(bad(&ac1.replacen("\"branch\"", "\"branch\":\"working\",\"branch\"", 1)), ev("json"));
    let huge = format!("{{\"x\":\"{}\"}}", "a".repeat(70_000));
    assert_eq!(bad(&huge), ev("too_large"));
    // An inheritance event whose old spec is set is refused.
    let new_obj = &ac2[ac2.find("\"new\":").unwrap() + 6..ac2.find(",\"old\"").unwrap()];
    let t = ac1.replace("\"old\":null", &format!("\"old\":{new_obj}"));
    assert_eq!(bad(&t), ev("bad_shape"));
}

#[test]
fn errors_display_their_code() {
    assert_eq!(SpecError::HumanOnly.to_string(), "human_only");
    assert_eq!(SpecError::BadProfileRef.to_string(), "bad_profile_ref");
    assert_eq!(SpecError::GoverningSpecMissing { field: "standards" }.to_string(), "governing_spec_missing{standards}");
    assert_eq!(SpecError::CharExists { char: "a\"b".into() }.to_string(), "char_exists{a\\\"b}");
    let d = SpecError::from(fid_gdt::chars::dprof::DprofError::Store { reason: "json" });
    assert_eq!((d.code(), d.reason(), d.to_string().as_str()), ("store_invalid", Some("json"), "store_invalid{json}"));
}

#[test]
fn the_gate_is_total_on_hostile_ids() {
    let (st, dp, _) = ac1();
    let long = "a".repeat(10_000);
    for id in ["", " ", "é", "\0", "flat-1\n", long.as_str(), "../flat-1"] {
        let e = st.require_governing_spec(H, id, &dp).unwrap_err();
        assert!(matches!(e.code(), "bad_request" | "char_unknown"), "{id:?}: {e}");
    }
}

// ------------------------------------------------------------------ review of #241

#[test]
fn invalid_profile_identities_cannot_be_built() {
    use fid_gdt::chars::dprof::{Edition, ProfileId, SemVer};
    // SemVer parts are capped at nine digits, as the parser caps them.
    assert!(SemVer::new(1_000_000_000, 0, 0).is_none());
    assert!(SemVer::new(0, 1_000_000_000, 0).is_none());
    assert!(SemVer::new(0, 0, u32::MAX).is_none());
    let max = SemVer::new(999_999_999, 999_999_999, 999_999_999).unwrap();
    assert_eq!(SemVer::parse(&max.to_string()), Some(max));
    // Editions only through the parser.
    for bad in ["", "Y14.5-2018", "y14..5", "-y", &"a".repeat(33)] {
        assert!(Edition::parse(bad).is_none(), "{bad:?}");
    }
    // The widest reference the parts allow still round-trips through its text.
    let edition = Edition::parse(&"a".repeat(32)).unwrap();
    let digest = *rf(&asme_ref()).digest();
    for dialect in [Dialect::Asme, Dialect::Iso] {
        let r = ProfileRef::new(ProfileId::new(dialect, edition.clone(), max), digest);
        assert_eq!(ProfileRef::parse(&r.to_string()).as_ref(), Some(&r));
    }
    // Every constructible reference is accepted by GoverningSpec::new on the round-trip rule;
    // the rule is checked first.
    let r = ProfileRef::new(ProfileId::new(Dialect::Asme, Edition::parse("y14.5-2018").unwrap(), max), digest);
    let stds = Standards::new(vec![StdRef::parse(ASME_STD).unwrap()]).unwrap();
    let s = GoverningSpec::new(Dialect::Asme, stds, r.clone(), InheritedFrom::Explicit).unwrap();
    assert_eq!(s.defaults_profile(), &r);
    // Parsed documents carry a digest that holds over their canonical bytes.
    for f in ["asme_placeholder_1.0.0.json", "asme_placeholder_1.1.0.json", "iso_placeholder_1.0.0.json"] {
        let doc = parse_profile(&read("dprof", f), &vocab()).unwrap();
        assert!(doc.digest_holds(), "{f}");
        assert_eq!(doc.reference().id(), &doc.profile().id());
    }
}

#[test]
fn mixed_journal_of_create_publish_repoint_and_set_replays_identical() {
    use fid_types::Digest;
    use std::collections::BTreeMap;
    let (start_st, start_dp) = load("plan_asme.fidproj");
    let (mut st, mut dp) = (start_st.clone(), start_dp.clone());
    let mut journal: Vec<Vec<u8>> = Vec::new();
    for c in ["flat-1", "flat-2"] {
        let e = st.create_characteristic(&mut dp, H, "plan-1", c, false).unwrap();
        journal.push(e.event().unwrap().canonical_bytes().unwrap());
    }
    let a110 = read("dprof", "asme_placeholder_1.1.0.json");
    let doc = parse_profile(&a110, &vocab()).unwrap();
    let p = dp.publish(doc, H, "root://imports/asme_placeholder_1.1.0.json", false).unwrap();
    journal.push(p.event().unwrap().canonical_bytes().unwrap());
    let r = st.repoint_defaults_profile(&mut dp, H, &["flat-1"], &p.reference().to_string(), false).unwrap();
    journal.push(r.event().unwrap().canonical_bytes().unwrap());
    // After the re-point the spec no longer follows the plan default.
    let s1 = st.require_governing_spec(H, "flat-1", &dp).unwrap();
    assert_eq!((s1.inherited_from(), s1.defaults_profile()), (InheritedFrom::Explicit, p.reference()));
    assert_eq!(st.require_governing_spec(H, "flat-2", &dp).unwrap().inherited_from(), InheritedFrom::Plan);
    let s = set(&mut st, &mut dp, H, "flat-2", "ISO", &[ISO_STD], &iso_ref()).unwrap();
    journal.push(s.event().unwrap().canonical_bytes().unwrap());
    // A later set on the re-pointed characteristic records the explicit origin as old.
    let s = set(&mut st, &mut dp, H, "flat-1", "ASME", &[ASME_STD], &asme_ref()).unwrap();
    assert_eq!(s.old().unwrap().inherited_from(), InheritedFrom::Explicit);
    journal.push(s.event().unwrap().canonical_bytes().unwrap());

    let content: BTreeMap<Digest, Vec<u8>> = [(*p.reference().digest(), a110.clone())].into_iter().collect();
    let out = replay(&start_st, &start_dp, &journal, &content, &vocab());
    assert_eq!(out.steps, vec![ReplayOutcome::Identical; journal.len()]);
    assert_eq!(out.outcome, ReplayOutcome::Identical);
    assert_eq!((out.spec, out.dprof), (st, dp));
    // Without the published content the publish step is not replayable.
    let none: BTreeMap<Digest, Vec<u8>> = BTreeMap::new();
    let out = replay(&start_st, &start_dp, &journal, &none, &vocab());
    assert_eq!(out.steps.last(), Some(&ReplayOutcome::NotReplayable));
    // Dropping the publish makes the re-point diverge.
    let mut skipped = journal.clone();
    skipped.remove(2);
    let out = replay(&start_st, &start_dp, &skipped, &content, &vocab());
    assert_eq!(out.outcome, ReplayOutcome::Divergent);
}

#[test]
fn the_gate_takes_its_branch_from_the_principal() {
    let (mut st, mut dp, _) = ac1();
    set(&mut st, &mut dp, A, "flat-1", "ISO", &[ISO_STD], &iso_ref()).unwrap();
    assert_eq!(st.require_governing_spec(H, "flat-1", &dp).unwrap().dialect(), Dialect::Asme);
    assert_eq!(st.require_governing_spec(A, "flat-1", &dp).unwrap().dialect(), Dialect::Iso);
    assert_eq!(st.require_governing_spec(Principal::Script, "flat-1", &dp).unwrap().dialect(), Dialect::Iso);
}
