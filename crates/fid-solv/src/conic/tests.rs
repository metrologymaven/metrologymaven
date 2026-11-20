//! Crate-internal tests: the test-only limits of AC-4 (no public entry point takes limits,
//! REQ-KRN.SOLV.CONIC.WRAP-003) and the settings/descriptor agreement.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

/// The circle-socp block of `fixtures/solv/wrap/small-v1/problems.txt` (AC-1's SOCP), line
/// for line. Copied rather than included: the kernel's implementation-digest scan
/// (KRN.MAN.MANI.IMPL) refuses `include_str!` of files outside the crate's hashed set.
/// `make_fixtures.py --check` fails if any of these lines is not in the fixture's block.
const CIRCLE_SOCP: [&str; 8] = [
    "n 3",
    "q 0000000000000000 0000000000000000 3ff0000000000000",
    "m 24",
    "a_colptr 0 8 16 24",
    "a_rowval 1 4 7 10 13 16 19 22 2 5 8 11 14 17 20 23 0 3 6 9 12 15 18 21",
    "a_nzval 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 3ff0000000000000 bff0000000000000 bff0000000000000 bff0000000000000 bff0000000000000 bff0000000000000 bff0000000000000 bff0000000000000 bff0000000000000",
    "b 0000000000000000 c006a00000000000 4003f00000000000 0000000000000000 400ac80000000000 c001e00000000000 0000000000000000 c007780000000000 c00d400000000000 0000000000000000 3fe6000000000000 c00a880000000000 0000000000000000 c00d500000000000 bffa800000000000 0000000000000000 bf9c000000000000 3ff6700000000000 0000000000000000 400b600000000000 3ffc800000000000 0000000000000000 bff3e00000000000 bff6f00000000000",
    "cones soc 3 soc 3 soc 3 soc 3 soc 3 soc 3 soc 3 soc 3",
];

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

fn usizes(s: &str) -> Vec<usize> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

/// The circle-socp problem of `fixtures/solv/wrap/small-v1`.
fn circle_socp() -> ConicProblem {
    let get = |key: &str| -> String {
        let l = CIRCLE_SOCP.iter().find(|l| l.starts_with(&format!("{key} "))).unwrap();
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

/// Every final solver status maps to its cause; only `Solved` gives no cause. The statuses a
/// demotion can replace (Solved, AlmostSolved and the four infeasibility statuses) become
/// NumericalError in `solution.status`, which maps to `Numerical`.
#[test]
fn cause_of_every_status() {
    use fid_clarabel::solver::SolverStatus as S;
    let table = [
        (S::Unsolved, Some(ConicCause::NotRun)),
        (S::Solved, None),
        (S::PrimalInfeasible, Some(ConicCause::PrimalInfeasible)),
        (S::DualInfeasible, Some(ConicCause::DualInfeasible)),
        (S::AlmostSolved, Some(ConicCause::AlmostSolved)),
        (S::AlmostPrimalInfeasible, Some(ConicCause::AlmostPrimalInfeasible)),
        (S::AlmostDualInfeasible, Some(ConicCause::AlmostDualInfeasible)),
        (S::MaxIterations, Some(ConicCause::IterationLimit)),
        (S::NumericalError, Some(ConicCause::Numerical)),
        (S::InsufficientProgress, Some(ConicCause::InsufficientProgress)),
    ];
    for (status, cause) in table {
        assert_eq!(cause_of(status), cause, "{status:?}");
    }
    // the table is complete: one row per status, by the enum's discriminants 0..=9
    let mut seen: Vec<i32> = table.iter().map(|(s, _)| *s as i32).collect();
    seen.sort_unstable();
    assert_eq!(seen, (0..=9).collect::<Vec<i32>>());
}
