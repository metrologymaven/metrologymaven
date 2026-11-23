//! Kernel unit and fixture tests for `GDT.DIM.PTDEV` (`fid-gps::dim::ptdev`).

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

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        _ => panic!("expected number"),
    }
}

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn rot_z(deg: f64) -> Rigid {
    let rad = deg.to_radians();
    let c = rad.cos();
    let s = rad.sin();
    Rigid::new([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0]).unwrap()
}

#[test]
fn ac1_plane_points_rotated_30_deg_frame() {
    let doc = fid_canon::parse(&std::fs::read(fix("ptdev_plane.json")).unwrap()).unwrap();
    let o = obj(&doc);
    let deg = num(&o["frame_rotation_z_deg"]);
    let frame_to_world = rot_z(deg);
    let world_to_frame = Transform::from(frame_to_world).inverse().unwrap();

    let pts: Vec<[f64; 3]> = arr(&o["points"]).iter().map(vec3).collect();
    let noms: Vec<NominalPoint> = arr(&o["nominals"])
        .iter()
        .map(|n| {
            let no = obj(n);
            NominalPoint { point: vec3(&no["point"]), normal: vec3(&no["normal"]) }
        })
        .collect();
    let exp_dn: Vec<f64> = arr(&o["expected_dn_si"]).iter().map(num).collect();

    let res = evaluate_point_deviations(&pts, &noms, &world_to_frame).unwrap();
    assert_eq!(res.deviations.len(), 2);

    let c = deg.to_radians().cos();
    let s = deg.to_radians().sin();
    for (i, d) in res.deviations.iter().enumerate() {
        let e = exp_dn[i];
        assert!((d.dn - e).abs() <= TOL, "pt {i} dn={} vs {e}", d.dn);
        assert!((d.dx - e * c).abs() <= TOL, "pt {i} dx={} vs {}", d.dx, e * c);
        assert!((d.dy - (-e * s)).abs() <= TOL, "pt {i} dy={} vs {}", d.dy, -e * s);
        assert!(d.dz.abs() <= TOL, "pt {i} dz={}", d.dz);
    }
    assert!((res.min_dn_si - (-0.00002)).abs() <= TOL);
    assert!((res.max_dn_si - 0.00005).abs() <= TOL);
    assert!((res.max_abs_dn_si - 0.00005).abs() <= TOL);
}

#[test]
fn ac2_cylinder_feature_vs_nominal() {
    let doc = fid_canon::parse(&std::fs::read(fix("ptdev_cylinder_feature.json")).unwrap()).unwrap();
    let o = obj(&doc);
    let parse_cyl = |v: &CValue| {
        let co = obj(v);
        DevFeature::Cylinder {
            axis_point: vec3(&co["axis_point"]),
            axis_dir: vec3(&co["axis_dir"]),
            radius: num(&co["radius"]),
        }
    };
    let actual = parse_cyl(&o["actual"]);
    let nominal = parse_cyl(&o["nominal"]);
    let exp = obj(&o["expected"]);
    let exp_loc = vec3(&exp["location_si"]);
    let exp_dir = num(&exp["direction_rad"]);
    let exp_size = num(&exp["size_si"]);

    let fd = evaluate_feature_deviation(&actual, &nominal, &Transform::IDENTITY).unwrap();
    for i in 0..3 {
        assert!((fd.location[i] - exp_loc[i]).abs() <= TOL, "loc[{i}]={} vs {}", fd.location[i], exp_loc[i]);
    }
    assert!((fd.direction_rad.unwrap() - exp_dir).abs() <= TOL);
    assert!((fd.size.unwrap() - exp_size).abs() <= TOL);
}

#[test]
fn ac3_non_unit_normal_and_columnar_format() {
    let pts = [[0.0, 0.0, 0.0], [0.001, 0.0, 0.0]];
    let noms = [
        NominalPoint { point: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0] },
        NominalPoint { point: [0.0, 0.0, 0.0], normal: [1.1, 0.0, 0.0] },
    ];
    let err = evaluate_point_deviations(&pts, &noms, &Transform::IDENTITY).unwrap_err();
    assert_eq!(err, PtDevError::NominalNormalNotUnit { index: 1 });
    assert_eq!(err.code(), "nominal_normal_not_unit");

    // Verify columnar header and length.
    let ok_noms = [
        NominalPoint { point: [0.0, 0.0, 0.0], normal: [1.0, 0.0, 0.0] },
        NominalPoint { point: [0.0, 0.0, 0.0], normal: [1.0, 0.0, 0.0] },
    ];
    let res = evaluate_point_deviations(&pts, &ok_noms, &Transform::IDENTITY).unwrap();
    let bytes = encode_columnar(&res.deviations);
    assert!(bytes.starts_with(COLUMNAR_MAGIC));
    assert_eq!(bytes.len(), COLUMNAR_MAGIC.len() + 8 + 2 * 32);
}
