//! Kernel unit and fixture tests for `GDT.DIM.SCALAR` (`fid-gps::dim::scalar`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dim::scalar::{
    evaluate_minmax, evaluate_scalar, MinMaxStat, QuantityKind, ScalarElement, ScalarError, PROPERTY_REGISTRY,
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

#[test]
fn property_registry_is_strictly_sorted() {
    for w in PROPERTY_REGISTRY.windows(2) {
        let a = (w[0].element_kind, w[0].property);
        let b = (w[1].element_kind, w[1].property);
        assert!(a < b, "registry out of order: {a:?} >= {b:?}");
    }
}

#[test]
fn ac1_ac2_ac3_scalar_properties_and_errors() {
    let doc = fid_canon::parse(&std::fs::read(fix("scalar_props.json")).unwrap()).unwrap();
    let o = obj(&doc);
    let cyl_o = obj(&o["cylinder"]);
    let cone_o = obj(&o["cone"]);

    let cyl = ScalarElement::Cylinder {
        length: num(&cyl_o["length"]),
        diameter: num(&cyl_o["diameter"]),
        radius: num(&cyl_o["radius"]),
    };
    let cone = ScalarElement::Cone {
        half_angle_rad: num(&cone_o["half_angle_rad"]),
        apex_angle_rad: num(&cone_o["apex_angle_rad"]),
        length: num(&cone_o["length"]),
    };

    // AC-1: cylinder.length (40 mm) and cone.half_angle (15 deg).
    let len = evaluate_scalar(&cyl, "cylinder.length", Some("mm")).unwrap();
    assert_eq!(len.quantity_kind, QuantityKind::Length);
    assert_eq!(len.si_unit, "si:m");
    assert!((len.value_si - 0.04).abs() <= TOL);

    let ang = evaluate_scalar(&cone, "cone.half_angle", Some("deg")).unwrap();
    assert_eq!(ang.quantity_kind, QuantityKind::Angle);
    assert_eq!(ang.si_unit, "si:rad");
    assert!((ang.value_si - 15.0_f64.to_radians()).abs() <= TOL);

    // AC-2: unknown_property.
    assert_eq!(evaluate_scalar(&cyl, "cylinder.colour", None).unwrap_err().code(), "unknown_property");
    assert_eq!(evaluate_scalar(&cyl, "cone.half_angle", None).unwrap_err().code(), "unknown_property");

    // AC-3: quantity_kind_mismatch (length tolerance on angle property).
    assert_eq!(evaluate_scalar(&cone, "cone.half_angle", Some("mm")).unwrap_err().code(), "quantity_kind_mismatch");
}

#[test]
fn ac4_ac5_minmax_set_ties_empty_and_nan() {
    let doc = fid_canon::parse(&std::fs::read(fix("minmax_set.json")).unwrap()).unwrap();
    let o = obj(&doc);
    let vals: Vec<f64> = arr(&o["values"]).iter().map(num).collect();
    assert_eq!(vals.len(), 1000);

    let res = evaluate_minmax(&vals, MinMaxStat::Max, QuantityKind::Length).unwrap();
    assert!((res.min_si - num(&o["expected_min_si"])).abs() <= TOL);
    assert!((res.max_si - num(&o["expected_max_si"])).abs() <= TOL);
    assert!((res.range_si - num(&o["expected_range_si"])).abs() <= TOL);
    assert_eq!(res.min_index, num(&o["expected_min_index"]) as u64);
    assert_eq!(res.max_index, num(&o["expected_max_index"]) as u64);
    assert_eq!(res.value_si, res.max_si);

    // AC-5: empty_value_set and nan_value.
    assert_eq!(evaluate_minmax(&[], MinMaxStat::Range, QuantityKind::Length), Err(ScalarError::EmptyValueSet));
    assert_eq!(
        evaluate_minmax(&[1.0, f64::NAN, 2.0], MinMaxStat::Min, QuantityKind::Length),
        Err(ScalarError::NanValue { index: Some(1) })
    );
}
