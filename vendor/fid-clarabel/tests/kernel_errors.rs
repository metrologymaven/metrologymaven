//! KRN.SOLV.CONIC.PATCH review fixes: no reported result without finite values of record,
//! no panic from the public entry points, finite data updates, and the size budget of the
//! minimum-degree ordering.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(non_snake_case)]

use fid_clarabel::algebra::*;
use fid_clarabel::qdldl::{min_degree_ordering, QDLDLError, MAX_ORDERING_DIM, MAX_ORDERING_NNZ};
use fid_clarabel::solver::*;

fn claims_result(s: SolverStatus) -> bool {
    matches!(
        s,
        SolverStatus::Solved
            | SolverStatus::AlmostSolved
            | SolverStatus::PrimalInfeasible
            | SolverStatus::DualInfeasible
            | SolverStatus::AlmostPrimalInfeasible
            | SolverStatus::AlmostDualInfeasible
    )
}

fn diag2(v: f64) -> CscMatrix {
    CscMatrix::new(2, 2, vec![0, 1, 2], vec![0, 1], vec![v, v])
}

fn solve_scaled(q: f64, p: f64, equilibrate: bool) -> DefaultSolver {
    let settings = DefaultSettings { equilibrate_enable: equilibrate, ..DefaultSettings::default() };
    let mut solver =
        DefaultSolver::new(&diag2(p), &[q, -q], &diag2(1.0), &[1.0, 1.0], &[NonnegativeConeT(2)], settings).unwrap();
    solver.solve();
    solver
}

#[test]
fn non_finite_mu_ends_with_numerical_error_and_no_result() {
    // q = 1e308: the starting point's duality gap overflows, fid_num::red reports it, the
    // facade turns it into NaN, and the non-finite-mu guard stops at iteration 0.
    for eq in [true, false] {
        let s = solve_scaled(1e308, 1.0, eq);
        assert!(s.info.mu.is_nan(), "the guard path was not taken (mu = {})", s.info.mu);
        assert_eq!(s.solution.status, SolverStatus::NumericalError);
        assert!(s.solution.obj_val.is_nan());
        // the reset info is NaN, so no "almost" check could have passed
        assert!(s.info.cost_primal.is_nan() && s.info.gap_abs.is_nan());
    }
}

#[test]
fn extreme_scales_never_report_an_unbacked_result() {
    for e in (100..=308).step_by(4) {
        for m in [1.0, 3.0, 7.0] {
            let v = m * 10f64.powi(e - 1).min(f64::MAX / 10.0);
            for (q, p) in [(v, 1.0), (v, v), (1.0, v)] {
                for eq in [true, false] {
                    let s = solve_scaled(q, p, eq);
                    let st = s.solution.status;
                    if claims_result(st) {
                        let sol = &s.solution;
                        assert!(
                            sol.x.iter().chain(&sol.z).chain(&sol.s).all(|x| x.is_finite()),
                            "{st:?} at q={q:e}, p={p:e}"
                        );
                        if matches!(st, SolverStatus::Solved | SolverStatus::AlmostSolved) {
                            assert!(
                                sol.obj_val.is_finite(),
                                "{st:?} with objective {} at q={q:e}, p={p:e}",
                                sol.obj_val
                            );
                        }
                    }
                }
            }
        }
    }
}

fn base() -> (CscMatrix, Vec<f64>, CscMatrix, Vec<f64>, Vec<SupportedConeT>) {
    (diag2(1.0), vec![1.0, -1.0], diag2(1.0), vec![1.0, 1.0], vec![NonnegativeConeT(2)])
}

fn bad_input(P: &CscMatrix, q: &[f64], A: &CscMatrix, b: &[f64], cones: &[SupportedConeT]) -> bool {
    matches!(DefaultSolver::new(P, q, A, b, cones, DefaultSettings::default()), Err(SolverError::BadInputData(_)))
}

#[test]
fn malformed_and_non_finite_input_is_bad_input_not_a_panic() {
    let (P, q, A, b, cones) = base();
    // CscMatrix::new no longer asserts; check_format catches it
    let broken = CscMatrix::new(2, 2, vec![0, 1], vec![0, 1, 5], vec![1.0]);
    assert!(broken.check_format().is_err());
    assert!(bad_input(&broken, &q, &A, &b, &cones));
    assert!(bad_input(&P, &q, &broken, &b, &cones));
    // ragged rows: From gives a matrix that fails check_format
    let ragged = CscMatrix::from(vec![vec![1.0, 0.0], vec![1.0]].iter());
    assert!(ragged.check_format().is_err());
    assert!(bad_input(&P, &q, &ragged, &b, &cones));
    // NaN and infinities
    assert!(bad_input(&P, &[f64::NAN, 1.0], &A, &b, &cones));
    assert!(bad_input(&P, &[f64::INFINITY, 1.0], &A, &b, &cones));
    assert!(bad_input(&P, &q, &A, &[f64::NAN, 1.0], &cones));
    assert!(bad_input(&P, &q, &A, &[f64::NEG_INFINITY, 1.0], &cones));
    assert!(bad_input(&diag2(f64::NAN), &q, &A, &b, &cones));
    // +infinity in b is an absent bound, as upstream
    assert!(DefaultSolver::new(&P, &q, &A, &[f64::INFINITY, 1.0], &cones, DefaultSettings::default()).is_ok());
    // cone sizes that overflow usize
    assert!(bad_input(&P, &q, &A, &b, &[NonnegativeConeT(usize::MAX), NonnegativeConeT(3)]));
    assert!(bad_input(&P, &q, &A, &b, &[SecondOrderConeT(usize::MAX), NonnegativeConeT(3)]));
}

#[test]
fn csc_entry_points_return_errors_instead_of_panicking() {
    assert!(matches!(
        CscMatrix::new_from_triplets(2, 2, vec![0, 1], vec![0], vec![1.0, 2.0]),
        Err(SparseFormatError::IncompatibleDimension)
    ));
    assert!(matches!(
        CscMatrix::new_from_triplets(2, 2, vec![2], vec![0], vec![1.0]),
        Err(SparseFormatError::BadRowval)
    ));
    assert!(matches!(
        CscMatrix::new_from_triplets(2, 2, vec![0], vec![2], vec![1.0]),
        Err(SparseFormatError::BadColptr)
    ));
    let M = CscMatrix::new_from_triplets(2, 2, vec![0, 1, 1], vec![0, 1, 1], vec![1.0, 2.0, 3.0]).unwrap();
    assert_eq!(M.get_entry((1, 1)), Some(5.0));
    assert_eq!(M.get_entry((2, 0)), None);
    assert_eq!(M.get_entry((0, 9)), None);
    assert_eq!(M.index_to_coord(1), Some((1, 1)));
    assert_eq!(M.index_to_coord(2), None);
    let mut M = M;
    assert!(M.set_entry((0, 1), 4.0).is_ok());
    assert_eq!(M.get_entry((0, 1)), Some(4.0));
    assert!(M.set_entry((3, 0), 1.0).is_err());
    let mut broken = CscMatrix::new(2, 2, vec![0], vec![], vec![]);
    assert_eq!(broken.get_entry((0, 0)), None);
    assert!(broken.set_entry((0, 0), 1.0).is_err());
}

#[test]
fn data_updates_must_be_finite_and_leave_the_solver_unchanged_when_refused() {
    let (P, q, A, b, cones) = base();
    let settings = DefaultSettings { presolve_enable: false, ..DefaultSettings::default() };
    let mut s = DefaultSolver::new(&P, &q, &A, &b, &cones, settings).unwrap();
    s.solve();
    let before: Vec<u64> = s.solution.x.iter().map(|v| v.to_bits()).collect();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(s.update_q(&vec![bad, 1.0]), Err(DataUpdateError::NonFiniteData)));
        assert!(matches!(s.update_b(&vec![1.0, bad]), Err(DataUpdateError::NonFiniteData)));
        assert!(matches!(s.update_P(&vec![bad, 1.0]), Err(DataUpdateError::NonFiniteData)));
        assert!(matches!(s.update_A(&diag2(bad)), Err(DataUpdateError::NonFiniteData)));
        let idx = [1usize];
        let val = [bad];
        assert!(matches!(s.update_q(&std::iter::zip(idx.iter(), val.iter())), Err(DataUpdateError::NonFiniteData)));
        assert!(matches!(s.update_A(&(vec![0usize], vec![bad])), Err(DataUpdateError::NonFiniteData)));
    }
    s.solve();
    let after: Vec<u64> = s.solution.x.iter().map(|v| v.to_bits()).collect();
    assert_eq!(before, after);
    assert!(s.update_q(&vec![2.0, -1.0]).is_ok());
}

fn diagonal(n: usize) -> CscMatrix {
    CscMatrix::new(n, n, (0..=n).collect(), (0..n).collect(), vec![1.0; n])
}

#[test]
fn ordering_budget_is_a_typed_error() {
    let big = diagonal(MAX_ORDERING_DIM + 1);
    assert!(matches!(min_degree_ordering(&big), Err(QDLDLError::OrderingTooLarge)));
    let mut dense_col = CscMatrix::new(4, 4, vec![0, 1, 2, 3, MAX_ORDERING_NNZ + 1], vec![0, 1, 2], vec![1.0; 3]);
    dense_col.rowval.resize(MAX_ORDERING_NNZ + 1, 0);
    dense_col.nzval.resize(MAX_ORDERING_NNZ + 1, 1.0);
    assert!(matches!(min_degree_ordering(&dense_col), Err(QDLDLError::OrderingTooLarge)));
    let malformed = CscMatrix::new(3, 3, vec![0, 1], vec![0], vec![1.0]);
    assert!(matches!(min_degree_ordering(&malformed), Err(QDLDLError::OrderingTooLarge)));
    // the solver refuses such a problem up front as bad input
    let n = MAX_ORDERING_DIM;
    let P = diagonal(n);
    let A = diagonal(n);
    let r =
        DefaultSolver::new(&P, &vec![1.0; n], &A, &vec![1.0; n], &[NonnegativeConeT(n)], DefaultSettings::default());
    assert!(matches!(r, Err(SolverError::BadInputData(_))));
}

/// One realistic size, not timed: example_box.rs scaled to n = 2000 (KKT dimension 6000,
/// 8000 stored entries) is ordered within the budget and solved.
#[test]
fn ordering_handles_a_realistic_size() {
    let n = 2000;
    let I1 = CscMatrix::identity(n);
    let mut I2 = CscMatrix::identity(n);
    I2.negate();
    let A = CscMatrix::vcat(&I1, &I2).unwrap();
    // the KKT pattern [P A'; . -I] in upper-triangular form, as the solver assembles it
    let dim = 3 * n;
    let mut colptr = vec![0];
    let mut rowval = Vec::new();
    for c in 0..dim {
        if c >= n {
            let row = c - n; // row of A, column of A' in the KKT
            let var = row % n;
            rowval.push(var);
        }
        rowval.push(c);
        colptr.push(rowval.len());
    }
    let nz = rowval.len();
    let K = CscMatrix::new(dim, dim, colptr, rowval, vec![1.0; nz]);
    let perm = min_degree_ordering(&K).unwrap();
    let mut seen = vec![false; dim];
    for &p in &perm {
        assert!(!seen[p]);
        seen[p] = true;
    }
    assert_eq!(perm.len(), dim);
    let mut s = DefaultSolver::new(
        &CscMatrix::identity(n),
        &vec![1.0; n],
        &A,
        &vec![1.0; 2 * n],
        &[NonnegativeConeT(2 * n)],
        DefaultSettings::default(),
    )
    .unwrap();
    s.solve();
    assert_eq!(s.solution.status, SolverStatus::Solved);
    assert!((s.solution.obj_val + n as f64 / 2.0).abs() <= 1e-6 * n as f64);
}
