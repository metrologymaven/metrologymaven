//! Integration tests for spec node `UNC.MODEL.SUBST.INPUT` (AC-1..AC-4,
//! REQ-UNC.MODEL.SUBST.INPUT-001..004, custom profiles, SI unit consistency, non-finite
//! rejection, multi-field condition mismatches, and D0 canonical digest determinism).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::dist::UnitCode;
use fid_unc::subst::{
    check, CharacteristicKind, InsufficientScope, ProcedureProfile, ReferenceSourceKind, SubstError, SubstitutionInput,
    DEFAULT_MIN_REPETITIONS_PER_RUN, DEFAULT_MIN_RUNS, DEFAULT_PROFILE_ID,
};
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/unc/model/subst")
}

fn load(name: &str) -> SubstitutionInput {
    SubstitutionInput::load_file(&fixture_dir().join(name)).unwrap_or_else(|e| panic!("load {name}: {e}"))
}

#[test]
fn ac1_ring_gauge_ok_and_bore_ok_return_checked_input() {
    let profile = ProcedureProfile::default_profile();
    assert_eq!(profile.id, DEFAULT_PROFILE_ID);
    assert_eq!(profile.min_runs, DEFAULT_MIN_RUNS);
    assert_eq!(profile.min_repetitions_per_run, DEFAULT_MIN_REPETITIONS_PER_RUN);

    let ring = load("ring_gauge_ok.json");
    let checked = check(&ring, &profile).expect("ring_gauge_ok checks cleanly");
    assert_eq!(checked.reference().value(), 0.0500012);
    assert_eq!(checked.reference().expanded_u(), 6.0e-7);
    assert_eq!(checked.reference().k(), 2.0);
    assert_eq!(checked.reference().unit(), UnitCode::Metre);
    assert_eq!(checked.reference().source_kind(), ReferenceSourceKind::Dcc);
    assert_eq!(checked.calibrated_runs().runs().len(), 20);
    assert_eq!(checked.calibrated_runs().total_observations(), 60);
    assert_eq!(checked.production_runs().runs().len(), 5);
    assert!(checked.uec3_eligible());

    // Deterministic canonical digest (D0)
    let d1 = checked.digest().expect("digest 1");
    let d2 = check(&ring, &profile).expect("recheck").digest().expect("digest 2");
    assert_eq!(d1, d2);

    // Second valid dataset (bore with certificate source)
    let bore = load("bore_ok.json");
    let checked_bore = check(&bore, &profile).expect("bore_ok checks cleanly");
    assert_eq!(checked_bore.reference().source_kind(), ReferenceSourceKind::Certificate);
    assert_eq!(checked_bore.reference().unit(), UnitCode::Metre);
    assert!(checked_bore.uec3_eligible());
    assert_ne!(d1, checked_bore.digest().expect("bore digest"));
}

#[test]
fn ac2_insufficient_runs_reports_required_and_supplied_and_site_override_works() {
    let input = load("ring_gauge_insufficient_runs.json");
    let default_profile = ProcedureProfile::default_profile();

    let err = check(&input, &default_profile).unwrap_err();
    assert_eq!(
        err,
        SubstError::InsufficientRuns {
            scope: InsufficientScope::CalibratedRunCount,
            required: 20,
            supplied: 5,
            run_index: None,
        }
    );
    assert_eq!(err.code(), "insufficient_runs");
    assert_eq!(err.to_string(), "insufficient_runs{required: 20, supplied: 5}");
    assert!(!err.uec3_eligible());

    // Site override profile with min_runs = 5 accepts the exact same dataset
    let site_profile = ProcedureProfile::custom("site-pilot-5x3", 5, 3).expect("custom profile");
    let checked = check(&input, &site_profile).expect("site profile accepts 5 runs");
    assert_eq!(checked.profile().id, "site-pilot-5x3");
    assert_eq!(checked.calibrated_runs().runs().len(), 5);

    // Insufficient repetitions inside a single calibrated run
    let mut short_rep = load("ring_gauge_ok.json");
    short_rep.calibrated_runs.as_mut().unwrap().runs[7].values.truncate(2);
    let rep_err = check(&short_rep, &default_profile).unwrap_err();
    assert_eq!(
        rep_err,
        SubstError::InsufficientRuns {
            scope: InsufficientScope::CalibratedRepetitions,
            required: 3,
            supplied: 2,
            run_index: Some(7),
        }
    );
    assert_eq!(rep_err.code(), "insufficient_runs");
}

#[test]
fn ac3_conditions_mismatch_names_fields_and_blocks_uec3() {
    let profile = ProcedureProfile::default_profile();
    let input = load("ring_gauge_strategy_mismatch.json");

    let err = check(&input, &profile).unwrap_err();
    assert_eq!(err, SubstError::ConditionsMismatch { fields: vec!["strategy_digest"], uec3_eligible: false });
    assert_eq!(err.code(), "conditions_mismatch");
    assert_eq!(err.to_string(), "conditions_mismatch{strategy_digest}");
    assert!(!err.uec3_eligible());

    // Multiple mismatched fields are reported in deterministic declaration order
    let mut multi = load("ring_gauge_ok.json");
    let pc = &mut multi.production_runs.as_mut().unwrap().conditions;
    pc.feature_ref = "other-feature".to_string();
    pc.machine_id = "cmm-lab-99".to_string();
    let merr = check(&multi, &profile).unwrap_err();
    assert_eq!(
        merr,
        SubstError::ConditionsMismatch { fields: vec!["feature_ref", "machine_id"], uec3_eligible: false }
    );
    assert_eq!(merr.to_string(), "conditions_mismatch{feature_ref,machine_id}");
}

#[test]
fn ac4_missing_k_and_other_required_fields_fail_with_input_missing() {
    let profile = ProcedureProfile::default_profile();
    let missing_k = load("ring_gauge_missing_k.json");

    let err = check(&missing_k, &profile).unwrap_err();
    assert_eq!(err, SubstError::InputMissing { field: "k".to_string() });
    assert_eq!(err.code(), "input_missing");
    assert_eq!(err.to_string(), "input_missing{k}");

    let base = load("ring_gauge_ok.json");

    let mut no_val = base.clone();
    no_val.reference.value = None;
    assert_eq!(check(&no_val, &profile).unwrap_err(), SubstError::InputMissing { field: "value".to_string() });

    let mut no_unit = base.clone();
    no_unit.reference.unit = None;
    assert_eq!(check(&no_unit, &profile).unwrap_err(), SubstError::InputMissing { field: "unit".to_string() });

    let mut no_u = base.clone();
    no_u.reference.expanded_u = None;
    assert_eq!(check(&no_u, &profile).unwrap_err(), SubstError::InputMissing { field: "u".to_string() });

    let mut no_sk = base.clone();
    no_sk.reference.source_kind = None;
    assert_eq!(check(&no_sk, &profile).unwrap_err(), SubstError::InputMissing { field: "source_kind".to_string() });

    let mut no_sd = base.clone();
    no_sd.reference.source_digest = None;
    assert_eq!(check(&no_sd, &profile).unwrap_err(), SubstError::InputMissing { field: "source_digest".to_string() });

    let mut no_cal = base.clone();
    no_cal.calibrated_runs = None;
    assert_eq!(
        check(&no_cal, &profile).unwrap_err(),
        SubstError::InputMissing { field: "calibrated_runs".to_string() }
    );

    let mut no_prod = base;
    no_prod.production_runs = None;
    assert_eq!(
        check(&no_prod, &profile).unwrap_err(),
        SubstError::InputMissing { field: "production_runs".to_string() }
    );
}

#[test]
fn req004_si_units_unit_consistency_and_finiteness_enforced() {
    let profile = ProcedureProfile::default_profile();
    let base = load("ring_gauge_ok.json");

    // Non-SI unit string in JSON rejected fail-closed as invalid_value
    let raw = std::fs::read_to_string(fixture_dir().join("ring_gauge_ok.json")).expect("read");
    let non_si = raw.replacen("\"si:m\"", "\"mm\"", 1);
    let err = SubstitutionInput::from_json(non_si.as_bytes()).unwrap_err();
    assert_eq!(err.code(), "invalid_value");

    // Unit inconsistent with characteristic kind (si:rad for diameter)
    let mut wrong_unit = base.clone();
    wrong_unit.reference.unit = Some(UnitCode::Radian);
    let err = check(&wrong_unit, &profile).unwrap_err();
    assert_eq!(err.code(), "invalid_value");

    // Angular characteristic requires si:rad (not si:m)
    let mut angle_with_m = base.clone();
    angle_with_m.calibrated_runs.as_mut().unwrap().conditions.characteristic_kind = CharacteristicKind::Angle;
    angle_with_m.production_runs.as_mut().unwrap().conditions.characteristic_kind = CharacteristicKind::Angle;
    assert_eq!(check(&angle_with_m, &profile).unwrap_err().code(), "invalid_value");

    // Angular characteristic with si:rad everywhere succeeds
    angle_with_m.reference.unit = Some(UnitCode::Radian);
    angle_with_m.calibrated_runs.as_mut().unwrap().unit = Some(UnitCode::Radian);
    angle_with_m.production_runs.as_mut().unwrap().unit = Some(UnitCode::Radian);
    assert!(check(&angle_with_m, &profile).is_ok());

    // Non-finite values in reference or run observations fail with invalid_value
    let mut nan_ref = base.clone();
    nan_ref.reference.value = Some(f64::NAN);
    assert_eq!(check(&nan_ref, &profile).unwrap_err().code(), "invalid_value");

    let mut inf_obs = base.clone();
    inf_obs.calibrated_runs.as_mut().unwrap().runs[0].values[1] = f64::INFINITY;
    assert_eq!(check(&inf_obs, &profile).unwrap_err().code(), "invalid_value");

    // Non-positive U or k fails with invalid_value
    let mut zero_u = base.clone();
    zero_u.reference.expanded_u = Some(0.0);
    assert_eq!(check(&zero_u, &profile).unwrap_err().code(), "invalid_value");

    let mut neg_k = base;
    neg_k.reference.k = Some(-2.0);
    assert_eq!(check(&neg_k, &profile).unwrap_err().code(), "invalid_value");
}
