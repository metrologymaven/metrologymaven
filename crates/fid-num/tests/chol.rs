//! Pivoted Cholesky (`la::chol_pivoted`, added for spec node UNC.MODEL.DIST).
//!
//! Expected factors, permutations and ranks are hand-derived from the pinned rules in
//! `src/la/chol.rs` on small matrices whose factors are exact in binary64; the larger
//! cases check the reconstruction `P A P^T = L L^T` against a bound, never against values
//! produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::{chol_pivoted, LaError, Mat};
use fid_num::rng::{Address, Cursor};
use fid_num::NumError;

fn m(rows: &[&[f64]]) -> Mat {
    Mat::from_rows(rows).unwrap()
}

fn l_rows(c: &fid_num::la::CholPivoted) -> Vec<Vec<f64>> {
    let n = c.l().rows();
    (0..n).map(|i| c.l().row(i).unwrap().to_vec()).collect()
}

#[test]
fn diagonal_matrix_pivots_largest_first() {
    let c = chol_pivoted(&m(&[&[1.0, 0.0, 0.0], &[0.0, 4.0, 0.0], &[0.0, 0.0, 2.25]]), 0.0).unwrap();
    assert_eq!(c.perm(), &[1, 2, 0]);
    assert_eq!(c.rank(), 3);
    assert_eq!(l_rows(&c), vec![vec![2.0, 0.0, 0.0], vec![0.0, 1.5, 0.0], vec![0.0, 0.0, 1.0]]);
}

#[test]
fn two_by_two_exact_factor_and_lower_triangle_only() {
    // d = (4, 3): pivot 0; L00 = 2, L10 = 2 / 2 = 1, d1 = 3 - 1 = 2, L11 = sqrt(2).
    let want = vec![vec![2.0, 0.0], vec![1.0, 2.0_f64.sqrt()]];
    let c = chol_pivoted(&m(&[&[4.0, 2.0], &[2.0, 3.0]]), 0.0).unwrap();
    assert_eq!((c.perm(), c.rank()), (&[0usize, 1][..], 2));
    assert_eq!(l_rows(&c), want);
    // The strict upper triangle is never read.
    let c2 = chol_pivoted(&m(&[&[4.0, -99.0], &[2.0, 3.0]]), 0.0).unwrap();
    assert_eq!(c2, c);
}

#[test]
fn ties_go_to_the_lowest_position() {
    let c = chol_pivoted(&m(&[&[1.0, 0.5], &[0.5, 1.0]]), 0.0).unwrap();
    assert_eq!(c.perm(), &[0, 1]);
}

#[test]
fn rank_deficient_psd_stops_at_zero_pivot() {
    let c = chol_pivoted(&m(&[&[1.0, 1.0], &[1.0, 1.0]]), 0.0).unwrap();
    assert_eq!(c.rank(), 1);
    assert_eq!(l_rows(&c), vec![vec![1.0, 0.0], vec![1.0, 0.0]]);
    let z = chol_pivoted(&m(&[&[0.0, 0.0], &[0.0, 0.0]]), 0.0).unwrap();
    assert_eq!(z.rank(), 0);
}

#[test]
fn indefinite_matrix_stops_at_negative_pivot() {
    // d after step 0 is 1 - 2*2 = -3.
    let c = chol_pivoted(&m(&[&[1.0, 2.0], &[2.0, 1.0]]), 0.0).unwrap();
    assert_eq!(c.rank(), 1);
    let neg = chol_pivoted(&m(&[&[-1.0]]), 0.0).unwrap();
    assert_eq!(neg.rank(), 0);
}

#[test]
fn relative_tolerance_cuts_small_pivots() {
    let a = m(&[&[1.0, 0.0], &[0.0, 1e-10]]);
    assert_eq!(chol_pivoted(&a, 1e-8).unwrap().rank(), 1);
    assert_eq!(chol_pivoted(&a, 1e-12).unwrap().rank(), 2);
    // d_p <= thr stops (equality included): thr = 0.5 * 1 = 0.5.
    assert_eq!(chol_pivoted(&m(&[&[1.0, 0.0], &[0.0, 0.5]]), 0.5).unwrap().rank(), 1);
}

#[test]
fn reconstruction_of_random_spd_and_psd_matrices() {
    let mut cur = Cursor::new(Address { seed: 0xC401, stream: 7 }, 0);
    for &(n, r) in &[(1usize, 1usize), (3, 3), (6, 6), (6, 3), (12, 12), (12, 5)] {
        // A = B B^T with B n x r (rank r).
        let b: Vec<f64> = (0..n * r).map(|_| cur.uniform_f64() * 2.0 - 1.0).collect();
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                a[i * n + j] = fid_num::red::dot(&b[i * r..(i + 1) * r], &b[j * r..(j + 1) * r]).unwrap();
            }
        }
        let c = chol_pivoted(&Mat::new(n, n, a.clone()).unwrap(), (n as f64) * 1e-13).unwrap();
        assert_eq!(c.rank(), r, "n={n} r={r}");
        let l = c.l().data();
        let p = c.perm();
        let mut sorted = p.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..n).collect::<Vec<_>>());
        for i in 0..n {
            for j in 0..n {
                let lij = fid_num::red::dot(&l[i * n..i * n + c.rank()], &l[j * n..j * n + c.rank()]).unwrap();
                let err = (lij - a[p[i] * n + p[j]]).abs();
                assert!(err <= 1e-12 * n as f64, "n={n} r={r} ({i},{j}) err {err}");
            }
        }
    }
}

#[test]
fn errors() {
    let e = chol_pivoted(&Mat::new(2, 3, vec![0.0; 6]).unwrap(), 0.0).unwrap_err();
    assert_eq!(LaError::of(&e), Some(LaError::BadShape));
    let a = m(&[&[1.0]]);
    assert_eq!(LaError::of(&chol_pivoted(&a, -1.0).unwrap_err()), Some(LaError::BadTolerance));
    assert!(matches!(chol_pivoted(&a, f64::NAN), Err(NumError::NotANumber { op: "la.chol_pivoted", .. })));
    assert!(matches!(chol_pivoted(&a, f64::INFINITY), Err(NumError::Infinite { op: "la.chol_pivoted", .. })));
    // Overflowing column entry: 1e300 / 1e-150-ish pivot.
    let big = m(&[&[1e-300, 0.0], &[1e300, 1.0]]);
    // d = (1e-300, 1): pivot 1 first, L00 = 1, L10 = 1e300, d1 = 1e-300 - 1e600 overflows.
    assert!(matches!(chol_pivoted(&big, 0.0), Err(NumError::Infinite { op: "la.chol_pivoted", index: Some(1) })));
}
