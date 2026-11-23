//! Acceptance tests for `ALN.BFIT.FEAT.DEV` (`AC-1`..`AC-6`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::feat_dev::{
    deviation, DevError, DevLabel, DevOptions, FeaturePair, FittedFeature, NominalFeature, Vec3, WorkingPlane,
};
use fid_geo_frame::Rigid;
use fid_testkit::WORKSPACE_ROOT;
use std::path::Path;

fn identity() -> Rigid {
    Rigid::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0]).unwrap()
}

fn translation(t: Vec3) -> Rigid {
    Rigid::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t).unwrap()
}

/// Left-multiplies `pose` by small twist `exp([omega]_x, v)` (rotation first, then translation
/// about the alignment origin) for AC-4 central finite difference verification.
fn left_compose_twist(xi: [f64; 6], pose: &Rigid) -> Rigid {
    let w = [xi[0], xi[1], xi[2]];
    let v = [xi[3], xi[4], xi[5]];
    let th2 = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
    let dr = if th2 == 0.0 {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    } else {
        let th = th2.sqrt();
        let k = [w[0] / th, w[1] / th, w[2] / th];
        let c = th.cos();
        let s = th.sin();
        let omc = 1.0 - c;
        [
            [c + k[0] * k[0] * omc, k[0] * k[1] * omc - k[2] * s, k[0] * k[2] * omc + k[1] * s],
            [k[1] * k[0] * omc + k[2] * s, c + k[1] * k[1] * omc, k[1] * k[2] * omc - k[0] * s],
            [k[2] * k[0] * omc - k[1] * s, k[2] * k[1] * omc + k[0] * s, c + k[2] * k[2] * omc],
        ]
    };
    let r0 = pose.r();
    let t0 = pose.t();
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = dr[i][0] * r0[0][j] + dr[i][1] * r0[1][j] + dr[i][2] * r0[2][j];
        }
    }
    let t = [
        dr[0][0] * t0[0] + dr[0][1] * t0[1] + dr[0][2] * t0[2] + v[0],
        dr[1][0] * t0[0] + dr[1][1] * t0[1] + dr[1][2] * t0[2] + v[1],
        dr[2][0] * t0[0] + dr[2][1] * t0[1] + dr[2][2] * t0[2] + v[2],
    ];
    Rigid::new(r, t).unwrap()
}

fn fixture_pairs() -> Vec<FeaturePair> {
    let raw = std::fs::read_to_string(Path::new(WORKSPACE_ROOT).join("fixtures/aln/bfit/feat/dev_types.json")).unwrap();
    assert!(raw.contains(r#""seed": 7"#));
    vec![
        FeaturePair {
            index: 0,
            measured: FittedFeature::Point { position: [0.12, -0.05, 0.08] },
            nominal: NominalFeature::Point { position: [0.12, -0.05, 0.08] },
        },
        FeaturePair {
            index: 1,
            measured: FittedFeature::VectorPoint { position: [0.05, 0.03, -0.04] },
            nominal: NominalFeature::VectorPoint { position: [0.05, 0.03, -0.04], normal: [0.0, 0.6, 0.8] },
        },
        FeaturePair {
            index: 2,
            measured: FittedFeature::Plane {
                centroid: [0.02, -0.01, 0.15],
                normal: [0.0, 0.0, 1.0],
                support_radius: 0.05,
            },
            nominal: NominalFeature::Plane { point: [0.02, -0.01, 0.15], normal: [0.0, 0.0, 1.0] },
        },
        FeaturePair {
            index: 3,
            measured: FittedFeature::Line { point: [0.04, 0.02, 0.01], direction: [1.0, 0.0, 0.0] },
            nominal: NominalFeature::Line { point: [0.04, 0.02, 0.01], direction: [1.0, 0.0, 0.0], length: 0.08 },
        },
        FeaturePair {
            index: 4,
            measured: FittedFeature::Circle { centre: [-0.03, 0.07, 0.02], normal: [0.0, 0.0, 1.0], diameter: 0.04 },
            nominal: NominalFeature::Circle { centre: [-0.03, 0.07, 0.02], normal: [0.0, 0.0, 1.0], diameter: 0.04 },
        },
        FeaturePair {
            index: 5,
            measured: FittedFeature::Sphere { centre: [0.06, -0.04, 0.09], diameter: 0.03 },
            nominal: NominalFeature::Sphere { centre: [0.06, -0.04, 0.09], diameter: 0.03 },
        },
        FeaturePair {
            index: 6,
            measured: FittedFeature::Cylinder { point: [0.01, 0.02, 0.0], axis: [0.0, 0.0, 1.0], diameter: 0.025 },
            nominal: NominalFeature::Cylinder {
                point: [0.01, 0.02, 0.0],
                axis: [0.0, 0.0, 1.0],
                diameter: 0.025,
                length: 0.1,
            },
        },
        FeaturePair {
            index: 7,
            measured: FittedFeature::Cone {
                apex: [-0.02, -0.03, 0.05],
                axis: [0.0, 1.0, 0.0],
                size: 0.5235987755982988,
            },
            nominal: NominalFeature::Cone {
                apex: [-0.02, -0.03, 0.05],
                axis: [0.0, 1.0, 0.0],
                size: 0.5235987755982988,
                length: 0.06,
            },
        },
        FeaturePair {
            index: 8,
            measured: FittedFeature::Slot {
                centre: [0.08, 0.01, -0.02],
                normal: [0.0, 0.0, 1.0],
                long_axis: [1.0, 0.0, 0.0],
                width: 0.012,
            },
            nominal: NominalFeature::Slot {
                centre: [0.08, 0.01, -0.02],
                normal: [0.0, 0.0, 1.0],
                long_axis: [1.0, 0.0, 0.0],
                width: 0.012,
                length: 0.05,
            },
        },
    ]
}

/// AC-1: At identity pose with measured == nominal, every component across all 9 feature types
/// is 0 within 1e-15 and labels match the Design table.
#[test]
fn ac1_identity_pose_all_nine_types_zero_and_expected_labels() {
    let expected_labels: &[&[DevLabel]] = &[
        &[DevLabel::Dx, DevLabel::Dy, DevLabel::Dz],
        &[DevLabel::NormalOffset],
        &[DevLabel::NormalOffset, DevLabel::AngleE1, DevLabel::AngleE2],
        &[DevLabel::AxisOffsetE1, DevLabel::AxisOffsetE2, DevLabel::AngleE1, DevLabel::AngleE2],
        &[DevLabel::Dx, DevLabel::Dy, DevLabel::Dz, DevLabel::AngleE1, DevLabel::AngleE2],
        &[DevLabel::Dx, DevLabel::Dy, DevLabel::Dz],
        &[DevLabel::AxisOffsetE1, DevLabel::AxisOffsetE2, DevLabel::AngleE1, DevLabel::AngleE2],
        &[DevLabel::Dx, DevLabel::Dy, DevLabel::Dz, DevLabel::AngleE1, DevLabel::AngleE2],
        &[DevLabel::Dx, DevLabel::Dy, DevLabel::Dz, DevLabel::LongAxisAngle, DevLabel::AngleE1, DevLabel::AngleE2],
    ];
    let pairs = fixture_pairs();
    let id = identity();
    let opts = DevOptions::default();

    for (pair, want) in pairs.iter().zip(expected_labels.iter()) {
        let d = deviation(pair, &id, &opts).unwrap();
        assert_eq!(d.labels.as_slice(), *want, "pair {}", pair.index);
        assert_eq!(d.jacobian.rows(), d.values.len());
        assert!(!d.flipped);
        for &v in &d.values {
            assert!(v.abs() < 1e-15, "pair {} non-zero value {v}", pair.index);
        }
    }
}

/// AC-2: Translation `(1e-3, -2e-3, 0.5e-3)` m gives exact positional components within 1e-12 m
/// and zero angular components within 1e-15.
#[test]
fn ac2_pure_translation_analytic_positions_and_zero_angles() {
    let dt = [1e-3, -2e-3, 0.5e-3];
    let pose = translation(dt);
    let pairs = fixture_pairs();
    let opts = DevOptions::default();

    // Point (pair 0): dx, dy, dz == dt
    let dp = deviation(&pairs[0], &pose, &opts).unwrap();
    for (got, exp) in dp.values.iter().zip(dt.iter()) {
        assert!((got - exp).abs() < 1e-12);
    }

    // VectorPoint (pair 1): dt . [0, 0.6, 0.8] = -0.0012 + 0.0004 = -0.0008
    let dv = deviation(&pairs[1], &pose, &opts).unwrap();
    assert!((dv.values[0] - (-0.8e-3)).abs() < 1e-12);

    // Plane (pair 2, normal +Z): normal_offset == dt.z == 0.5e-3, angles == 0
    let dpl = deviation(&pairs[2], &pose, &opts).unwrap();
    assert!((dpl.values[0] - 0.5e-3).abs() < 1e-12);
    assert!(dpl.values[1].abs() < 1e-15);
    assert!(dpl.values[2].abs() < 1e-15);

    // Every angular component across all 9 pairs must be 0 within 1e-15 under pure translation.
    for pair in &pairs {
        let d = deviation(pair, &pose, &opts).unwrap();
        for (&v, &lbl) in d.values.iter().zip(d.labels.iter()) {
            if matches!(lbl, DevLabel::AngleE1 | DevLabel::AngleE2 | DevLabel::LongAxisAngle) {
                assert!(v.abs() < 1e-15, "pair {} angle {lbl:?} = {v}", pair.index);
            }
        }
    }
}

/// AC-3: Cylinder tilted by 1e-3 rad with length 0.1 m has angular norm `1e-4 m` within
/// 1e-12 m and returned `L == 0.1 m`; antiparallel flip & `use_size` also verified.
#[test]
fn ac3_tilted_cylinder_angular_scale_flip_and_use_size() {
    let alpha = 1e-3_f64;
    let pair = FeaturePair {
        index: 6,
        measured: FittedFeature::Cylinder {
            point: [0.0, 0.0, 0.0],
            axis: [alpha.sin(), 0.0, alpha.cos()],
            diameter: 0.0252,
        },
        nominal: NominalFeature::Cylinder {
            point: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            diameter: 0.0250,
            length: 0.1,
        },
    };
    let d = deviation(&pair, &identity(), &DevOptions { use_size: true, ..DevOptions::default() }).unwrap();
    assert_eq!(d.length_scale, 0.1);
    let ang_norm = (d.values[2] * d.values[2] + d.values[3] * d.values[3]).sqrt();
    // Exact formula L * ||(u_m . e1, u_m . e2)|| = L * sin(alpha); differs from L * alpha = 1e-4
    // by L * alpha^3 / 6 = 1.667e-11.
    assert!((ang_norm - 0.1 * alpha.sin()).abs() < 1e-15, "ang_norm={ang_norm}");
    assert!((ang_norm - 1e-4).abs() < 2e-11, "ang_norm={ang_norm}");
    assert_eq!(*d.labels.last().unwrap(), DevLabel::Size);
    assert!((*d.values.last().unwrap() - 0.0002).abs() < 1e-15);
    assert_eq!(d.jacobian.row(d.values.len() - 1).unwrap(), &[0.0; 6]);

    // Antiparallel flip (REQ-004): axis pointing -Z gets flipped and recorded.
    let flipped_pair = FeaturePair {
        index: 6,
        measured: FittedFeature::Cylinder { point: [0.0, 0.0, 0.0], axis: [0.0, 0.0, -1.0], diameter: 0.025 },
        nominal: NominalFeature::Cylinder {
            point: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            diameter: 0.025,
            length: 0.1,
        },
    };
    let df = deviation(&flipped_pair, &identity(), &DevOptions::default()).unwrap();
    assert!(df.flipped);
    assert!(df.values.iter().all(|v| v.abs() < 1e-15));
}

/// AC-4: Random poses (seed 7) -> analytic n x 6 twist Jacobian matches central finite
/// differences (`step = 1e-7`) to relative error `< 1e-6` across ALL 9 feature types.
#[test]
fn ac4_analytic_jacobian_matches_finite_differences_seed_7() {
    let mut rng = 7u64;
    let mut next = || -> f64 {
        rng = rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        ((z >> 11) as f64) * (1.0 / ((1u64 << 53) as f64)) * 2.0 - 1.0
    };

    let pairs = fixture_pairs();
    let eps = 1e-7;
    let opts = DevOptions::default();

    for _trial in 0..4 {
        let xi0 = [next() * 0.2, next() * 0.2, next() * 0.2, next() * 0.05, next() * 0.05, next() * 0.05];
        let pose = left_compose_twist(xi0, &identity());

        for pair in &pairs {
            let base = deviation(pair, &pose, &opts).unwrap();
            let mut err_sq = 0.0;
            let mut norm_sq = 0.0;
            let mut fd_mat = vec![[0.0; 6]; base.values.len()];
            for col in 0..6 {
                let mut dp = [0.0; 6];
                dp[col] = eps;
                let mut dm = [0.0; 6];
                dm[col] = -eps;
                let vp = deviation(pair, &left_compose_twist(dp, &pose), &opts).unwrap().values;
                let vm = deviation(pair, &left_compose_twist(dm, &pose), &opts).unwrap().values;
                for row in 0..base.values.len() {
                    fd_mat[row][col] = (vp[row] - vm[row]) / (2.0 * eps);
                }
            }
            for row in 0..base.values.len() {
                let jrow = base.jacobian.row(row).unwrap();
                let row_norm = jrow.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-6);
                for col in 0..6 {
                    let ana = jrow[col];
                    let fd = fd_mat[row][col];
                    let diff = ana - fd;
                    err_sq += diff * diff;
                    norm_sq += ana * ana;
                    let scale = ana.abs().max(fd.abs()).max(row_norm);
                    assert!(
                        diff.abs() / scale < 1e-6,
                        "pair={} row={} ({:?}) col={}: ana={ana} fd={fd} row_norm={row_norm}",
                        pair.index,
                        row,
                        base.labels[row],
                        col
                    );
                }
            }
            assert!(
                err_sq.sqrt() / norm_sq.sqrt().max(1e-12) < 1e-6,
                "pair={} Frobenius relative error exceeded 1e-6",
                pair.index
            );
        }
    }
}

/// AC-5 (error path) & AC-6 (2D working plane): plane paired with cylinder fails with
/// `pair_type_mismatch{index}`; circle on XY working plane returns exactly 2 in-plane components.
#[test]
fn ac5_error_paths_and_ac6_working_plane_2d() {
    let pairs = fixture_pairs();
    let mismatch = FeaturePair { index: 42, measured: pairs[2].measured, nominal: pairs[6].nominal };
    assert_eq!(
        deviation(&mismatch, &identity(), &DevOptions::default()),
        Err(DevError::PairTypeMismatch { index: 42 })
    );

    let bad_l = DevOptions { length_scale: Some(-0.1), ..DevOptions::default() };
    assert_eq!(deviation(&pairs[4], &identity(), &bad_l), Err(DevError::InvalidLengthScale));

    // AC-6: Circle pair on XY working plane returns exactly 2 components (centre x, y in plane).
    let opts_2d = DevOptions {
        working_plane: Some(WorkingPlane { origin: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] }),
        ..DevOptions::default()
    };
    let d2 = deviation(&pairs[4], &translation([1e-3, -2e-3, 5e-3]), &opts_2d).unwrap();
    assert_eq!(d2.labels, vec![DevLabel::PlaneU, DevLabel::PlaneV]);
    assert_eq!(d2.values.len(), 2);
    assert!((d2.values[0] - 1e-3).abs() < 1e-12);
    assert!((d2.values[1] - (-2e-3)).abs() < 1e-12);
}
