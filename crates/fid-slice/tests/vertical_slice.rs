//! Integration tests proving Vertical Slice 0 end-to-end:
//! KNP-1 fitting -> 3-2-1 DRF -> GD&T tolerance zones -> side-preserving formatting ->
//! hash-chained canonical journal -> deterministic ZIP Result Package -> fail-closed verify.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fid_kdec::Outcome;
use fid_slice::{demo_job, run_slice, synth_bore_points, verify_package_bytes, JobSpec};

#[test]
fn demo_pipeline_end_to_end_verifies_and_conforms() {
    let job = demo_job();
    let out = run_slice(&job).expect("demo vertical slice must succeed");

    assert_eq!(out.characteristics.len(), 4);
    assert_eq!(out.verified_members, 6);
    assert_eq!(out.journal.len(), 5);
    for c in &out.characteristics {
        assert_eq!(c.outcome, Outcome::Conforms, "expected {} to conform, got {:?}", c.label, c.outcome);
        assert_eq!(c.formatter_id, "fmt.length@1");
        assert_eq!(c.evaluation_basis, "POINTS");
    }
    assert!(out.form3_csv.contains("MM-BRKT-001"));
    assert!(out.form3_csv.contains("CHAR-003"));
}

#[test]
fn package_is_byte_identical_across_runs_d0() {
    let job = demo_job();
    let a = run_slice(&job).expect("run 1");
    let b = run_slice(&job).expect("run 2");

    assert_eq!(a.journal_head, b.journal_head);
    assert_eq!(a.package_digest, b.package_digest);
    assert_eq!(a.package_bytes, b.package_bytes);
}

#[test]
fn tampered_package_fails_closed() {
    let out = run_slice(&demo_job()).expect("demo run");
    let mut tampered = out.package_bytes.clone();
    // Flip a byte inside a stored/deflated member payload well before the central directory.
    let idx = tampered.len() / 2;
    tampered[idx] ^= 0xFF;

    let err = verify_package_bytes(&tampered).expect_err("tampered package must fail verification");
    assert_eq!(err.code, "slice.verify");
}

#[test]
fn shifted_bore_detects_nonconforming_true_position() {
    let mut job = demo_job();
    // Shift bore X by +0.040 mm -> radial offset 40 um -> true position diameter >= 0.080 mm (> 0.050 mm limit).
    job.bore_points = synth_bore_points([0.050_040, 0.040_000], 0.010_000, 0.0, 0.000_001);
    let out = run_slice(&job).expect("shifted bore slice");

    let pos = out.characteristics.iter().find(|c| c.balloon == 3).expect("balloon 3 present");
    assert_eq!(pos.outcome, Outcome::Nonconforms);
    assert_eq!(pos.side, "above");
}

#[test]
fn json_job_round_trip_matches_demo_digest() {
    let job = demo_job();
    let bytes = fid_canon::to_canonical_bytes(&job.to_cvalue().unwrap()).unwrap();
    let parsed = JobSpec::from_json(&bytes).expect("parse job JSON");

    let a = run_slice(&job).unwrap();
    let b = run_slice(&parsed).unwrap();
    assert_eq!(a.package_digest, b.package_digest);
}
