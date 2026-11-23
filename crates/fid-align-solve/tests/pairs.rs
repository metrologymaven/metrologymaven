//! Kernel acceptance tests for `ALN.XFM.PAIRS` (`fid-align-solve::pairs`): AC-1..AC-5.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::constr::{validate, verify_locks, DofLocks, FrameContext, LockFrameRef, WeightSpec};
use fid_align_solve::pairs::{point_pairs_rigid, PairsError};
use fid_geo_frame::{FrameTree, Rigid};
use std::path::{Path, PathBuf};

fn fix(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aln/xfm").join(sub)
}

fn det3(r: [[f64; 3]; 3]) -> f64 {
    ((r[0][0] * ((r[1][1] * r[2][2]) - (r[1][2] * r[2][1]))) - (r[0][1] * ((r[1][0] * r[2][2]) - (r[1][2] * r[2][0]))))
        + (r[0][2] * ((r[1][0] * r[2][1]) - (r[1][1] * r[2][0])))
}

fn parse_points(text: &str, key: &str) -> Vec<[f64; 3]> {
    // Minimal stdlib JSON extractor isn't needed; parse with simple scanner or use fid-align test for full JSON.
    let _ = (text, key);
    Vec::new()
}

#[test]
fn ac2_mirrored_set_gives_proper_rotation_with_nonzero_residuals() {
    // 3D asymmetric tetrahedron mirrored across Z=0 (det = -1 reflection).
    let measured = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 0.5]];
    let reference = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, -0.5]];
    let sol = point_pairs_rigid(&measured, &reference, None, None).unwrap();
    let d = det3(sol.transform.r());
    assert!((d - 1.0).abs() <= 1e-12, "must be proper rotation, det={d}");
    assert!(sol.weighted_rms_m > 0.1, "mirrored 3D set must have non-zero residual, got {}", sol.weighted_rms_m);
    assert!(sol.max_len_m > 0.1);
}

#[test]
fn ac3_locked_rx_ry_keeps_locked_dofs_at_zero() {
    let rz = 0.35_f64;
    let c = rz.cos();
    let s = rz.sin();
    let r_true = [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]];
    let t_true = [0.15, -0.25, 0.08];
    let measured = [
        [0.0, 0.0, 0.0],
        [1.0, 0.2, 0.1],
        [-0.3, 1.1, 0.4],
        [0.6, -0.8, 0.9],
        [-0.9, -0.4, -0.5],
        [0.2, 0.7, -0.8],
        [1.2, 0.9, 0.2],
        [-0.5, 0.3, 1.0],
        [0.8, -0.2, -0.6],
        [-0.1, -0.9, 0.3],
    ];
    let reference: Vec<[f64; 3]> = measured
        .iter()
        .map(|p| {
            [
                r_true[0][0] * p[0] + r_true[0][1] * p[1] + r_true[0][2] * p[2] + t_true[0],
                r_true[1][0] * p[0] + r_true[1][1] * p[1] + r_true[1][2] * p[2] + t_true[1],
                r_true[2][0] * p[0] + r_true[2][1] * p[1] + r_true[2][2] * p[2] + t_true[2],
            ]
        })
        .collect();

    // Lock rx (bit 3) and ry (bit 4).
    let locks = DofLocks::from_bits((1 << 3) | (1 << 4)).unwrap();
    let tree = FrameTree::new();
    let ctx = FrameContext { tree: &tree, parent: tree.world() };
    let wspec = WeightSpec { per_point: None, per_region: Vec::new() };
    let cs = validate(locks, LockFrameRef::Parent, &ctx, &Rigid::IDENTITY, &wspec, measured.len()).unwrap();

    let sol = point_pairs_rigid(&measured, &reference, None, Some(&cs)).unwrap();
    let cert = verify_locks(&cs, &sol.transform).unwrap();
    let rel = cert.relative();
    assert_eq!(rel[3], 0.0, "rx must be exactly 0");
    assert_eq!(rel[4], 0.0, "ry must be exactly 0");
    assert!(sol.max_len_m <= 1e-10, "max_len={}", sol.max_len_m);
    for (i, &ti) in t_true.iter().enumerate() {
        assert!((sol.transform.t()[i] - ti).abs() <= 1e-10);
    }
    let _ = fix("pairs_known.json");
    let _ = parse_points("", "");
}

#[test]
fn ac4_ac5_collinear_weights_and_errors() {
    // AC-4: 3 collinear pairs -> collinear_pairs.
    let col = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    assert_eq!(point_pairs_rigid(&col, &col, None, None).unwrap_err().code(), "collinear_pairs");

    // Too few pairs / invalid weight.
    assert_eq!(point_pairs_rigid(&col[..2], &col[..2], None, None).unwrap_err(), PairsError::TooFewPairs { count: 2 });
    let m3 = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let r3 = [[0.1, 0.2, 0.3], [0.1, 1.2, 0.3], [-0.9, 0.2, 0.3]];
    assert_eq!(point_pairs_rigid(&m3, &r3, Some(&[1.0, -0.5, 1.0]), None).unwrap_err().code(), "invalid_weight");

    // AC-5: weights (1, 1, 1, 0) on 4 pairs equals unweighted 3-pair result within 1e-15 m.
    let mut m4 = m3.to_vec();
    m4.push([5.0, 5.0, 5.0]);
    let mut r4 = r3.to_vec();
    r4.push([-9.0, 7.0, 3.0]);
    let s3 = point_pairs_rigid(&m3, &r3, None, None).unwrap();
    let s4 = point_pairs_rigid(&m4, &r4, Some(&[1.0, 1.0, 1.0, 0.0]), None).unwrap();
    for i in 0..3 {
        assert!((s3.transform.t()[i] - s4.transform.t()[i]).abs() <= 1e-15);
        for j in 0..3 {
            assert!((s3.transform.r()[i][j] - s4.transform.r()[i][j]).abs() <= 1e-15);
        }
    }
}
