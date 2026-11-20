//! Crate-internal tests: the test-only limits of AC-4 (no public entry point takes limits,
//! REQ-KRN.SOLV.CONIC.WRAP-003) and the settings/descriptor agreement.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

const FIXTURE: &str = include_str!("../../../../fixtures/solv/wrap/small-v1/problems.txt");

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

fn usizes(s: &str) -> Vec<usize> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

/// The circle-socp problem of `fixtures/solv/wrap/small-v1` (AC-1's SOCP).
fn circle_socp() -> ConicProblem {
    let mut lines = FIXTURE.lines().skip_while(|l| !l.starts_with("problem circle-socp "));
    let mut get = |key: &str| -> String {
        let l = lines.find(|l| l.starts_with(&format!("{key} "))).unwrap();
        l[key.len() + 1..].to_string()
    };
    let n: usize = get("n").parse().unwrap();
    let q = hexes(&get("q"));
    let m: usize = get("m").parse().unwrap();
    let a = Csc::new(m, n, usizes(&get("a_colptr")), usizes(&get("a_rowval")), hexes(&get("a_nzval")));
    let b = hexes(&get("b"));
    let toks: Vec<String> = get("cones").split_whitespace().map(String::from).collect();
    let cones = toks
        .chunks(2)
        .map(|c| {
            assert_eq!(c[0], "soc");
            Cone::Soc(c[1].parse().unwrap())
        })
        .collect();
    ConicProblemBuilder::new(q, a, b, cones).build().unwrap()
}

/// AC-4: a test-only limits set with max iterations 2 on AC-1's SOCP.
#[test]
fn ac4_iteration_limit() {
    let problem = circle_socp();
    let limits = ConicLimits { max_iter: 2, ..ConicLimits::FIXED };
    match solve_with(&problem, &limits) {
        Err(ConicError::Solver(e)) => {
            assert_eq!(e.cause, ConicCause::IterationLimit);
            assert_eq!(e.iterations, 2);
            assert!(e.primal_residual.is_some() && e.dual_residual.is_some() && e.gap.is_some());
        }
        other => panic!("expected IterationLimit, got {other:?}"),
    }
    // The fixed limits solve it.
    assert!(solve_socp(&problem).is_ok());
}

/// The settings the solver runs with carry every ConicLimits value.
#[test]
fn settings_carry_limits() {
    let l = ConicLimits::FIXED;
    let s = limits::settings(&l);
    assert_eq!(s.max_iter, l.max_iter);
    assert_eq!(s.tol_feas.to_bits(), l.tol_feas.to_bits());
    assert_eq!(s.tol_gap_abs.to_bits(), l.tol_gap_abs.to_bits());
    assert_eq!(s.tol_gap_rel.to_bits(), l.tol_gap_rel.to_bits());
    assert_eq!(s.tol_infeas_abs.to_bits(), l.tol_infeas.to_bits());
    assert_eq!(s.tol_infeas_rel.to_bits(), l.tol_infeas.to_bits());
    assert_eq!(s.static_regularization_constant.to_bits(), l.static_reg.to_bits());
    assert_eq!(s.iterative_refinement_max_iter, u32::from(l.refine_steps));
    assert_eq!(s.direct_solve_method, "qdldl");
    assert!(!s.presolve_enable);
}

/// A changed limit changes the descriptor (so the manifest digest records it).
#[test]
fn descriptor_follows_limits() {
    let a = limits::descriptor_of(&ConicLimits::FIXED).canonical_json();
    let b = limits::descriptor_of(&ConicLimits { max_iter: 2, ..ConicLimits::FIXED }).canonical_json();
    assert_ne!(a, b);
    assert_eq!(a, limits_descriptor().canonical_json());
}
