//! Spec leaf VIZ.INBOX.GATES.PLAN: integration tests covering AC-1 .. AC-4,
//! `sign.show_profile@1` decoding, `CeremonyProvider` execution, all-or-nothing `GateRunner`
//! semantics, and launch-1 independence from signing crates.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_inbox::gates::plan::{
    plan, plan_with_provider, profile_from_show_result, AssertionRef, CeremonyOutcome, CeremonyProvider, EntryHash,
    GatePlan, GateResult, GateRunner, GateStep, GateUi, RunOutcome, StepOutcome, UnavailableReason,
};
use fid_journal::PrincipalKind;
use fid_profile::{instantiate, MeaningCode, ProfileDoc, ProfileName};
use fid_profile_cmd::{run_show, ProfileSource, ProfileState};
use fid_types::Digest;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse_bool(s: &str) -> bool {
    match s {
        "true" => true,
        "false" => false,
        other => panic!("unexpected bool in fixture: {other}"),
    }
}

fn parse_expected(kind: &str, val: &str) -> GatePlan {
    match kind {
        "unavailable" => match val {
            "human_required" => GatePlan::Unavailable { reason: UnavailableReason::HumanRequired },
            "ceremony_unavailable" => GatePlan::Unavailable { reason: UnavailableReason::CeremonyUnavailable },
            other => panic!("unexpected unavailable reason: {other}"),
        },
        "steps" => {
            if val.is_empty() {
                return GatePlan::Steps(Vec::new());
            }
            let steps = val
                .split(';')
                .map(|tok| {
                    if let Some(inner) = tok.strip_prefix("ConfirmCount(").and_then(|s| s.strip_suffix(')')) {
                        GateStep::ConfirmCount { n: inner.parse().unwrap() }
                    } else if tok == "Reason" {
                        GateStep::Reason
                    } else if let Some(inner) = tok.strip_prefix("Signature(").and_then(|s| s.strip_suffix(')')) {
                        let meanings = if inner.is_empty() {
                            Vec::new()
                        } else {
                            inner.split('+').map(|m| MeaningCode::new(m).unwrap()).collect()
                        };
                        GateStep::Signature { meanings }
                    } else if tok == "SecondPerson" {
                        GateStep::SecondPerson
                    } else {
                        panic!("unexpected step token: {tok}");
                    }
                })
                .collect();
            GatePlan::Steps(steps)
        }
        other => panic!("unexpected expected_kind: {other}"),
    }
}

/// AC-1: every row of `fixtures/viz/inbox-gate-matrix.csv` (3 x 2 x 2 x 2 x 2 = 48 rows) matches.
#[test]
fn ac1_gate_matrix_fixture_matches_every_row() {
    let csv = fs::read_to_string(repo_root().join("fixtures/viz/inbox-gate-matrix.csv")).unwrap();
    let mut rows = 0usize;
    let mut saw_header = false;

    for raw in csv.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !saw_header {
            assert_eq!(
                line,
                "profile,target_protected,command_count,principal,provider_present,expected_kind,expected_value"
            );
            saw_header = true;
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 7, "bad CSV row: {line}");

        let profile_name = ProfileName::from_wire(cols[0]).unwrap();
        let doc = ProfileDoc::builtin(profile_name);
        let target_protected = parse_bool(cols[1]);
        let command_count: usize = cols[2].parse().unwrap();
        let principal = PrincipalKind::parse(cols[3]).unwrap();
        let provider_present = parse_bool(cols[4]);
        let expected = parse_expected(cols[5], cols[6]);

        let got = plan(&doc, target_protected, command_count, principal, provider_present);
        assert_eq!(got, expected, "mismatch on row: {line}");
        rows += 1;
    }

    assert_eq!(rows, 48, "expected 3x2x2x2x2 = 48 rows");
}

struct ScriptedUi {
    seen: Vec<GateStep>,
    outcomes: VecDeque<StepOutcome>,
}

impl ScriptedUi {
    fn new(outcomes: impl IntoIterator<Item = StepOutcome>) -> Self {
        Self { seen: Vec::new(), outcomes: outcomes.into_iter().collect() }
    }
}

impl GateUi for ScriptedUi {
    fn run_step(&mut self, step: &GateStep) -> StepOutcome {
        self.seen.push(step.clone());
        self.outcomes.pop_front().expect("unexpected extra UI step")
    }
}

/// AC-2: Lean profile, unprotected target, no ceremony provider -> single apply has empty plan
/// (launch-1 configuration) and promotes directly with no UI steps.
#[test]
fn ac2_lean_unprotected_single_apply_without_provider_is_empty() {
    let lean = ProfileDoc::builtin(ProfileName::Lean);
    let p = plan_with_provider(&lean, false, 1, PrincipalKind::Human, None);
    assert_eq!(p, GatePlan::Steps(Vec::new()));
    assert!(p.is_available());

    let mut ui = ScriptedUi::new([]);
    let mut promoted = false;
    let outcome: RunOutcome<&'static str, &'static str> = GateRunner::run(&p, &mut ui, |res| {
        assert_eq!(res, GateResult::default());
        promoted = true;
        Ok("head-2")
    });
    assert!(promoted);
    assert!(ui.seen.is_empty());
    assert_eq!(outcome, RunOutcome::Promoted("head-2"));
}

/// AC-3: a fake step that cancels never calls the promotion callback and leaves the proposal pending.
#[test]
fn ac3_cancelled_or_failed_step_never_promotes() {
    let reg = ProfileDoc::builtin(ProfileName::Regulated);
    let p = plan(&reg, false, 5, PrincipalKind::Human, false);
    assert_eq!(p, GatePlan::Steps(vec![GateStep::ConfirmCount { n: 5 }, GateStep::Reason, GateStep::SecondPerson]));

    // Cancel on the second step (Reason): SecondPerson must not run and promotion is never called.
    let mut ui = ScriptedUi::new([StepOutcome::passed(), StepOutcome::Cancelled]);
    let mut promoted = false;
    let outcome: RunOutcome<(), ()> = GateRunner::run(&p, &mut ui, |_| {
        promoted = true;
        Ok(())
    });
    assert!(!promoted, "promotion callback must never run when a step cancels");
    assert_eq!(ui.seen, vec![GateStep::ConfirmCount { n: 5 }, GateStep::Reason]);
    assert_eq!(outcome, RunOutcome::Cancelled { at_step: GateStep::Reason });

    // Failure on step also blocks promotion.
    let mut ui_fail = ScriptedUi::new([StepOutcome::Failed { reason: "token removed".into() }]);
    let outcome_fail: RunOutcome<(), ()> = GateRunner::run(&p, &mut ui_fail, |_| {
        promoted = true;
        Ok(())
    });
    assert!(!promoted);
    assert_eq!(
        outcome_fail,
        RunOutcome::StepFailed { at_step: GateStep::ConfirmCount { n: 5 }, reason: "token removed".into() }
    );
}

/// AC-4: agent principal yields Unavailable { HumanRequired } in every profile and never promotes.
#[test]
fn ac4_agent_principal_is_human_required() {
    for name in ProfileName::ALL {
        let doc = ProfileDoc::builtin(name);
        let p = plan(&doc, false, 1, PrincipalKind::Agent, true);
        assert_eq!(p, GatePlan::Unavailable { reason: UnavailableReason::HumanRequired });
        assert!(!p.is_available());

        let mut ui = ScriptedUi::new([]);
        let mut called = false;
        let outcome: RunOutcome<(), ()> = GateRunner::run(&p, &mut ui, |_| {
            called = true;
            Ok(())
        });
        assert!(!called);
        assert_eq!(outcome, RunOutcome::Unavailable { reason: UnavailableReason::HumanRequired });
    }
}

struct FakeCeremony {
    result: Result<AssertionRef, CeremonyOutcome>,
}

impl CeremonyProvider for FakeCeremony {
    fn run(&self, _pre_image: EntryHash, meanings: &[MeaningCode]) -> Result<AssertionRef, CeremonyOutcome> {
        assert!(!meanings.is_empty(), "regulated ceremony must receive required meanings");
        self.result.clone()
    }
}

/// CeremonyProvider integration via `GateRunner::run_with_ceremony`: passes reason & assertion
/// through on success, blocks promotion on cancel/fail/missing provider.
#[test]
fn ceremony_provider_roundtrip_and_failure_paths() {
    let tenant = br#"{
        "tenant_meanings": [
            {"code": "X_QA_RELEASE", "label": "QA Release", "description": "Quality engineering sign-off."}
        ],
        "required_meanings": ["AUTHORED", "X_QA_RELEASE", "APPROVED"]
    }"#;
    let reg_custom = instantiate(ProfileName::Regulated, Some(tenant)).unwrap();
    let assertion = AssertionRef::new("esig:urn:test:assertion-1").unwrap();
    let ok_provider = FakeCeremony { result: Ok(assertion.clone()) };
    let p = plan_with_provider(reg_custom.document(), true, 1, PrincipalKind::Human, Some(&ok_provider));
    assert_eq!(
        p,
        GatePlan::Steps(vec![
            GateStep::Reason,
            GateStep::Signature {
                meanings: vec![
                    MeaningCode::new("AUTHORED").unwrap(),
                    MeaningCode::new("X_QA_RELEASE").unwrap(),
                    MeaningCode::new("APPROVED").unwrap(),
                ],
            },
            GateStep::SecondPerson,
        ])
    );

    let pre_image = Digest::TOMBSTONE;
    let mut ui = ScriptedUi::new([StepOutcome::passed_with_reason("Approved FAI lot 42"), StepOutcome::passed()]);
    let outcome: RunOutcome<GateResult, ()> =
        GateRunner::run_with_ceremony(&p, pre_image, Some(&ok_provider), &mut ui, Ok);
    assert_eq!(
        outcome,
        RunOutcome::Promoted(GateResult { reason: Some("Approved FAI lot 42".into()), assertion: Some(assertion) })
    );

    // Cancelled ceremony blocks SecondPerson and promotion.
    let cancel_provider = FakeCeremony { result: Err(CeremonyOutcome::Cancelled) };
    let mut ui2 = ScriptedUi::new([StepOutcome::passed_with_reason("reason")]);
    let mut promoted = false;
    let outcome2: RunOutcome<(), ()> =
        GateRunner::run_with_ceremony(&p, pre_image, Some(&cancel_provider), &mut ui2, |_| {
            promoted = true;
            Ok(())
        });
    assert!(!promoted);
    assert!(matches!(outcome2, RunOutcome::Cancelled { at_step: GateStep::Signature { .. } }));

    // Missing provider at runtime fails closed even if handed a Signature step directly.
    let mut ui3 = ScriptedUi::new([StepOutcome::passed_with_reason("reason")]);
    let outcome3: RunOutcome<(), ()> = GateRunner::run_with_ceremony(&p, pre_image, None, &mut ui3, |_| {
        promoted = true;
        Ok(())
    });
    assert!(!promoted);
    assert!(matches!(outcome3, RunOutcome::StepFailed { at_step: GateStep::Signature { .. }, .. }));
}

struct StaticProfileSource {
    state: ProfileState,
}

impl ProfileSource for StaticProfileSource {
    fn state(&self) -> Result<ProfileState, String> {
        Ok(self.state.clone())
    }

    fn call_branch(&self, _ctx: &fid_bus::handler::HandlerCtx) -> Result<String, String> {
        Ok("main".into())
    }
}

/// Reading the active profile directly from `sign.show_profile@1` (`fid_profile_cmd::run_show`)
/// works for default Lean and rejects tampered digest/name/origin fail-closed.
#[test]
fn reads_active_profile_from_show_profile_command() {
    let src = StaticProfileSource { state: ProfileState::default() };
    let shown = run_show(&src).unwrap();
    let canonical = profile_from_show_result(&shown).unwrap();
    assert_eq!(canonical.document().name(), ProfileName::Lean);
    assert_eq!(plan(canonical.document(), false, 1, PrincipalKind::Human, false), GatePlan::Steps(Vec::new()));

    // Tampered digest fails closed.
    let mut bad = shown.clone();
    if let CValue::Obj(ref mut m) = bad {
        m.insert("digest".into(), CValue::Str(Digest::TOMBSTONE.as_str().into()));
    }
    assert!(profile_from_show_result(&bad).is_err());

    // Tampered origin fails closed.
    let mut bad_origin = shown;
    if let CValue::Obj(ref mut m) = bad_origin {
        m.insert("origin".into(), CValue::Str("forged".into()));
    }
    assert!(profile_from_show_result(&bad_origin).is_err());
}

/// Launch-1 contract: `fid-inbox` links no signing crate in `[dependencies]`.
#[test]
fn no_signing_crate_linked() {
    let manifest = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("fid-sign-core"), "fid-inbox must not link fid-sign-core");
    assert!(!manifest.contains("fid-esig"), "fid-inbox must not link any e-signature crate");
}
