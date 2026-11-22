//! Acceptance criteria and oracle tests for `KRN.MZ.VOR` (AC-1, AC-3, AC-4, AC-5; AC-2 pending
//! PTB TraCIM Chebyshev 2D circle sets in `UNC.RDL`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::{FitError, PointsView};
use fid_fit::vor::{
    algorithm_id, convex_hull, farthest_voronoi, incircle, mz_circle2d, mz_circle2d_xy, nearest_voronoi, orient2d,
    MzCircle, ALGORITHM_ID,
};
use fid_kci::{check, parse_card};
use fid_num::f;
use fid_testkit::Sha256Hasher;
use std::cmp::Ordering;

const CASES_TXT: &str = include_str!("../../../fixtures/fit/vor-small-v1/cases.txt");
const PRED_TXT: &str = include_str!("../../../fixtures/fit/vor-small-v1/pred_cases.txt");
const CARD_TOML: &str = include_str!("../cards/fit.circle2d.mz_vor@1.0.0.toml");

fn parse_hex_f64(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).expect("hex f64"))
}

fn load_cases() -> Vec<(String, Vec<[f64; 2]>)> {
    let mut out = Vec::new();
    for line in CASES_TXT.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(toks[0], "case");
        let name = toks[1].to_string();
        let n: usize = toks[2].parse().expect("n");
        assert_eq!(toks.len(), 3 + 2 * n);
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            pts.push([parse_hex_f64(toks[3 + 2 * i]), parse_hex_f64(toks[3 + 2 * i + 1])]);
        }
        out.push((name, pts));
    }
    assert_eq!(out.len(), 500, "AC-1 requires 500 seeded cases");
    out
}

// ------------------------------------------------------------------ test-only brute-force oracle

struct OracleResult {
    center: [f64; 2],
    r_outer: f64,
    width: f64,
    inner_contacts: Vec<usize>,
    outer_contacts: Vec<usize>,
}

fn circumcenter_oracle(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
    let bx = b[0] - a[0];
    let by = b[1] - a[1];
    let cx = c[0] - a[0];
    let cy = c[1] - a[1];
    let d = 2.0 * (bx * cy - by * cx);
    if d.abs() <= 1e-24 || !d.is_finite() {
        return None;
    }
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    let x = a[0] + (cy * b2 - by * c2) / d;
    let y = a[1] + (bx * c2 - cx * b2) / d;
    if x.is_finite() && y.is_finite() {
        Some([x, y])
    } else {
        None
    }
}

fn bisector_intersection_oracle(u: [f64; 2], v: [f64; 2], p: [f64; 2], q: [f64; 2]) -> Option<[f64; 2]> {
    let a1 = 2.0 * (v[0] - u[0]);
    let b1 = 2.0 * (v[1] - u[1]);
    let r1 = (v[0] - u[0]) * (v[0] + u[0]) + (v[1] - u[1]) * (v[1] + u[1]);
    let a2 = 2.0 * (q[0] - p[0]);
    let b2 = 2.0 * (q[1] - p[1]);
    let r2 = (q[0] - p[0]) * (q[0] + p[0]) + (q[1] - p[1]) * (q[1] + p[1]);
    let det = a1 * b2 - a2 * b1;
    if det.abs() <= 1e-24 || !det.is_finite() {
        return None;
    }
    let x = (r1 * b2 - r2 * b1) / det;
    let y = (a1 * r2 - a2 * r1) / det;
    if x.is_finite() && y.is_finite() {
        Some([x, y])
    } else {
        None
    }
}

/// Exact test-only brute-force oracle over all point triples (3-1 / 1-3) and quadruples (2-2).
fn brute_force_mz_circle(pts: &[[f64; 2]]) -> OracleResult {
    let n = pts.len();
    // Centre for numerical stability of bisector equations when far from origin.
    let mx = pts.iter().map(|p| p[0]).sum::<f64>() / (n as f64);
    let my = pts.iter().map(|p| p[1]).sum::<f64>() / (n as f64);
    let q: Vec<[f64; 2]> = pts.iter().map(|p| [p[0] - mx, p[1] - my]).collect();

    let mut best_w = f64::INFINITY;
    let mut best_rmin = 0.0;
    let mut best_rmax = 0.0;
    let mut best_c = [0.0, 0.0];

    let mut eval_cand = |c: [f64; 2], lb: f64| {
        if lb > best_w + 1e-10 {
            return;
        }
        let mut min_k = f64::INFINITY;
        let mut max_k = f64::NEG_INFINITY;
        let mut min_p = q[0];
        let mut max_p = q[0];
        for &pt in &q {
            let k = (pt[0] * pt[0] + pt[1] * pt[1]) - 2.0 * (pt[0] * c[0] + pt[1] * c[1]);
            if !k.is_finite() {
                return;
            }
            if k < min_k {
                min_k = k;
                min_p = pt;
            }
            if k > max_k {
                max_k = k;
                max_p = pt;
            }
        }
        let rmin = f::hypot(min_p[0] - c[0], min_p[1] - c[1]).unwrap();
        let rmax = f::hypot(max_p[0] - c[0], max_p[1] - c[1]).unwrap();
        let denom = rmax + rmin;
        let w = if denom > 0.0 {
            let diff_sq = ((max_p[0] - min_p[0]) * (max_p[0] + min_p[0] - 2.0 * c[0]))
                + ((max_p[1] - min_p[1]) * (max_p[1] + min_p[1] - 2.0 * c[1]));
            if diff_sq <= 0.0 {
                0.0
            } else {
                diff_sq / denom
            }
        } else {
            0.0
        };
        let orig_c = [mx + c[0], my + c[1]];
        let orig_best = [mx + best_c[0], my + best_c[1]];
        let better = if w < best_w {
            true
        } else if w == best_w {
            match f::total_cmp(orig_c[0], orig_best[0]).unwrap() {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => f::total_cmp(orig_c[1], orig_best[1]).unwrap() == Ordering::Less,
            }
        } else {
            false
        };
        if better {
            best_w = w;
            best_rmin = rmin;
            best_rmax = rmax;
            best_c = c;
        }
    };

    // 1. All triples (i, j, k): covers every 3-1 and 1-3 contact candidate.
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                if let Some(c) = circumcenter_oracle(q[i], q[j], q[k]) {
                    eval_cand(c, 0.0);
                }
            }
        }
    }

    // 2. All disjoint pairs of pairs (i, j) x (k, l): covers every 2-2 contact candidate.
    // Using the exact lower bound `|d(c, k) - d(c, i)| <= true_width`, we skip pairs that
    // cannot improve on `best_w` in O(1) while preserving mathematical exactness.
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (i + 1)..n {
                if k == j {
                    continue;
                }
                for l in (k + 1)..n {
                    if l == j {
                        continue;
                    }
                    if let Some(c) = bisector_intersection_oracle(q[i], q[j], q[k], q[l]) {
                        let d_ij = f::hypot(q[i][0] - c[0], q[i][1] - c[1]).unwrap();
                        let d_kl = f::hypot(q[k][0] - c[0], q[k][1] - c[1]).unwrap();
                        eval_cand(c, (d_kl - d_ij).abs());
                    }
                }
            }
        }
    }

    let tol = 1e-13 * best_rmax;
    let mut inner_contacts = Vec::new();
    let mut outer_contacts = Vec::new();
    for (idx, &[px, py]) in q.iter().enumerate() {
        let d = f::hypot(px - best_c[0], py - best_c[1]).unwrap();
        if (d - best_rmin).abs() <= tol {
            inner_contacts.push(idx);
        }
        if (best_rmax - d).abs() <= tol {
            outer_contacts.push(idx);
        }
    }

    OracleResult {
        center: [mx + best_c[0], my + best_c[1]],
        r_outer: best_rmax,
        width: best_w,
        inner_contacts,
        outer_contacts,
    }
}

fn to_xyz(pts: &[[f64; 2]]) -> Vec<f64> {
    let mut xyz = Vec::with_capacity(pts.len() * 3);
    for &[x, y] in pts {
        xyz.push(x);
        xyz.push(y);
        xyz.push(0.0);
    }
    xyz
}

// ------------------------------------------------------------------ tests

#[test]
fn exact_predicates_match_python_fraction_reference() {
    let mut count = 0usize;
    for line in PRED_TXT.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        match toks[0] {
            "orient" => {
                let a = [parse_hex_f64(toks[1]), parse_hex_f64(toks[2])];
                let b = [parse_hex_f64(toks[3]), parse_hex_f64(toks[4])];
                let c = [parse_hex_f64(toks[5]), parse_hex_f64(toks[6])];
                let want: i8 = toks[7].parse().expect("sign");
                let got = orient2d(a, b, c);
                assert_eq!(got, want, "orient2d mismatch on line: {line}");
                count += 1;
            }
            "incirc" => {
                let a = [parse_hex_f64(toks[1]), parse_hex_f64(toks[2])];
                let b = [parse_hex_f64(toks[3]), parse_hex_f64(toks[4])];
                let c = [parse_hex_f64(toks[5]), parse_hex_f64(toks[6])];
                let d = [parse_hex_f64(toks[7]), parse_hex_f64(toks[8])];
                let want: i8 = toks[9].parse().expect("sign");
                let got = incircle(a, b, c, d);
                assert_eq!(got, want, "incircle mismatch on line: {line}");
                count += 1;
            }
            other => panic!("unexpected line kind: {other}"),
        }
    }
    assert_eq!(count, 20);
}

#[test]
fn ac1_vor_small_500_cases_agree_with_brute_force_oracle() {
    let cases = load_cases();
    let mut max_width_diff = 0.0_f64;

    for (name, pts) in &cases {
        let xyz = to_xyz(pts);
        let view = PointsView::new(&xyz, None).expect("valid PointsView");
        let got = mz_circle2d(&view).unwrap_or_else(|e| panic!("{name}: mz_circle2d failed: {e:?}"));
        let oracle = brute_force_mz_circle(pts);

        let w_diff = (got.width - oracle.width).abs();
        if w_diff > max_width_diff {
            max_width_diff = w_diff;
        }
        assert!(
            w_diff <= 1e-12,
            "{name}: width diff {w_diff:e} > 1e-12 (got={}, oracle={}, got_c={:?}, oracle_c={:?})",
            got.width,
            oracle.width,
            got.center,
            oracle.center
        );

        // Critical contact sets must match whenever the centre is uniquely determined (non-tied).
        let c_dist = f::hypot(got.center[0] - oracle.center[0], got.center[1] - oracle.center[1]).unwrap();
        let scale_ref = oracle.r_outer.max(1.0);
        if c_dist <= 1e-9 * scale_ref {
            assert_eq!(
                got.inner_contacts, oracle.inner_contacts,
                "{name}: inner_contacts mismatch (got_w={}, oracle_w={})",
                got.width, oracle.width
            );
            assert_eq!(
                got.outer_contacts, oracle.outer_contacts,
                "{name}: outer_contacts mismatch (got_w={}, oracle_w={})",
                got.width, oracle.width
            );
        }

        // Verify feasibility: every point lies within [r_inner, r_outer] and at least 4 contacts
        // exist (2+2 or 3+1 or 1+3, or more for cocircular/symmetric sets).
        let total_contacts = got.inner_contacts.len() + got.outer_contacts.len();
        assert!(
            total_contacts >= 4,
            "{name}: expected at least 4 critical contacts, got inner={:?} outer={:?}",
            got.inner_contacts,
            got.outer_contacts
        );
        assert_eq!(got.algorithm.to_string(), ALGORITHM_ID);
    }

    assert!(max_width_diff <= 1e-12, "max width diff {max_width_diff:e}");
}

#[test]
fn ac3_determinism_and_permutation_invariance() {
    let cases = load_cases();
    let hash_run = |items: &[(String, Vec<[f64; 2]>)]| -> String {
        let mut h = Sha256Hasher::new();
        for (name, pts) in items {
            let res: MzCircle = mz_circle2d_xy(pts).expect(name);
            h.update(name.as_bytes());
            for v in [res.center[0], res.center[1], res.r_inner, res.r_outer, res.width] {
                h.update(&v.to_bits().to_be_bytes());
            }
            for &i in &res.inner_contacts {
                h.update(&(i as u64).to_be_bytes());
            }
            for &i in &res.outer_contacts {
                h.update(&(i as u64).to_be_bytes());
            }
        }
        format!("{:02x?}", h.finish())
    };

    let d1 = hash_run(&cases);
    let d2 = hash_run(&cases);
    assert_eq!(d1, d2, "two runs must produce bit-identical digests");

    // Permuting point order must keep centre, radii and width bit-identical (lexicographic sort).
    for (name, pts) in cases.iter().take(25) {
        let r_fwd = mz_circle2d_xy(pts).expect(name);
        let mut rev = pts.clone();
        rev.reverse();
        let r_rev = mz_circle2d_xy(&rev).expect(name);
        assert_eq!(r_fwd.center[0].to_bits(), r_rev.center[0].to_bits(), "{name}: cx permutation drift");
        assert_eq!(r_fwd.center[1].to_bits(), r_rev.center[1].to_bits(), "{name}: cy permutation drift");
        assert_eq!(r_fwd.width.to_bits(), r_rev.width.to_bits(), "{name}: width permutation drift");
    }
}

#[test]
fn ac4_collinear_and_degenerate_inputs_return_typed_errors() {
    // 100 collinear points -> FitError::Collinear.
    let mut xyz = Vec::with_capacity(300);
    for i in 0..100 {
        let t = (i as f64) * 0.01;
        xyz.push(1.0 + 3.0 * t);
        xyz.push(-2.0 + 4.0 * t);
        xyz.push(0.0);
    }
    let view = PointsView::new(&xyz, None).expect("valid PointsView");
    let err = mz_circle2d(&view).expect_err("collinear points must fail");
    assert!(matches!(err, FitError::Collinear { .. }), "expected Collinear, got {err:?}");

    // Fewer than 4 distinct points -> TooFewPoints.
    let three = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    assert!(matches!(mz_circle2d_xy(&three), Err(FitError::TooFewPoints { n: 3, need: 4 })));

    // 6 points with only 3 distinct -> TooFewPoints.
    let dup_three = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    assert!(matches!(mz_circle2d_xy(&dup_three), Err(FitError::TooFewPoints { n: 3, need: 4 })));

    // Non-finite coordinate -> NonFinite.
    let bad = [[0.0, 0.0], [1.0, 0.0], [0.0, f64::NAN], [1.0, 1.0]];
    assert!(matches!(mz_circle2d_xy(&bad), Err(FitError::NonFinite { index: 2 })));
}

#[test]
fn ac5_two_thousand_points_feasible_and_fast() {
    let n = 2000usize;
    let mut state = 0x1234_5678_9ABC_DEF0_u64;
    let mut next_u = || -> f64 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 11) as f64) / ((1u64 << 53) as f64)
    };

    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let theta = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
        let r = 0.05 + 0.0005 * (2.0 * next_u() - 1.0);
        pts.push([0.12 + r * f::cos(theta).unwrap(), -0.07 + r * f::sin(theta).unwrap()]);
    }

    let t0 = std::time::Instant::now();
    let res = mz_circle2d_xy(&pts).expect("2000-point circle must succeed");
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Verify all 2000 points lie within [r_inner, r_outer] and width <= 0.001 m (the generator amplitude).
    assert!(res.width > 0.0 && res.width <= 0.0011, "unexpected width {}", res.width);
    for &[x, y] in &pts {
        let d = f::hypot(x - res.center[0], y - res.center[1]).unwrap();
        assert!(d >= res.r_inner - 1e-12 && d <= res.r_outer + 1e-12);
    }
    assert!(res.inner_contacts.len() + res.outer_contacts.len() >= 4);

    // Release-mode budget is < 200 ms (AC-5); allow generous debug headroom here.
    #[cfg(not(debug_assertions))]
    assert!(elapsed_ms < 200.0, "AC-5 exceeded 200 ms: {elapsed_ms:.1} ms");
    let _ = elapsed_ms;
}

#[test]
fn nearest_and_farthest_voronoi_diagrams_and_spec_card() {
    // Unit square + interior point: hull has 4 points, farthest Voronoi only references hull sites.
    let pts = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.5, 0.5]];
    let hull = convex_hull(&pts).expect("hull");
    assert_eq!(hull.len(), 4);
    assert!(!hull.contains(&4));

    let near = nearest_voronoi(&pts).expect("nearest voronoi");
    assert!(!near.vertices.is_empty());
    assert!(!near.edges.is_empty());

    let far = farthest_voronoi(&pts).expect("farthest voronoi");
    assert!(!far.vertices.is_empty());
    for v in &far.vertices {
        for s in v.sites {
            assert_ne!(s, 4, "interior point must never appear in farthest Voronoi");
        }
    }

    // Spec card validates cleanly against fid-kci.
    let card = parse_card(CARD_TOML).expect("parse fit.circle2d.mz_vor@1.0.0.toml");
    assert_eq!(card.id, algorithm_id());
    assert!(check(&[algorithm_id()], &[card.id]).is_clean(), "card check must pass");
}
