//! Kernel acceptance tests for `GDT.DIM.ANGLE` in `fid-gps` (`AC-1`..`AC-4`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gps::dim::angle::{evaluate_angle, AngleError, AngleInput, AngleMode, AuthoredUnit, FrameAxis};

const TOL: f64 = 1e-12;

#[test]
fn ac1_analytic_direction_pairs_both_modes_within_1e_12_rad() {
    let cases: &[([f64; 3], [f64; 3], f64, f64)] = &[
        ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.0, 0.0),
        ([1.0, 0.0, 0.0], [0.8660254037844386, 0.5, 0.0], core::f64::consts::FRAC_PI_6, core::f64::consts::FRAC_PI_6),
        ([1.0, 0.0, 0.0], [1.0, 1.0, 0.0], core::f64::consts::FRAC_PI_4, core::f64::consts::FRAC_PI_4),
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], core::f64::consts::FRAC_PI_2, core::f64::consts::FRAC_PI_2),
        ([1.0, 0.0, 0.0], [-1.0, 1.0, 0.0], 3.0 * core::f64::consts::FRAC_PI_4, core::f64::consts::FRAC_PI_4),
        ([1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], core::f64::consts::PI, 0.0),
        ([1.0, 0.0, 0.0], [1.0, 1e-9, 0.0], 1e-9, 1e-9),
        ([1.0, 0.0, 0.0], [-1.0, 1e-9, 0.0], core::f64::consts::PI - 1e-9, 1e-9),
    ];

    for &(a, b, exp_full, exp_acute) in cases {
        let full = evaluate_angle(&AngleInput {
            dir_a: a,
            dir_b: b,
            mode: AngleMode::Full,
            plane_normal: None,
            authored_unit: AuthoredUnit::Rad,
        })
        .unwrap();
        assert!((full.value_rad - exp_full).abs() < TOL, "full: got {}, exp {}", full.value_rad, exp_full);
        assert!(!full.projected);

        let acute = evaluate_angle(&AngleInput {
            dir_a: a,
            dir_b: b,
            mode: AngleMode::Acute,
            plane_normal: None,
            authored_unit: AuthoredUnit::Rad,
        })
        .unwrap();
        assert!((acute.value_rad - exp_acute).abs() < TOL, "acute: got {}, exp {}", acute.value_rad, exp_acute);
    }
}

#[test]
fn ac2_projected_xy_minus_30_degrees_about_plus_z() {
    let val = evaluate_angle(&AngleInput {
        dir_a: [1.0, 0.0, 0.3],
        dir_b: [0.8660254037844386, -0.5, -0.7],
        mode: AngleMode::Full,
        plane_normal: Some(FrameAxis::PosZ.unit_vec()),
        authored_unit: AuthoredUnit::Deg,
    })
    .unwrap();
    assert!(val.projected);
    assert!((val.value_rad - (-core::f64::consts::FRAC_PI_6)).abs() < TOL);
    assert!((val.authored_value - (-30.0)).abs() < 1e-10);
}

#[test]
fn ac3_degenerate_direction_and_non_finite_rejected() {
    let err = evaluate_angle(&AngleInput {
        dir_a: [0.0, 0.0, 1.0],
        dir_b: [1.0, 0.0, 0.0],
        mode: AngleMode::Acute,
        plane_normal: Some([0.0, 0.0, 1.0]),
        authored_unit: AuthoredUnit::Rad,
    })
    .expect_err("line parallel to projection normal must fail");
    assert_eq!(err.code(), "degenerate_direction");
    assert!(matches!(err, AngleError::DegenerateDirection { which: "a" }));

    let zero_err = evaluate_angle(&AngleInput {
        dir_a: [1e-13, 0.0, 0.0],
        dir_b: [1.0, 0.0, 0.0],
        mode: AngleMode::Full,
        plane_normal: None,
        authored_unit: AuthoredUnit::Rad,
    })
    .expect_err("sub-1e-12 vector must fail");
    assert_eq!(zero_err.code(), "degenerate_direction");

    let nan_err = evaluate_angle(&AngleInput {
        dir_a: [f64::NAN, 0.0, 0.0],
        dir_b: [1.0, 0.0, 0.0],
        mode: AngleMode::Full,
        plane_normal: None,
        authored_unit: AuthoredUnit::Rad,
    })
    .expect_err("NaN must fail");
    assert_eq!(nan_err.code(), "degenerate_direction");
}

#[test]
fn ac4_knp1_bit_identical_determinism_pin() {
    let inp = AngleInput {
        dir_a: [1.0, 0.0, 0.0],
        dir_b: [0.8660254037844386, 0.5, 0.0],
        mode: AngleMode::Full,
        plane_normal: None,
        authored_unit: AuthoredUnit::Deg,
    };
    let v1 = evaluate_angle(&inp).unwrap();
    let v2 = evaluate_angle(&inp).unwrap();
    assert_eq!(v1.value_rad.to_bits(), v2.value_rad.to_bits());
    assert_eq!(v1.authored_value.to_bits(), v2.authored_value.to_bits());
}
