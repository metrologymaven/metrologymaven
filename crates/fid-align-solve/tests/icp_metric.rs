//! Acceptance tests for `ALN.BFIT.ICP.METRIC` (`AC-1`..`AC-4`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::constr::{validate, ConstraintSet, Dof, DofLocks, FrameContext, LockFrameRef, WeightSpec};
use fid_align_solve::icp_metric::{
    build_rows, Correspondence, Correspondences, Metric, MetricError, ALG_POINT_TO_PLANE, ALG_POINT_TO_POINT, P3,
};
use fid_geo_frame::{FrameId, FrameTree, Rigid};
use fid_testkit::WORKSPACE_ROOT;
use std::path::Path;

fn make_cs(n_points: usize, mask: u8) -> ConstraintSet {
    let t = FrameTree::new();
    let ctx = FrameContext { tree: &t, parent: FrameId::WORLD };
    let id = Rigid::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0]).unwrap();
    validate(DofLocks::from_bits(mask).unwrap(), LockFrameRef::Parent, &ctx, &id, &WeightSpec::default(), n_points)
        .unwrap()
}

/// Apply infinitesimal-twist / Rodrigues rigid update `exp([omega]_x) x + t` for finite
/// difference verification (AC-2).
fn apply_twist(x: P3, params: [f64; 6]) -> P3 {
    let t = [params[0], params[1], params[2]];
    let w = [params[3], params[4], params[5]];
    let theta2 = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
    let rx = if theta2 == 0.0 {
        x
    } else {
        let theta = theta2.sqrt();
        let k = [w[0] / theta, w[1] / theta, w[2] / theta];
        let c = theta.cos();
        let s = theta.sin();
        let k_cross_x = [k[1] * x[2] - k[2] * x[1], k[2] * x[0] - k[0] * x[2], k[0] * x[1] - k[1] * x[0]];
        let k_dot_x = k[0] * x[0] + k[1] * x[1] + k[2] * x[2];
        [
            x[0] * c + k_cross_x[0] * s + k[0] * k_dot_x * (1.0 - c),
            x[1] * c + k_cross_x[1] * s + k[1] * k_dot_x * (1.0 - c),
            x[2] * c + k_cross_x[2] * s + k[2] * k_dot_x * (1.0 - c),
        ]
    };
    [rx[0] + t[0], rx[1] + t[1], rx[2] + t[2]]
}

/// AC-1: Fixture `fixtures/aln/bfit/icp/metric_rows.json` (5 correspondences) matches
/// analytic point-to-point and point-to-plane residuals & Jacobians within 1e-15 m, and
/// algorithm IDs map one-to-one.
#[test]
fn ac1_analytic_fixture_matches_within_1e_15_and_algorithm_ids_map() {
    assert_eq!(Metric::PointToPoint.algorithm_id(), ALG_POINT_TO_POINT);
    assert_eq!(Metric::PointToPlane.algorithm_id(), ALG_POINT_TO_PLANE);
    assert_eq!(Metric::from_algorithm_id(ALG_POINT_TO_POINT), Some(Metric::PointToPoint));
    assert_eq!(Metric::from_algorithm_id(ALG_POINT_TO_PLANE), Some(Metric::PointToPlane));

    let raw =
        std::fs::read_to_string(Path::new(WORKSPACE_ROOT).join("fixtures/aln/bfit/icp/metric_rows.json")).unwrap();
    let cases = parse_fixture(&raw);
    assert_eq!(cases.len(), 5);

    let moved: Vec<P3> = cases.iter().map(|c| c.x).collect();
    let corr =
        Correspondences::new(cases.iter().map(|c| Correspondence { valid: true, foot: c.q, normal: c.n }).collect());
    let weights: Vec<f64> = cases.iter().map(|c| c.w).collect();
    let cs = make_cs(5, 0);

    let p2p = build_rows(Metric::PointToPoint, &moved, &corr, &weights, &cs).unwrap();
    assert_eq!(p2p.r.len(), 15);
    assert_eq!(p2p.j.rows(), 15);
    assert_eq!(p2p.j.cols(), 6);
    for (i, c) in cases.iter().enumerate() {
        for (axis, &exp) in c.p2p_r.iter().enumerate() {
            let r_idx = 3 * i + axis;
            assert!((p2p.r[r_idx] - exp).abs() < 1e-15, "p2p r[{r_idx}]");
            assert_eq!(p2p.point_of_row[r_idx], i as u64);
        }
    }

    let p2pl = build_rows(Metric::PointToPlane, &moved, &corr, &weights, &cs).unwrap();
    assert_eq!(p2pl.r.len(), 5);
    assert_eq!(p2pl.j.rows(), 5);
    assert_eq!(p2pl.j.cols(), 6);
    for (i, c) in cases.iter().enumerate() {
        assert!((p2pl.r[i] - c.p2pl_r).abs() < 1e-15, "p2pl r[{i}]");
        assert_eq!(p2pl.point_of_row[i], i as u64);
        let jrow = p2pl.j.row(i).unwrap();
        for (col, (&got, &exp)) in jrow.iter().zip(c.p2pl_j.iter()).enumerate() {
            assert!((got - exp).abs() < 1e-15, "p2pl j[{i}][{col}]");
        }
    }
}

/// AC-2: Given deterministic random poses (seed 5), both point-to-point and point-to-plane
/// Jacobians match central finite differences to relative error < 1e-6.
#[test]
fn ac2_jacobian_matches_central_finite_differences_seed_5() {
    let mut rng = 5u64;
    let mut next_f64 = || -> f64 {
        // Simple deterministic splitmix64 for reproducible seed-5 test poses.
        rng = rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        ((z >> 11) as f64) * (1.0 / ((1u64 << 53) as f64)) * 2.0 - 1.0
    };

    let eps = 1e-7;
    let cs = make_cs(6, 0);
    for _trial in 0..5 {
        let mut moved = Vec::new();
        let mut items = Vec::new();
        let mut weights = Vec::new();
        for _ in 0..6 {
            let x = [next_f64() * 2.0, next_f64() * 2.0, next_f64() * 2.0];
            let q = [next_f64() * 2.0, next_f64() * 2.0, next_f64() * 2.0];
            let nr = [next_f64(), next_f64(), next_f64()];
            let nn = (nr[0] * nr[0] + nr[1] * nr[1] + nr[2] * nr[2]).sqrt().max(1e-3);
            let n = [nr[0] / nn, nr[1] / nn, nr[2] / nn];
            let w = next_f64().abs() + 0.5;
            moved.push(x);
            items.push(Correspondence { valid: true, foot: q, normal: n });
            weights.push(w);
        }
        let corr = Correspondences::new(items);

        for metric in [Metric::PointToPoint, Metric::PointToPlane] {
            let rows = build_rows(metric, &moved, &corr, &weights, &cs).unwrap();
            for col in 0..6 {
                let mut dp = [0.0; 6];
                dp[col] = eps;
                let mut dm = [0.0; 6];
                dm[col] = -eps;
                let moved_p: Vec<P3> = moved.iter().map(|&x| apply_twist(x, dp)).collect();
                let moved_m: Vec<P3> = moved.iter().map(|&x| apply_twist(x, dm)).collect();
                let rp = build_rows(metric, &moved_p, &corr, &weights, &cs).unwrap().r;
                let rm = build_rows(metric, &moved_m, &corr, &weights, &cs).unwrap().r;

                for r_idx in 0..rows.r.len() {
                    let fd = (rp[r_idx] - rm[r_idx]) / (2.0 * eps);
                    let ana = rows.j.row(r_idx).unwrap()[col];
                    let scale = ana.abs().max(fd.abs()).max(1e-9);
                    assert!((ana - fd).abs() / scale < 1e-6, "{metric:?} row={r_idx} col={col}: ana={ana} fd={fd}");
                }
            }
        }
    }
}

/// AC-3: Weights `(0, 1, 4)` on three points -> point 0 contributes no rows, point 2's rows
/// are doubled relative to weight 1; invalid correspondences are skipped; empty/NaN rejected.
#[test]
fn ac3_weights_0_1_4_and_error_paths() {
    let p = [1.0, 2.0, 3.0];
    let q = [1.0, 2.0, 2.0];
    let n = [0.0, 0.0, 1.0];
    let moved = [p, p, p];
    let corr = Correspondences::new(vec![
        Correspondence { valid: true, foot: q, normal: n },
        Correspondence { valid: true, foot: q, normal: n },
        Correspondence { valid: true, foot: q, normal: n },
    ]);
    let cs = make_cs(3, 0);
    let rows = build_rows(Metric::PointToPlane, &moved, &corr, &[0.0, 1.0, 4.0], &cs).unwrap();
    assert_eq!(rows.point_of_row, vec![1, 2]);
    assert!((rows.r[0] - 1.0).abs() < 1e-15);
    assert!((rows.r[1] - 2.0).abs() < 1e-15);
    for col in 0..6 {
        assert!((rows.j.row(1).unwrap()[col] - 2.0 * rows.j.row(0).unwrap()[col]).abs() < 1e-15);
    }

    // All invalid / zero-weight -> empty_rows.
    assert_eq!(build_rows(Metric::PointToPlane, &moved, &corr, &[0.0, 0.0, 0.0], &cs), Err(MetricError::EmptyRows));

    // NaN coordinate -> nan_input.
    let bad_moved = [[f64::NAN, 0.0, 0.0], p, p];
    assert_eq!(
        build_rows(Metric::PointToPoint, &bad_moved, &corr, &[1.0, 1.0, 1.0], &cs).unwrap_err().code(),
        "nan_input"
    );
}

/// AC-4: Locks on `tz` and `c` (`Dof::Tz`, `Dof::Rz`) reduce the Jacobian to 4 columns in
/// order `[tx, ty, a, b]`.
#[test]
fn ac4_locks_on_tz_and_c_reduce_to_tx_ty_a_b() {
    let moved = [[2.0, 3.0, -1.0]];
    let corr =
        Correspondences::new(vec![Correspondence { valid: true, foot: [1.0, 2.0, 0.0], normal: [0.6, 0.8, 0.0] }]);
    let locks = (1u8 << Dof::Tz.index()) | (1u8 << Dof::Rz.index());
    let cs_locked = make_cs(1, locks);
    let cs_free = make_cs(1, 0);

    let full = build_rows(Metric::PointToPlane, &moved, &corr, &[1.0], &cs_free).unwrap();
    let red = build_rows(Metric::PointToPlane, &moved, &corr, &[1.0], &cs_locked).unwrap();
    assert_eq!(red.j.cols(), 4);
    let f = full.j.row(0).unwrap();
    let r = red.j.row(0).unwrap();
    // Free columns in order [tx(0), ty(1), a(3), b(4)].
    assert_eq!(r, &[f[0], f[1], f[3], f[4]]);
}

// ---- Minimal dependency-free parser for fixtures/aln/bfit/icp/metric_rows.json ----

struct Case {
    x: P3,
    q: P3,
    n: P3,
    w: f64,
    p2p_r: P3,
    p2pl_r: f64,
    p2pl_j: [f64; 6],
}

fn parse_fixture(raw: &str) -> Vec<Case> {
    let mut out = Vec::new();
    for chunk in raw.split(r#""x": ["#).skip(1) {
        let x = parse_vec3(chunk);
        let q = parse_vec3_key(chunk, r#""q": ["#);
        let n = parse_vec3_key(chunk, r#""n": ["#);
        let w = parse_num_key(chunk, r#""w": "#);
        let p2p_r = parse_vec3_key(chunk, r#""p2p_r": ["#);
        let p2pl_r = parse_num_key(chunk, r#""p2pl_r": "#);
        let p2pl_j = parse_vec6_key(chunk, r#""p2pl_j": ["#);
        out.push(Case { x, q, n, w, p2p_r, p2pl_r, p2pl_j });
    }
    out
}

fn parse_vec3(s: &str) -> P3 {
    let end = s.find(']').unwrap();
    let nums: Vec<f64> = s[..end].split(',').map(|t| t.trim().parse().unwrap()).collect();
    [nums[0], nums[1], nums[2]]
}

fn parse_vec3_key(hay: &str, key: &str) -> P3 {
    let start = hay.find(key).unwrap() + key.len();
    parse_vec3(&hay[start..])
}

fn parse_vec6_key(hay: &str, key: &str) -> [f64; 6] {
    let start = hay.find(key).unwrap() + key.len();
    let s = &hay[start..];
    let end = s.find(']').unwrap();
    let nums: Vec<f64> = s[..end].split(',').map(|t| t.trim().parse().unwrap()).collect();
    [nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]]
}

fn parse_num_key(hay: &str, key: &str) -> f64 {
    let start = hay.find(key).unwrap() + key.len();
    hay[start..].split([',', '}', '\n', ' ']).next().unwrap().parse().unwrap()
}
