//! Acceptance tests for `ALN.RPS.CLASSIC` solver in `fid-align-solve` (`REQ-001`..`REQ-007`,
//! `AC-1`..`AC-5`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::{
    solve_321_elements, solve_321_three_points, Rps321Error, Rps321Tolerances, RpsElement, SignedAxis,
};

fn fixture_text() -> String {
    let path = format!("{}/fixtures/aln/rps/block_321.json", fid_testkit::WORKSPACE_ROOT);
    std::fs::read_to_string(path).unwrap()
}

/// Extracts all floating-point numbers appearing after `"key":` up to the closing bracket/brace
/// of that field in our structured fixture JSON.
fn extract_floats_after(text: &str, key: &str, count: usize) -> Vec<f64> {
    let needle = format!("\"{key}\":");
    let start = text.find(&needle).unwrap_or_else(|| panic!("missing key {key}")) + needle.len();
    let mut out = Vec::with_capacity(count);
    let bytes = text.as_bytes();
    let mut i = start;
    while i < bytes.len() && out.len() < count {
        let b = bytes[i];
        if b == b'-' || b.is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_digit()
                    || bytes[j] == b'.'
                    || bytes[j] == b'e'
                    || bytes[j] == b'E'
                    || bytes[j] == b'+'
                    || bytes[j] == b'-')
            {
                j += 1;
            }
            if let Ok(v) = text[i..j].parse::<f64>() {
                out.push(v);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    assert_eq!(out.len(), count, "expected {count} floats after {key}");
    out
}

fn mat3(v: &[f64]) -> [[f64; 3]; 3] {
    [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]]
}

fn vec3(v: &[f64]) -> [f64; 3] {
    [v[0], v[1], v[2]]
}

fn det3(r: &[[f64; 3]; 3]) -> f64 {
    r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0])
}

fn apply(r: &[[f64; 3]; 3], t: [f64; 3], p: [f64; 3]) -> [f64; 3] {
    [
        r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2] + t[0],
        r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2] + t[1],
        r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2] + t[2],
    ]
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn fixture_elements(text: &str) -> (RpsElement, RpsElement, RpsElement) {
    let top = extract_floats_after(text, "top_plane", 6);
    let front = extract_floats_after(text, "front_line", 6);
    let end = extract_floats_after(text, "end_point", 3);
    (
        RpsElement::Plane { point: vec3(&top[0..3]), normal: vec3(&top[3..6]) },
        RpsElement::Line { point: vec3(&front[0..3]), direction: vec3(&front[3..6]) },
        RpsElement::Point { position: vec3(&end[0..3]) },
    )
}

#[test]
fn ac1_and_ac2_block_321_poses_and_determinant() {
    let text = fixture_text();
    let (top, front, end) = fixture_elements(&text);
    let corners_part = extract_floats_after(&text, "corners_part", 24);
    let corners_world = extract_floats_after(&text, "corners_world", 24);
    let exp_zx = extract_floats_after(&text, "expected_zx", 12);
    let exp_y_negx = extract_floats_after(&text, "expected_y_negx", 12);

    // AC-1: default +Z primary, +X secondary.
    let sol_zx =
        solve_321_elements(&top, &front, &end, SignedAxis::PosZ, SignedAxis::PosX, Rps321Tolerances::default())
            .unwrap();
    let r = sol_zx.transform.r();
    let t = sol_zx.transform.t();
    assert!((det3(&r) - 1.0).abs() <= 1e-15, "det = {}", det3(&r));
    let exp_r = mat3(&exp_zx[0..9]);
    let exp_t = vec3(&exp_zx[9..12]);
    for i in 0..3 {
        assert!((t[i] - exp_t[i]).abs() <= 1e-12);
        for j in 0..3 {
            assert!((r[i][j] - exp_r[i][j]).abs() <= 1e-12);
        }
    }
    for k in 0..8 {
        let pw = vec3(&corners_world[3 * k..3 * k + 3]);
        let pp = vec3(&corners_part[3 * k..3 * k + 3]);
        let got = apply(&r, t, pw);
        assert!(dist(got, pp) <= 1e-12, "corner {k} error {}", dist(got, pp));
    }

    // AC-2: primary +Y, secondary -X.
    let sol_ynx =
        solve_321_elements(&top, &front, &end, SignedAxis::PosY, SignedAxis::NegX, Rps321Tolerances::default())
            .unwrap();
    let r2 = sol_ynx.transform.r();
    let t2 = sol_ynx.transform.t();
    assert!((det3(&r2) - 1.0).abs() <= 1e-15);
    let exp_r2 = mat3(&exp_y_negx[0..9]);
    let exp_t2 = vec3(&exp_y_negx[9..12]);
    for k in 0..8 {
        let pw = vec3(&corners_world[3 * k..3 * k + 3]);
        let got = apply(&r2, t2, pw);
        let want = apply(&exp_r2, exp_t2, pw);
        assert!(dist(got, want) <= 1e-12, "AC-2 corner {k} error {}", dist(got, want));
    }
}

#[test]
fn ac3_ac4_and_req_errors() {
    let text = fixture_text();
    let (top, front, end) = fixture_elements(&text);
    let tol = Rps321Tolerances::default();

    // REQ-003: same axis (regardless of sign) -> invalid_axis_assignment.
    let err = solve_321_elements(&top, &front, &end, SignedAxis::PosZ, SignedAxis::NegZ, tol).unwrap_err();
    assert_eq!(err.code(), "invalid_axis_assignment");

    // AC-3: secondary line parallel to primary normal.
    let top_n = match top {
        RpsElement::Plane { normal, .. } => normal,
        _ => unreachable!(),
    };
    let parallel_line = RpsElement::Line { point: [0.0, 0.0, 0.0], direction: top_n };
    let err = solve_321_elements(&top, &parallel_line, &end, SignedAxis::PosZ, SignedAxis::PosX, tol).unwrap_err();
    assert_eq!(err.code(), "secondary_parallel_to_primary");

    // Antiparallel secondary also rejected.
    let antiparallel = RpsElement::Line { point: [0.0, 0.0, 0.0], direction: [-top_n[0], -top_n[1], -top_n[2]] };
    let err = solve_321_elements(&top, &antiparallel, &end, SignedAxis::PosZ, SignedAxis::PosX, tol).unwrap_err();
    assert_eq!(err.code(), "secondary_parallel_to_primary");

    // AC-4: tertiary point lying on the secondary line.
    let on_front = RpsElement::Point { position: front.locus_point() };
    let err = solve_321_elements(&top, &front, &on_front, SignedAxis::PosZ, SignedAxis::PosX, tol).unwrap_err();
    assert_eq!(err.code(), "tertiary_on_secondary");

    // No direction on primary/secondary point or sphere.
    let pt = RpsElement::Point { position: [0.0, 0.0, 0.0] };
    let err = solve_321_elements(&pt, &front, &end, SignedAxis::PosZ, SignedAxis::PosX, tol).unwrap_err();
    assert_eq!(err.code(), "no_direction");
    let sph = RpsElement::Sphere { center: [0.0, 0.0, 0.0] };
    let err = solve_321_elements(&top, &sph, &end, SignedAxis::PosZ, SignedAxis::PosX, tol).unwrap_err();
    assert_eq!(err.code(), "no_direction");

    // NaN input rejected.
    let bad = RpsElement::Point { position: [f64::NAN, 0.0, 0.0] };
    let err = solve_321_elements(&top, &front, &bad, SignedAxis::PosZ, SignedAxis::PosX, tol).unwrap_err();
    assert_eq!(err, Rps321Error::NanInput);
}

#[test]
fn ac5_three_points_and_collinear_error() {
    let text = fixture_text();
    let pts = extract_floats_after(&text, "three_points_world", 9);
    let corners_part = extract_floats_after(&text, "corners_part", 24);
    let corners_world = extract_floats_after(&text, "corners_world", 24);
    let p3 = [vec3(&pts[0..3]), vec3(&pts[3..6]), vec3(&pts[6..9])];

    let sol = solve_321_three_points(p3, SignedAxis::PosX, SignedAxis::PosY, Rps321Tolerances::default()).unwrap();
    let r = sol.transform.r();
    let t = sol.transform.t();
    assert!((det3(&r) - 1.0).abs() <= 1e-15);
    for k in 0..8 {
        let pw = vec3(&corners_world[3 * k..3 * k + 3]);
        let pp = vec3(&corners_part[3 * k..3 * k + 3]);
        assert!(dist(apply(&r, t, pw), pp) <= 1e-12);
    }

    // Collinear points fail with `collinear_points`.
    let col = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [2.0, 4.0, 6.0 + 1e-9]];
    let err = solve_321_three_points(col, SignedAxis::PosX, SignedAxis::PosY, Rps321Tolerances::default()).unwrap_err();
    assert_eq!(err.code(), "collinear_points");
}

#[test]
fn supports_cylinder_cone_circle_and_sphere_elements() {
    let cyl = RpsElement::Cylinder { point: [0.0, 0.0, 0.05], axis: [0.0, 0.0, 1.0] };
    let cone = RpsElement::Cone { point: [0.0, 0.02, 0.0], axis: [1.0, 0.0, 0.0] };
    let sph = RpsElement::Sphere { center: [0.03, 0.10, 0.0] };
    let sol =
        solve_321_elements(&cyl, &cone, &sph, SignedAxis::PosZ, SignedAxis::PosX, Rps321Tolerances::default()).unwrap();
    assert!(dist(sol.origin_world, [0.03, 0.02, 0.05]) <= 1e-15);

    let circle = RpsElement::Circle { center: [0.0, 0.0, 0.05], normal: [0.0, 0.0, 1.0] };
    let sol2 =
        solve_321_elements(&circle, &cone, &sph, SignedAxis::PosZ, SignedAxis::PosX, Rps321Tolerances::default())
            .unwrap();
    assert!(dist(sol2.origin_world, [0.03, 0.02, 0.05]) <= 1e-15);
}
