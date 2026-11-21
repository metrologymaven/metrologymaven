//! Acceptance criteria of spec node AUTO.RUN.BIND (AC-1 .. AC-5) and its error paths, against
//! `fixtures/program/family/bracket.json`, `fixtures/scans/bracket-120.ply` and
//! `fixtures/auto/run/bind/expected.json` (written by the independent reference
//! `fixtures/auto/run/bind/make_cases.py`: digests from hashlib, records hand-labelled from the
//! spec). Lifecycle states are set through the real `AUTO.REC.LIFE` commands; principals are
//! derived by the bus. The record schema is checked with the bus's validator.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{b120, bracket, digest, identified, prepare, Expected, World};
use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;
use fid_program::binding_from_json;
use fid_program_rec::life::LifeState;
use fid_run::{record_schema, BindingSource, ProgramRef, RunError, RunFlags, RunRecord};

const TRIAL: RunFlags = RunFlags { trial: true, override_deprecated: false };
const OVERRIDE: RunFlags = RunFlags { trial: false, override_deprecated: true };

fn canonical(r: &RunRecord) -> String {
    String::from_utf8(r.to_canonical_bytes().unwrap()).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_released_program_for_an_agent() {
    let ex = Expected::load();
    let w = World::at("released");
    let plan = prepare(&w, identified(), b120(), &w.agent, RunFlags::default()).unwrap();
    let r = &plan.record;
    assert_eq!(r.program_digest().as_str(), ex.s("program_digest"));
    assert_eq!(r.program_id().to_string(), ex.s("program_id"));
    assert_eq!(r.variant(), Some("B-120"));
    assert_eq!(r.inputs()["scan"].sha256.as_str(), ex.s("scan_sha256"));
    assert_eq!(r.inputs()["scan"].handle, ex.s("scan_handle"));
    assert_eq!((r.trial(), r.override_deprecated(), r.lifecycle_state()), (false, false, LifeState::Released));
    assert_eq!(canonical(r), ex.s("ac1_record"));
    // The record reads back to itself, strictly.
    assert_eq!(RunRecord::from_canonical_bytes(&r.to_canonical_bytes().unwrap()).unwrap(), *r);
    // Every step, in document order, re-targetable (FMT has no per-step mark yet).
    let steps: Vec<_> = plan.steps_retargetable.iter().map(|(id, _)| *id).collect();
    assert_eq!(steps, bracket().step_ids());
    assert!(plan.steps_retargetable.iter().all(|(_, ok)| *ok));
}

#[test]
fn ac1_released_program_by_binding_and_for_a_person() {
    let ex = Expected::load();
    let w = World::at("released");
    let b = binding_from_json(br#"{"hole_spacing":{"unit":"mm","value":"120"},"scan":"input:scans/bracket-120.ply"}"#)
        .unwrap();
    let r = prepare(&w, identified(), BindingSource::Binding(b), &w.alice, RunFlags::default()).unwrap().record;
    assert_eq!(r.variant(), None);
    assert_eq!(r.inputs()["scan"].sha256.as_str(), ex.s("scan_sha256"));
    assert_eq!(r.principal().kind.as_str(), "human");
    // A trial of a released program only marks the run; anyone may ask for it.
    let t = prepare(&w, identified(), b120(), &w.agent, TRIAL).unwrap().record;
    assert!(t.trial());
    // An override on a program that is not deprecated is not applied, so not recorded.
    let o = prepare(&w, identified(), b120(), &w.alice, OVERRIDE).unwrap().record;
    assert!(!o.override_deprecated());
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_registered_program_needs_a_persons_trial() {
    let ex = Expected::load();
    let w = World::at("registered");
    for flags in [RunFlags::default(), TRIAL] {
        let e = prepare(&w, identified(), b120(), &w.agent, flags).unwrap_err();
        assert_eq!(e, RunError::NotReleased { state: LifeState::Registered });
    }
    assert_eq!(prepare(&w, identified(), b120(), &w.alice, RunFlags::default()).unwrap_err().code(), "not_released");
    let r = prepare(&w, identified(), b120(), &w.alice, TRIAL).unwrap().record;
    assert!(r.trial());
    assert_eq!(canonical(&r), ex.s("ac2_record"));
}

#[test]
fn draft_program_runs_only_as_a_persons_trial() {
    let w = World::at("draft");
    let p = bracket();
    assert_eq!(
        prepare(&w, ProgramRef::Draft(&p), b120(), &w.agent, TRIAL).unwrap_err(),
        RunError::NotReleased { state: LifeState::Draft }
    );
    let r = prepare(&w, ProgramRef::Draft(&p), b120(), &w.alice, TRIAL).unwrap().record;
    assert_eq!((r.lifecycle_state(), r.program_version(), r.trial()), (LifeState::Draft, None, true));
    // An unidentified digest names no frozen bytes.
    assert_eq!(prepare(&w, identified(), b120(), &w.alice, TRIAL).unwrap_err(), RunError::ProgramNotFound);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_deprecated_program_needs_a_persons_override() {
    let ex = Expected::load();
    let w = World::at("deprecated");
    for flags in [RunFlags::default(), TRIAL, OVERRIDE, RunFlags { trial: true, override_deprecated: true }] {
        let e = prepare(&w, identified(), b120(), &w.agent, flags).unwrap_err();
        assert_eq!(e, RunError::Deprecated { version: 1 });
    }
    for flags in [RunFlags::default(), TRIAL] {
        assert_eq!(prepare(&w, identified(), b120(), &w.alice, flags).unwrap_err().code(), "deprecated");
    }
    let r = prepare(&w, identified(), b120(), &w.alice, OVERRIDE).unwrap().record;
    assert!(r.override_deprecated());
    assert_eq!(canonical(&r), ex.s("ac3_record"));
    // A deprecated program cannot be passed off as a draft: its digest identifies it.
    let p = bracket();
    assert_eq!(prepare(&w, ProgramRef::Draft(&p), b120(), &w.alice, TRIAL).unwrap_err().code(), "deprecated");
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_binding_missing_hole_spacing() {
    let ex = Expected::load();
    let w = World::at("released");
    let b = binding_from_json(&common::read("fixtures/program/family/bindings/missing-hole-spacing.json")).unwrap();
    let RunError::BindingInvalid { errors } =
        prepare(&w, identified(), BindingSource::Binding(b), &w.agent, RunFlags::default()).unwrap_err()
    else {
        panic!("binding_invalid")
    };
    let got: Vec<CValue> = errors
        .iter()
        .map(|e| {
            let mut o = std::collections::BTreeMap::new();
            o.insert("code".to_string(), CValue::Str(e.code().into()));
            o.insert("param".to_string(), CValue::Str(e.param().unwrap_or("").into()));
            CValue::Obj(o)
        })
        .collect();
    assert_eq!(CValue::Arr(got), *ex.get("ac4_errors"));
}

#[test]
fn binding_errors_are_all_reported_and_variants_must_exist() {
    let w = World::at("released");
    let b = binding_from_json(&common::read("fixtures/program/family/bindings/many-errors.json")).unwrap();
    let e = prepare(&w, identified(), BindingSource::Binding(b), &w.agent, RunFlags::default()).unwrap_err();
    let RunError::BindingInvalid { errors } = e else { panic!("binding_invalid") };
    assert_eq!(errors.len(), 5);
    let e = prepare(&w, identified(), BindingSource::Variant("B-999".into()), &w.agent, RunFlags::default());
    assert_eq!(e.unwrap_err().code(), "binding_invalid");
    // The lifecycle refusal comes before the binding check.
    let w = World::at("registered");
    let e = prepare(&w, identified(), BindingSource::Variant("B-999".into()), &w.agent, RunFlags::default());
    assert_eq!(e.unwrap_err().code(), "not_released");
}

#[test]
fn unreadable_input() {
    let w = World::at("released");
    // B-4IN binds a scan that does not exist.
    let e = prepare(&w, identified(), BindingSource::Variant("B-4IN".into()), &w.agent, RunFlags::default());
    let Err(RunError::InputUnreadable { param, .. }) = e else { panic!("input_unreadable: {e:?}") };
    assert_eq!(param, "scan");
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_same_inputs_same_record_but_run_id() {
    let ex = Expected::load();
    let w = World::at("released");
    let cat = common::catalogue();
    let mut ids = common::Counting::default();
    let mut ctx = fid_run::BindCtx { lifecycle: &w.store, catalogue: &cat, inputs: &common::Scans, run_ids: &mut ids };
    let a = fid_run::prepare_run(&mut ctx, identified(), b120(), &w.agent, RunFlags::default()).unwrap();
    let b = fid_run::prepare_run(&mut ctx, identified(), b120(), &w.agent, RunFlags::default()).unwrap();
    let CValue::Arr(run_ids) = ex.get("run_ids") else { panic!("run_ids") };
    assert_eq!(CValue::Str(a.record.run_id().to_string()), run_ids[0]);
    assert_eq!(CValue::Str(b.record.run_id().to_string()), run_ids[1]);
    assert_ne!(a.record.run_id(), b.record.run_id());
    let strip = |r: &RunRecord| {
        let CValue::Obj(mut o) = r.to_value() else { panic!("object") };
        o.remove("run_id");
        o
    };
    assert_eq!(strip(&a.record), strip(&b.record));
    assert_eq!(a.steps_retargetable, b.steps_retargetable);
}

// ------------------------------------------------------------------ record form and schema

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

#[test]
fn records_match_their_schema() {
    let schema = Compiled::compile(&record_schema().unwrap()).unwrap();
    let ex = Expected::load();
    for k in ["ac1_record", "ac2_record", "ac3_record"] {
        let v = fid_canon::parse(ex.s(k).as_bytes()).unwrap();
        if let Err(f) = schema.evaluate(&v, &NoHooks) {
            panic!("{k}: {} at {} ({})", f.keyword, f.pointer, f.message);
        }
        let on_branch = RunRecord::from_value(&v).unwrap().with_branch("run/1").to_value();
        assert!(schema.evaluate(&on_branch, &NoHooks).is_ok(), "{k} on a branch");
    }
    let w = World::at("draft");
    let p = bracket();
    let draft = prepare(&w, ProgramRef::Draft(&p), b120(), &w.alice, TRIAL).unwrap().record.to_value();
    assert!(schema.evaluate(&draft, &NoHooks).is_ok());
    let base = fid_canon::parse(ex.s("ac1_record").as_bytes()).unwrap();
    let bad: [(&str, CValue); 6] = [
        ("trial", CValue::Str("yes".into())),
        ("lifecycle_state", CValue::Str("approved".into())),
        ("program_digest", CValue::Str("sha256:ABC".into())),
        ("outcome", CValue::Str("done".into())),
        ("program_version", CValue::Num(0.0)),
        ("approved_by", CValue::Str("alice".into())),
    ];
    for (k, v) in bad {
        let CValue::Obj(mut o) = base.clone() else { panic!("object") };
        o.insert(k.into(), v);
        let v = CValue::Obj(o);
        assert!(schema.evaluate(&v, &NoHooks).is_err(), "schema accepts {k}");
        assert!(RunRecord::from_value(&v).is_err(), "reader accepts {k}");
    }
    // A draft has no version, and only a draft.
    let CValue::Obj(mut o) = base.clone() else { panic!("object") };
    o.insert("program_version".into(), CValue::Null);
    assert!(RunRecord::from_value(&CValue::Obj(o)).is_err());
    // Non-canonical bytes are refused.
    let mut spaced = ex.s("ac1_record").into_bytes();
    spaced.insert(1, b' ');
    assert!(RunRecord::from_canonical_bytes(&spaced).is_err());
    let _ = digest(&ex.s("program_digest"));
}
