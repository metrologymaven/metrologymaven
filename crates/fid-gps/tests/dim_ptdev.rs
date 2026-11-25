//! Kernel tests for `GDT.DIM.PTDEV` (`fid-gps::dim::ptdev`): AC-1 and AC-2 on the analytic
//! fixtures, REQ-004 for points, circles, spheres and planes, the AC-3 error path and the
//! columnar format of AC-4. Expected values come from `fixtures/gdt/dim/make_ptdev.py`
//! (independent Python reference); none are computed here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dim::ptdev::{
    encode_columnar, evaluate_feature_deviation, evaluate_point_deviations, DevFeature, NominalPoint, PtDevError,
    Rigid, Transform, COLUMNAR_MAGIC,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TOL: f64 = 1e-12;

fn fix(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/dim").join(sub)
}

fn load(sub: &str) -> CValue {
    fid_canon::parse(&std::fs::read(fix(sub)).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("expected object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("expected array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("expected string"),
    }
}

/// Fixture numbers are written as `repr` strings by the Python reference.
fn num(v: &CValue) -> f64 {
    st(v).parse().unwrap()
}

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    assert_eq!(a.len(), 3);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn opt_num(v: &CValue) -> Option<f64> {
    match v {
        CValue::Null => None,
        other => Some(num(other)),
    }
}

/// The fixture frame's world-to-frame transform (inverse of its frame-to-world rigid motion).
fn world_to_frame(v: &CValue) -> Transform {
    let o = obj(v);
    let rows = arr(&o["r"]);
    let r = [vec3(&rows[0]), vec3(&rows[1]), vec3(&rows[2])];
    Transform::from(Rigid::new(r, vec3(&o["t"])).unwrap()).inverse().unwrap()
}

fn feature(v: &CValue) -> DevFeature {
    let o = obj(v);
    match st(&o["kind"]) {
        "point" => DevFeature::Point { position: vec3(&o["position"]) },
        "circle" => {
            DevFeature::Circle { center: vec3(&o["center"]), normal: vec3(&o["normal"]), radius: num(&o["radius"]) }
        }
        "sphere" => DevFeature::Sphere { center: vec3(&o["center"]), radius: num(&o["radius"]) },
        "cylinder" => DevFeature::Cylinder {
            axis_point: vec3(&o["axis_point"]),
            axis_dir: vec3(&o["axis_dir"]),
            radius: num(&o["radius"]),
        },
        "plane" => DevFeature::Plane { centroid: vec3(&o["centroid"]), normal: vec3(&o["normal"]) },
        other => panic!("unknown kind {other}"),
    }
}

fn plane_inputs(o: &BTreeMap<String, CValue>) -> (Vec<[f64; 3]>, Vec<NominalPoint>) {
    let rows = arr(&o["points"]);
    let pts = rows.iter().map(|r| vec3(&obj(r)["point"])).collect();
    let noms = rows
        .iter()
        .map(|r| {
            let n = obj(&obj(r)["nominal"]);
            NominalPoint { point: vec3(&n["point"]), normal: vec3(&n["normal"]) }
        })
        .collect();
    (pts, noms)
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOL, "{what}: {got:e} vs {want:e}");
}

#[test]
fn ac1_plane_points_in_rotated_frame() {
    let doc = load("ptdev_plane.json");
    let o = obj(&doc);
    let (pts, noms) = plane_inputs(o);
    let res = evaluate_point_deviations(&pts, &noms, &world_to_frame(&o["frame"])).unwrap();
    let rows = arr(&o["points"]);
    assert_eq!(res.deviations.len(), rows.len());
    assert_eq!(rows.len(), 2);
    for (i, (d, r)) in res.deviations.iter().zip(rows).enumerate() {
        let r = obj(r);
        let e = obj(&r["expected"]);
        close(d.dn, num(&e["dn"]), &format!("pt {i} dn"));
        close(d.dx, num(&e["dx"]), &format!("pt {i} dx"));
        close(d.dy, num(&e["dy"]), &format!("pt {i} dy"));
        close(d.dz, num(&e["dz"]), &format!("pt {i} dz"));
        // AC-1 names the planted offsets themselves: +0.05 mm and -0.02 mm.
        close(d.dn, num(&r["planted_dn"]), &format!("pt {i} planted dn"));
    }
    let sum = obj(&o["expected_summary"]);
    close(res.min_dn_si, num(&sum["min_dn_si"]), "min dn");
    close(res.max_dn_si, num(&sum["max_dn_si"]), "max dn");
    close(res.max_abs_dn_si, num(&sum["max_abs_dn_si"]), "max |dn|");
}

#[test]
fn ac2_cylinder_feature_vs_nominal() {
    let doc = load("ptdev_cylinder_feature.json");
    let o = obj(&doc);
    let fd = evaluate_feature_deviation(&feature(&o["actual"]), &feature(&o["nominal"]), &Transform::IDENTITY).unwrap();
    assert_eq!(fd.feature_kind, "cylinder");
    for key in ["expected", "planted"] {
        let e = obj(&o[key]);
        let loc = vec3(&e["location_si"]);
        for (i, want) in loc.iter().enumerate() {
            close(fd.location[i], *want, &format!("{key} location[{i}]"));
        }
        close(fd.direction_rad.unwrap(), num(&e["direction_rad"]), &format!("{key} direction"));
        close(fd.size.unwrap(), num(&e["size_si"]), &format!("{key} size"));
    }
}

#[test]
fn req004_point_circle_sphere_plane_in_rotated_frame() {
    let doc = load("ptdev_features.json");
    let o = obj(&doc);
    let w2f = world_to_frame(&o["frame"]);
    let cases = arr(&o["cases"]);
    assert_eq!(cases.len(), 4);
    for c in cases {
        let c = obj(c);
        let name = st(&c["name"]);
        let fd = evaluate_feature_deviation(&feature(&c["actual"]), &feature(&c["nominal"]), &w2f).unwrap();
        assert_eq!(fd.feature_kind, name);
        let e = obj(&c["expected"]);
        let loc = vec3(&e["location_si"]);
        for (i, want) in loc.iter().enumerate() {
            close(fd.location[i], *want, &format!("{name} location[{i}]"));
        }
        match (fd.direction_rad, opt_num(&e["direction_rad"])) {
            (Some(got), Some(want)) => close(got, want, &format!("{name} direction")),
            (None, None) => {}
            other => panic!("{name}: direction presence differs: {other:?}"),
        }
        match (fd.size, opt_num(&e["size_si"])) {
            (Some(got), Some(want)) => close(got, want, &format!("{name} size")),
            (None, None) => {}
            other => panic!("{name}: size presence differs: {other:?}"),
        }
    }
}

#[test]
fn ac3_non_unit_normal_names_index_and_other_errors() {
    let pts = [[0.0, 0.0, 0.0], [0.001, 0.0, 0.0]];
    let noms = [
        NominalPoint { point: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] },
        NominalPoint { point: [0.0, 0.0, 0.0], normal: [1.1, 0.0, 0.0] },
    ];
    let err = evaluate_point_deviations(&pts, &noms, &Transform::IDENTITY).unwrap_err();
    assert_eq!(err, PtDevError::NominalNormalNotUnit { index: 1 });
    assert_eq!(err.code(), "nominal_normal_not_unit");

    // A norm within 1 +- 1e-9 is accepted; just outside it is refused (REQ-003).
    let inside = [NominalPoint { point: [0.0; 3], normal: [1.0 + 5e-10, 0.0, 0.0] }];
    assert!(evaluate_point_deviations(&pts[..1], &inside, &Transform::IDENTITY).is_ok());
    let outside = [NominalPoint { point: [0.0; 3], normal: [1.0 - 2e-9, 0.0, 0.0] }];
    assert_eq!(
        evaluate_point_deviations(&pts[..1], &outside, &Transform::IDENTITY).unwrap_err(),
        PtDevError::NominalNormalNotUnit { index: 0 }
    );

    assert_eq!(
        evaluate_point_deviations(&pts, &noms[..1], &Transform::IDENTITY).unwrap_err().code(),
        "length_mismatch"
    );
    assert_eq!(evaluate_point_deviations(&[], &[], &Transform::IDENTITY).unwrap_err().code(), "empty_point_set");
    let nan = [[f64::NAN, 0.0, 0.0]];
    let unit = [NominalPoint { point: [0.0; 3], normal: [0.0, 0.0, 1.0] }];
    assert_eq!(evaluate_point_deviations(&nan, &unit, &Transform::IDENTITY).unwrap_err().code(), "non_finite");

    let sphere = DevFeature::Sphere { center: [0.0; 3], radius: 0.01 };
    let point = DevFeature::Point { position: [0.0; 3] };
    assert_eq!(
        evaluate_feature_deviation(&sphere, &point, &Transform::IDENTITY).unwrap_err().code(),
        "feature_kind_mismatch"
    );
    let zero_normal = DevFeature::Plane { centroid: [0.0; 3], normal: [0.0; 3] };
    let good = DevFeature::Plane { centroid: [0.0; 3], normal: [0.0, 0.0, 1.0] };
    assert_eq!(
        evaluate_feature_deviation(&zero_normal, &good, &Transform::IDENTITY).unwrap_err().code(),
        "degenerate_direction"
    );
}

#[test]
fn ac4_columnar_layout_round_trips() {
    let doc = load("ptdev_plane.json");
    let o = obj(&doc);
    let (pts, noms) = plane_inputs(o);
    let res = evaluate_point_deviations(&pts, &noms, &world_to_frame(&o["frame"])).unwrap();
    let bytes = encode_columnar(&res.deviations);
    let n = res.deviations.len();
    assert!(bytes.starts_with(COLUMNAR_MAGIC));
    assert_eq!(bytes.len(), COLUMNAR_MAGIC.len() + 8 + n * 32);
    let at = |off: usize| u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap());
    let head = COLUMNAR_MAGIC.len();
    assert_eq!(at(head), n as u64);
    // Columns dx, dy, dz, dn: each n little-endian f64 values in point order.
    for (i, d) in res.deviations.iter().enumerate() {
        let col = |c: usize| at(head + 8 + (c * n + i) * 8);
        assert_eq!(col(0), d.dx.to_bits());
        assert_eq!(col(1), d.dy.to_bits());
        assert_eq!(col(2), d.dz.to_bits());
        assert_eq!(col(3), d.dn.to_bits());
    }
}
