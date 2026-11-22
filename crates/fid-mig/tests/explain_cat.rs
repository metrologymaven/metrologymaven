//! Acceptance tests for `MIG.EXPLAIN.CAT` (`spec/mig/explain/cat/NODE.md`):
//! AC-1 (valid-set.json matches valid-set.digest),
//! AC-2 (bad-factor.json PROBE_RADIUS -> candidate_invalid at index 0),
//! AC-3 (agent principal forces source = AgentProposed with agent principal & session IDs),
//! AC-4 (reordered candidates yield identical digest),
//! AC-5 (65 candidates -> candidate_set_too_large).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::principal::PrincipalKind;
use fid_canon::{parse_with_limits, CValue};
use fid_mig::explain::cat::{
    validate_candidate_set_json, validate_candidate_set_value, CallerContext, CandidateSource, CatError, Factor,
    FactorCatalogue, MAX_CANDIDATES,
};

const VALID_SET_JSON: &str = include_str!("../../../fixtures/mig/explain/cat/valid-set.json");
const VALID_SET_DIGEST: &str = include_str!("../../../fixtures/mig/explain/cat/valid-set.digest");
const BAD_FACTOR_JSON: &str = include_str!("../../../fixtures/mig/explain/cat/bad-factor.json");
const TOO_LARGE_JSON: &str = include_str!("../../../fixtures/mig/explain/cat/too-large.json");

fn human_caller() -> CallerContext {
    CallerContext { kind: PrincipalKind::Human, principal_id: "alice".into(), session: None }
}

fn agent_caller() -> CallerContext {
    CallerContext { kind: PrincipalKind::Agent, principal_id: "agent-1".into(), session: Some("sess-42".into()) }
}

/// AC-1 + REQ-001 + REQ-005: `valid-set.json` validates against `mig-factors@1`, is sorted
/// in exact catalogue order, and its digest equals `valid-set.digest`.
#[test]
fn ac1_valid_set_matches_committed_digest_and_catalogue_order() {
    let cat = FactorCatalogue::v1().expect("mig-factors@1 loads");
    let set = validate_candidate_set_json(&cat, &human_caller(), VALID_SET_JSON.as_bytes()).unwrap();
    assert_eq!(set.candidates.len(), 6);
    let factors: Vec<Factor> = set.candidates.iter().map(|c| c.factor).collect();
    assert_eq!(factors, Factor::ALL.to_vec());
    assert_eq!(set.digest.as_str(), VALID_SET_DIGEST.trim());
}

/// AC-2 + REQ-002 (error path): `bad-factor.json` (factor `PROBE_RADIUS`) fails with
/// `candidate_invalid` at index 0 naming `PROBE_RADIUS`; schema violations on valid factors
/// also fail with `candidate_invalid`.
#[test]
fn ac2_unknown_factor_and_bad_setting_fail_candidate_invalid() {
    let cat = FactorCatalogue::v1().unwrap();
    let err = validate_candidate_set_json(&cat, &human_caller(), BAD_FACTOR_JSON.as_bytes()).expect_err("must fail");
    assert_eq!(err.code(), "candidate_invalid");
    match err {
        CatError::CandidateInvalid { index, factor, .. } => {
            assert_eq!(index, 0);
            assert_eq!(factor, "PROBE_RADIUS");
        }
        other => panic!("unexpected error: {other:?}"),
    }

    // Setting failing ROUNDING schema (unknown extra key / bad rule).
    let bad_setting = br#"{"candidates":[{"factor":"ROUNDING","setting":{"decimals":2,"rule":"magic"},"source":"UserSelected","proposer":"alice"}]}"#;
    let err2 = validate_candidate_set_json(&cat, &human_caller(), bad_setting).expect_err("bad setting must fail");
    assert_eq!(err2.code(), "candidate_invalid");
    assert!(matches!(err2, CatError::CandidateInvalid { index: 0, ref factor, .. } if factor == "ROUNDING"));
}

/// AC-3 + REQ-003: Submitting `valid-set.json` as agent principal `agent-1` overrides every
/// candidate's source to `AgentProposed` with proposer `agent-1` and session `sess-42`,
/// regardless of what the caller claimed (`UserSelected` / `ExportStated`).
#[test]
fn ac3_agent_principal_forces_agent_proposed_on_every_candidate() {
    let cat = FactorCatalogue::v1().unwrap();
    let set = validate_candidate_set_json(&cat, &agent_caller(), VALID_SET_JSON.as_bytes()).unwrap();
    assert_eq!(set.candidates.len(), 6);
    for c in &set.candidates {
        assert_eq!(c.source, CandidateSource::AgentProposed);
        assert_eq!(c.proposer, "agent-1");
        assert_eq!(c.session.as_deref(), Some("sess-42"));
    }
}

/// AC-4 + REQ-005: Reordering the candidates in `valid-set.json` yields the exact same digest.
#[test]
fn ac4_reordered_candidates_yield_identical_digest() {
    let cat = FactorCatalogue::v1().unwrap();
    let s1 = validate_candidate_set_json(&cat, &human_caller(), VALID_SET_JSON.as_bytes()).unwrap();

    let CValue::Obj(mut root) = parse_with_limits(VALID_SET_JSON.as_bytes(), 64 * 1024, 16).unwrap() else {
        panic!("root obj")
    };
    let CValue::Arr(list) = root.get_mut("candidates").unwrap() else { panic!("arr") };
    list.reverse();
    let s2 = validate_candidate_set_value(&cat, &human_caller(), &CValue::Obj(root)).unwrap();

    assert_eq!(s1.digest, s2.digest);
    assert_eq!(s1.to_canonical_json().unwrap(), s2.to_canonical_json().unwrap());
}

/// AC-5 + REQ-004 (error path): A 65-candidate set fails with `candidate_set_too_large`; a
/// 64-candidate set succeeds.
#[test]
fn ac5_65_candidates_fails_too_large_and_64_succeeds() {
    let cat = FactorCatalogue::v1().unwrap();
    let err = validate_candidate_set_json(&cat, &human_caller(), TOO_LARGE_JSON.as_bytes()).expect_err("65 must fail");
    assert_eq!(err, CatError::CandidateSetTooLarge { count: 65 });
    assert_eq!(err.code(), "candidate_set_too_large");

    let CValue::Obj(mut root) = parse_with_limits(TOO_LARGE_JSON.as_bytes(), 256 * 1024, 16).unwrap() else {
        panic!("root obj")
    };
    let CValue::Arr(list) = root.get_mut("candidates").unwrap() else { panic!("arr") };
    list.truncate(MAX_CANDIDATES);
    let ok = validate_candidate_set_value(&cat, &human_caller(), &CValue::Obj(root)).unwrap();
    assert_eq!(ok.candidates.len(), MAX_CANDIDATES);
}
