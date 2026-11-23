//! Acceptance tests for `ALN.BFIT.STOP` (`AC-1`..`AC-5`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::stop::{
    command_convergence_manifest, AlignmentStopStatus, StopCriteria, StopError, StopMonitor, StoppedReason, Verdict,
    BESTFIT_STOP_CRITERIA_MANIFEST,
};
use fid_testkit::WORKSPACE_ROOT;
use std::path::Path;

/// AC-1 & AC-2: Every scripted trace in `fixtures/aln/bfit/stop-traces.json` yields its
/// expected verdict, stopping iteration, best-pose iteration and alignment status.
#[test]
fn ac1_and_ac2_stop_traces_fixture_verdicts_and_best_pose() {
    let raw = std::fs::read_to_string(Path::new(WORKSPACE_ROOT).join("fixtures/aln/bfit/stop-traces.json")).unwrap();
    // Minimal deterministic JSON trace check without pulling serde into the kernel test.
    let traces = parse_traces(&raw);
    assert_eq!(traces.len(), 5);

    for tr in &traces {
        let mut mon = StopMonitor::new(StopCriteria::default()).unwrap();
        mon.seed_initial(tr.initial_objective).unwrap();
        let mut final_verdict = Verdict::Continue;
        let mut final_iter = 0u32;
        for step in &tr.steps {
            final_iter = step.iter;
            final_verdict = mon.update(step.iter, step.dt, step.dr, step.objective).unwrap();
            if final_verdict != Verdict::Continue {
                break;
            }
        }
        assert_eq!(final_verdict.as_str(), tr.expected_verdict, "trace {}", tr.name);
        assert_eq!(final_iter, tr.expected_iteration, "trace {}", tr.name);
        let (best_iter, _) = mon.best().expect("best recorded");
        assert_eq!(best_iter, tr.expected_best_iteration, "trace {}", tr.name);
        let status = mon.alignment_status(final_verdict).expect("terminal status");
        assert_eq!(status.status_str(), tr.expected_status, "trace {}", tr.name);
        assert!(final_verdict.to_stopped_by().is_some(), "bridge to ALN.QA.HIST StoppedBy");
    }
}

/// AC-3: A trace that never converges stops at iteration 100 with `stopped(max_iterations)`
/// and maps to `not_converged` carrying the lowest-objective iteration.
#[test]
fn ac3_non_converging_trace_stops_at_max_iterations_100_as_not_converged() {
    let mut mon = StopMonitor::new(StopCriteria::default()).unwrap();
    let mut verdict = Verdict::Continue;
    let mut stop_iter = 0u32;
    for iter in 1..=120u32 {
        // Slowly decreasing objective (never below rel tol 1e-12, never rising).
        let obj = 100.0 / f64::from(iter);
        verdict = mon.update(iter, 1e-4, 1e-4, obj).unwrap();
        stop_iter = iter;
        if verdict != Verdict::Continue {
            break;
        }
    }
    assert_eq!(stop_iter, 100);
    assert_eq!(verdict, Verdict::Stopped(StoppedReason::MaxIterations));
    assert_eq!(
        mon.alignment_status(verdict),
        Some(AlignmentStopStatus::NotConverged { reason: StoppedReason::MaxIterations, best_iteration: 100 })
    );
}

/// AC-4 (error path): Negative, NaN, zero/excessive criteria fail with `invalid_stop_criteria`
/// naming the exact field; NaN in iteration fails with `nan_in_iteration`.
#[test]
fn ac4_invalid_criteria_and_nan_in_iteration_errors() {
    let bad_t = StopCriteria { translation_tol: -1.0, ..StopCriteria::default() };
    assert_eq!(bad_t.validate(), Err(StopError::InvalidStopCriteria { field: "translation_tol" }));

    let bad_r = StopCriteria { rotation_tol: f64::NAN, ..StopCriteria::default() };
    assert_eq!(bad_r.validate(), Err(StopError::InvalidStopCriteria { field: "rotation_tol" }));

    let bad_o = StopCriteria { objective_rel_tol: f64::INFINITY, ..StopCriteria::default() };
    assert_eq!(bad_o.validate(), Err(StopError::InvalidStopCriteria { field: "objective_rel_tol" }));

    let bad_zero = StopCriteria { max_iterations: 0, ..StopCriteria::default() };
    assert_eq!(bad_zero.validate(), Err(StopError::InvalidStopCriteria { field: "max_iterations" }));

    let bad_hi = StopCriteria { max_iterations: 100_001, ..StopCriteria::default() };
    assert_eq!(bad_hi.validate(), Err(StopError::InvalidStopCriteria { field: "max_iterations" }));

    let bad_div = StopCriteria { divergence_window: 0, ..StopCriteria::default() };
    assert_eq!(bad_div.validate(), Err(StopError::InvalidStopCriteria { field: "divergence_window" }));

    let mut mon = StopMonitor::new(StopCriteria::default()).unwrap();
    assert_eq!(mon.update(1, f64::NAN, 0.0, 1.0), Err(StopError::NanInIteration { field: "dt" }));
    assert_eq!(mon.update(1, 0.0, -1e-6, 1.0), Err(StopError::NanInIteration { field: "dr" }));
    assert_eq!(mon.update(1, 0.0, 0.0, f64::NAN), Err(StopError::NanInIteration { field: "objective" }));
}

/// AC-5: Registry manifest of `align.run_bestfit@1` declares all five stopping criteria with
/// their units and defaults.
#[test]
fn ac5_registry_manifest_lists_all_five_criteria_with_units_and_defaults() {
    assert_eq!(BESTFIT_STOP_CRITERIA_MANIFEST.len(), 5);
    let expected = [
        ("max_iterations", "1", "100"),
        ("translation_tol", "m", "1e-9"),
        ("rotation_tol", "rad", "1e-9"),
        ("objective_rel_tol", "1", "1e-12"),
        ("divergence_window", "1", "5"),
    ];
    for (entry, &(name, unit, default)) in BESTFIT_STOP_CRITERIA_MANIFEST.iter().zip(expected.iter()) {
        assert_eq!(entry.name, name);
        assert_eq!(entry.unit, unit);
        assert_eq!(entry.default, default);
        assert!(!entry.meaning.is_empty());
    }
    let manifest_json = command_convergence_manifest("align.run_bestfit", 1);
    assert!(manifest_json.contains(r#""id":"align.run_bestfit""#));
    assert!(manifest_json.contains(r#""iterative":true"#));
    for &(name, unit, default) in &expected {
        assert!(manifest_json.contains(&format!(r#""name":"{name}""#)));
        assert!(manifest_json.contains(&format!(r#""unit":"{unit}""#)));
        assert!(manifest_json.contains(&format!(r#""default":"{default}""#)));
    }
}

// ---- Minimal dependency-free parser for fixtures/aln/bfit/stop-traces.json ----

struct Trace {
    name: String,
    initial_objective: f64,
    steps: Vec<Step>,
    expected_verdict: String,
    expected_iteration: u32,
    expected_best_iteration: u32,
    expected_status: String,
}

struct Step {
    iter: u32,
    dt: f64,
    dr: f64,
    objective: f64,
}

fn parse_traces(raw: &str) -> Vec<Trace> {
    let mut out = Vec::new();
    let chunks: Vec<&str> = raw.split(r#""name": ""#).skip(1).collect();
    for chunk in chunks {
        let name = chunk.split('"').next().unwrap().to_string();
        let initial_objective = extract_num(chunk, r#""initial_objective": "#);
        let expected_verdict = extract_str(chunk, r#""expected_verdict": ""#);
        let expected_iteration = extract_num(chunk, r#""expected_iteration": "#) as u32;
        let expected_best_iteration = extract_num(chunk, r#""expected_best_iteration": "#) as u32;
        let expected_status = extract_str(chunk, r#""expected_status": ""#);

        let steps_start = chunk.find(r#""steps": ["#).unwrap();
        let steps_end = chunk[steps_start..].find(']').unwrap() + steps_start;
        let steps_str = &chunk[steps_start..steps_end];
        let mut steps = Vec::new();
        for s in steps_str.split(r#""iter": "#).skip(1) {
            let iter = s.split(',').next().unwrap().trim().parse::<u32>().unwrap();
            let dt = extract_num(s, r#""dt": "#);
            let dr = extract_num(s, r#""dr": "#);
            let objective = extract_num(s, r#""objective": "#);
            steps.push(Step { iter, dt, dr, objective });
        }
        out.push(Trace {
            name,
            initial_objective,
            steps,
            expected_verdict,
            expected_iteration,
            expected_best_iteration,
            expected_status,
        });
    }
    out
}

fn extract_str(hay: &str, key: &str) -> String {
    let start = hay.find(key).unwrap() + key.len();
    hay[start..].split('"').next().unwrap().to_string()
}

fn extract_num(hay: &str, key: &str) -> f64 {
    let start = hay.find(key).unwrap() + key.len();
    let tok = hay[start..].split([',', '}', '\n', ' ']).next().unwrap();
    tok.parse::<f64>().unwrap()
}
