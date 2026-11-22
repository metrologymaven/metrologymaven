//! Acceptance tests for `KRN.MAN.VAL.RUN` (`spec/krn/man/val/run/NODE.md`):
//! AC-1 (val-toy-v1 matches expected_results.json incl. failing case),
//! AC-2 (direction-flipped plane and shifted in-plane / along-axis normalisation),
//! AC-3 (missing criteria file -> MissingCriteria{set}),
//! AC-4 (byte-identical canonical JSON across repeated and shuffled runs),
//! REQ-003 (no_mapping & kernel_error{code} not_run outcomes),
//! AC-H5 (2x-limit seeded fault injection: 100% detection, 0 false alarms).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_val::{
    load_criteria_dir, load_mapping_table, load_release, run_validation, CaseOutcome, CommandArtifactRunner,
    CriteriaSet, NotRunReason, ParamValue, ReferenceParam, Release, ReleaseCase, ValError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/val-toy-v1")
}

fn toy_runner() -> CommandArtifactRunner {
    CommandArtifactRunner {
        program: env!("CARGO_BIN_EXE_val-toy-artifact").to_string(),
        args: Vec::new(),
        timeout: Duration::from_secs(10),
    }
}

/// AC-1: Running the harness on `fixtures/man/val-toy-v1` (12 cases: 6 planes incl. 1 failing,
/// 6 circles) produces canonical JSON bytes identical to committed `expected_results.json`.
#[test]
fn ac1_val_toy_v1_matches_expected_results_with_deliberate_failure() {
    let dir = fixture_dir();
    let release = load_release(&dir.join("release.json")).unwrap();
    let mappings = load_mapping_table(&dir.join("mappings.toml")).unwrap();
    let criteria = load_criteria_dir(&dir.join("criteria")).unwrap();

    let results = run_validation(&release, &mappings, &criteria, &mut toy_runner()).unwrap();
    let got_bytes = results.to_canonical_json().unwrap();
    let expected_bytes = std::fs::read(dir.join("expected_results.json")).unwrap();
    assert_eq!(got_bytes, expected_bytes, "RunResults canonical bytes drifted from expected_results.json");

    assert_eq!(results.cases.len(), 12);
    let fail = results.cases.iter().find(|c| c.case_id == "plane-06-fail").expect("plane-06-fail present");
    assert_eq!(fail.outcome, CaseOutcome::Failed);
    let pt_dev = fail.deviations["point"];
    assert!((pt_dev - 0.005).abs() < 1e-12, "expected 0.005 m deviation, got {pt_dev}");

    let plane_sum = results.summaries.iter().find(|s| s.primitive == "plane").unwrap();
    assert_eq!((plane_sum.passed, plane_sum.failed, plane_sum.not_run), (5, 1, 0));
    let circle_sum = results.summaries.iter().find(|s| s.primitive == "circle").unwrap();
    assert_eq!((circle_sum.passed, circle_sum.failed, circle_sum.not_run), (6, 0, 0));
}

/// AC-2 + REQ-004: Flipping a reference plane's normal (`n -> -n`) AND sliding its reference
/// point along the plane still passes with near-zero angular and offset deviations.
#[test]
fn ac2_flipped_reference_plane_normal_and_in_plane_shift_still_pass() {
    let dir = fixture_dir();
    let mut release = load_release(&dir.join("release.json")).unwrap();
    let mappings = load_mapping_table(&dir.join("mappings.toml")).unwrap();
    let criteria = load_criteria_dir(&dir.join("criteria")).unwrap();

    // Keep only plane-01, flip its normal (0,0,1) -> (0,0,-1) and slide point by (+10, -25, 0) in plane.
    release.cases.retain(|c| c.case_id == "plane-01");
    let c = &mut release.cases[0];
    let ParamValue::Vec3(n) = c.reference["normal"].value else { panic!("vec3 normal") };
    c.reference.get_mut("normal").unwrap().value = ParamValue::Vec3([-n[0], -n[1], -n[2]]);
    let ParamValue::Vec3(p) = c.reference["point"].value else { panic!("vec3 point") };
    c.reference.get_mut("point").unwrap().value = ParamValue::Vec3([p[0] + 10.0, p[1] - 25.0, p[2]]);

    let results = run_validation(&release, &mappings, &criteria, &mut toy_runner()).unwrap();
    assert_eq!(results.cases.len(), 1);
    assert_eq!(results.cases[0].outcome, CaseOutcome::Passed);
    assert!(results.cases[0].deviations["normal"] <= 1e-12);
    assert!(results.cases[0].deviations["point"] <= 1e-12);
}

/// AC-3 (error path) + REQ-002: Missing criteria file for a set present in the release fails
/// with `ValError::MissingCriteria { set }`.
#[test]
fn ac3_missing_criteria_fails_closed() {
    let dir = fixture_dir();
    let release = load_release(&dir.join("release.json")).unwrap();
    let mappings = load_mapping_table(&dir.join("mappings.toml")).unwrap();
    let empty_criteria = CriteriaSet::default();

    let err = run_validation(&release, &mappings, &empty_criteria, &mut toy_runner()).expect_err("must fail");
    assert_eq!(err, ValError::MissingCriteria { set: "toy-set-1.0".into() });
    assert_eq!(err.code(), "missing_criteria");
}

/// AC-4 + REQ-005: Repeated runs and runs with reversed case input order produce byte-identical
/// canonical JSON encodings and identical SHA-256 digests.
#[test]
fn ac4_canonical_encoding_is_byte_identical_across_runs_and_input_order() {
    let dir = fixture_dir();
    let release1 = load_release(&dir.join("release.json")).unwrap();
    let mut release2 = release1.clone();
    release2.cases.reverse();
    let mappings = load_mapping_table(&dir.join("mappings.toml")).unwrap();
    let criteria = load_criteria_dir(&dir.join("criteria")).unwrap();

    let r1 = run_validation(&release1, &mappings, &criteria, &mut toy_runner()).unwrap();
    let r2 = run_validation(&release2, &mappings, &criteria, &mut toy_runner()).unwrap();
    assert_eq!(r1.to_canonical_json().unwrap(), r2.to_canonical_json().unwrap());
    assert_eq!(r1.digest().unwrap(), r2.digest().unwrap());
}

/// REQ-003: Unmapped cases record `not_run` with `no_mapping`, and kernel error responses
/// record `not_run` with `kernel_error{code}`.
#[test]
fn req003_records_no_mapping_and_kernel_error_reasons() {
    let dir = fixture_dir();
    let base = load_release(&dir.join("release.json")).unwrap();
    let mappings = load_mapping_table(&dir.join("mappings.toml")).unwrap();
    let criteria = load_criteria_dir(&dir.join("criteria")).unwrap();

    let unmapped = ReleaseCase {
        case_id: "cone-unmapped".into(),
        set: "toy-set".into(),
        set_version: "1.0".into(),
        primitive: "cone".into(),
        case_type: "ls".into(),
        points: vec![[0.0, 0.0, 0.0]],
        reference: BTreeMap::new(),
    };
    let too_few_pts = ReleaseCase {
        case_id: "plane-2pts".into(),
        set: "toy-set".into(),
        set_version: "1.0".into(),
        primitive: "plane".into(),
        case_type: "ls".into(),
        points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        reference: BTreeMap::from([
            ("point".into(), ReferenceParam { value: ParamValue::Vec3([0.0, 0.0, 0.0]), uncertainty: 0.0 }),
            ("normal".into(), ReferenceParam { value: ParamValue::Vec3([0.0, 0.0, 1.0]), uncertainty: 0.0 }),
            ("rms".into(), ReferenceParam { value: ParamValue::Scalar(0.0), uncertainty: 0.0 }),
        ]),
    };

    let release =
        Release { release_id: base.release_id, suite_digest: base.suite_digest, cases: vec![unmapped, too_few_pts] };

    let results = run_validation(&release, &mappings, &criteria, &mut toy_runner()).unwrap();
    let c_unmapped = results.cases.iter().find(|c| c.case_id == "cone-unmapped").unwrap();
    assert_eq!(c_unmapped.outcome, CaseOutcome::NotRun);
    assert_eq!(c_unmapped.reason, Some(NotRunReason::NoMapping));

    let c_kerr = results.cases.iter().find(|c| c.case_id == "plane-2pts").unwrap();
    assert_eq!(c_kerr.outcome, CaseOutcome::NotRun);
    assert_eq!(c_kerr.reason, Some(NotRunReason::KernelError { code: "insufficient_points".into() }));
}

/// AC-H5: Seeded fault injection on clean cases (half shifted by 2 × criteria limit) yields
/// 100% fault detection and 0 false alarms.
#[test]
fn ach5_seeded_2x_limit_fault_injection_has_100_percent_detection_and_zero_false_alarms() {
    let dir = fixture_dir();
    let mut release = load_release(&dir.join("release.json")).unwrap();
    let mappings = load_mapping_table(&dir.join("mappings.toml")).unwrap();
    let criteria = load_criteria_dir(&dir.join("criteria")).unwrap();
    let crit = criteria.get("toy-set", "1.0").unwrap();

    // Keep the 11 clean passing cases.
    release.cases.retain(|c| c.case_id != "plane-06-fail");
    assert_eq!(release.cases.len(), 11);

    // Deterministically shift 5 of the 11 cases by 2 × their parameter limit.
    let mut shifted_ids: BTreeSet<String> = BTreeSet::new();
    for (idx, case) in release.cases.iter_mut().enumerate() {
        if idx % 2 == 0 {
            shifted_ids.insert(case.case_id.clone());
            if case.primitive == "plane" {
                let lim = crit.per_parameter_limits["point"];
                let ParamValue::Vec3(n) = case.reference["normal"].value else { panic!("n") };
                let ParamValue::Vec3(p) = case.reference["point"].value else { panic!("p") };
                let shift = 2.0 * lim;
                case.reference.get_mut("point").unwrap().value =
                    ParamValue::Vec3([p[0] + shift * n[0], p[1] + shift * n[1], p[2] + shift * n[2]]);
            } else {
                let lim = crit.per_parameter_limits["radius"];
                let ParamValue::Scalar(r) = case.reference["radius"].value else { panic!("r") };
                case.reference.get_mut("radius").unwrap().value = ParamValue::Scalar(r + 2.0 * lim);
            }
        }
    }

    let results = run_validation(&release, &mappings, &criteria, &mut toy_runner()).unwrap();
    let failed_ids: BTreeSet<String> =
        results.cases.iter().filter(|c| c.outcome == CaseOutcome::Failed).map(|c| c.case_id.clone()).collect();
    assert_eq!(failed_ids, shifted_ids, "fault detection mismatch");
}
