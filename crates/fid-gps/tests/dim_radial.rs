//! Pure evaluation of spec node GDT.DIM.RADIAL (`fid_gps::dim::radial`): AC-1 (diameter and
//! radius of the LS circle, cylinder and sphere of `fixtures/gdt/dim/radial_analytic.json`
//! within 1e-12 m), AC-3 (a cone without gauge position), AC-4 (feature fitted by LS, chain
//! names maximum inscribed) and the other refusals of the pinned procedure. Expected values come
//! from the independent reference `fixtures/gdt/dim/make_radial.py` (planted geometry); error
//! paths are hand-labelled from the leaf's rules. AC-2 (cone diameter at a gauge) is open: the
//! cone fit is not on main, and a cone with a gauge is `fit_unavailable`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dim::radial::{evaluate, Gauge, Measure, RadialInput};
use fid_gps::dmt::AlgId;
use std::collections::BTreeMap;

const TOL: f64 = 1e-12;

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("object"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("string"),
    }
}

struct Feature {
    name: String,
    fit: AlgId,
    xyz: Vec<f64>,
    fields: BTreeMap<String, CValue>,
}

impl Feature {
    fn num(&self, k: &str) -> f64 {
        st(&self.fields[k]).parse().unwrap()
    }
}

fn features() -> Vec<Feature> {
    let path = format!("{}/../../fixtures/gdt/dim/radial_analytic.json", env!("CARGO_MANIFEST_DIR"));
    let doc = fid_canon::parse(&std::fs::read(path).unwrap()).unwrap();
    let CValue::Arr(fs) = &obj(&doc)["features"] else { panic!("features") };
    fs.iter()
        .map(|f| {
            let o = obj(f).clone();
            let CValue::Arr(xs) = &o["xyz"] else { panic!("xyz") };
            Feature {
                name: st(&o["feature"]).to_owned(),
                fit: AlgId::parse(st(&o["fit"])).unwrap(),
                xyz: xs.iter().map(|x| st(x).parse().unwrap()).collect(),
                fields: o,
            }
        })
        .collect()
}

fn feature(name: &str) -> Feature {
    features().into_iter().find(|f| f.name == name).unwrap()
}

fn run(f: &Feature, measure: Measure, chain: &[&AlgId], gauge: Option<Gauge>) -> Result<f64, String> {
    let inp = RadialInput { measure, feature_fit: &f.fit, association: chain, xyz: &f.xyz, gauge };
    evaluate(&inp).map(|v| v.value()).map_err(|e| e.to_string())
}

#[test]
fn ac1_diameter_and_radius_match_the_planted_features() {
    let mut seen = 0;
    for f in features().iter().filter(|f| f.name != "f-cone") {
        let chain = [&f.fit];
        let d = run(f, Measure::Diameter, &chain, None).unwrap();
        let r = run(f, Measure::Radius, &chain, None).unwrap();
        assert!((d - f.num("diameter")).abs() <= TOL, "{}: diameter {d:e}", f.name);
        assert!((r - f.num("radius")).abs() <= TOL, "{}: radius {r:e}", f.name);
        // REQ-001: the diameter is exactly twice the reported radius.
        assert_eq!(d.to_bits(), (2.0 * r).to_bits(), "{}", f.name);
        seen += 1;
    }
    assert_eq!(seen, 4, "circle2, circle3, cylinder, sphere");
}

#[test]
fn the_value_records_the_fit_that_produced_it() {
    let f = feature("f-cylinder");
    let inp = RadialInput {
        measure: Measure::Diameter,
        feature_fit: &f.fit,
        association: &[&f.fit],
        xyz: &f.xyz,
        gauge: None,
    };
    let a = evaluate(&inp).unwrap();
    let b = evaluate(&inp).unwrap();
    assert_eq!(a.algorithm(), "fit.cylinder.ls@1.0.0");
    assert_eq!(a.points(), 60);
    // D0: the same points give the same bits and the same fit digest.
    assert_eq!(a, b);
    let r = evaluate(&RadialInput { measure: Measure::Radius, ..inp }).unwrap();
    assert_eq!(r.fit_digest(), a.fit_digest());
    assert_eq!(r.radius().to_bits(), a.radius().to_bits());
}

#[test]
fn ac3_a_cone_without_gauge_position_is_refused() {
    let f = feature("f-cone");
    for m in [Measure::Diameter, Measure::Radius] {
        assert_eq!(run(&f, m, &[&f.fit], None).unwrap_err(), "gauge_position_missing");
    }
}

/// AC-2 is open (the cone fit, KRN.LSQ.AXIS.CONE, is not on main): a cone with a valid gauge
/// is refused, never estimated.
#[test]
fn a_cone_with_a_gauge_waits_for_the_cone_fit() {
    let f = feature("f-cone");
    let g = Some(Gauge::FromApex(f.num("gauge_from_apex")));
    assert_eq!(run(&f, Measure::Diameter, &[&f.fit], g).unwrap_err(), "fit_unavailable{fit.cone.ls@1.0.0}");
    for bad in [0.0, -0.01, f64::NAN, f64::INFINITY, 1e-310] {
        assert_eq!(
            run(&f, Measure::Diameter, &[&f.fit], Some(Gauge::FromApex(bad))).unwrap_err(),
            "bad_gauge{distance}"
        );
    }
}

#[test]
fn ac4_a_feature_fitted_by_ls_under_a_maximum_inscribed_chain_is_refused() {
    let f = feature("f-circle3");
    let mi = AlgId::parse("fit.circle3.mi@1.0.0").unwrap();
    assert_eq!(
        run(&f, Measure::Diameter, &[&mi], None).unwrap_err(),
        "fit_method_mismatch{fit.circle3.ls@1.0.0:fit.circle3.mi@1.0.0}"
    );
    // A chain without an association step, and one with two.
    assert_eq!(run(&f, Measure::Radius, &[], None).unwrap_err(), "fit_method_mismatch{fit.circle3.ls@1.0.0:none}");
    assert_eq!(run(&f, Measure::Radius, &[&f.fit, &f.fit], None).unwrap_err(), "ambiguous_association{2}");
    // Another primitive's LS fit is a mismatch too.
    let cyl = AlgId::parse("fit.cylinder.ls@1.0.0").unwrap();
    assert_eq!(
        run(&f, Measure::Radius, &[&cyl], None).unwrap_err(),
        "fit_method_mismatch{fit.circle3.ls@1.0.0:fit.cylinder.ls@1.0.0}"
    );
}

#[test]
fn features_that_are_not_radial_or_carry_a_gauge_are_refused() {
    let f = feature("f-sphere");
    for s in ["fit.plane.ls@1.0.0", "fit.line3.ls@1.0.0", "syn.assoc.least_squares@0.1.0"] {
        let a = AlgId::parse(s).unwrap();
        let inp =
            RadialInput { measure: Measure::Radius, feature_fit: &a, association: &[&a], xyz: &f.xyz, gauge: None };
        assert_eq!(evaluate(&inp).unwrap_err().to_string(), format!("not_radial_feature{{{s}}}"));
    }
    assert_eq!(run(&f, Measure::Radius, &[&f.fit], Some(Gauge::FromApex(0.01))).unwrap_err(), "gauge_not_applicable");
}

/// `fid-fit` refusals pass through and are never a value: too few points, coplanar points for
/// a sphere, a non-finite coordinate.
#[test]
fn fit_refusals_pass_through() {
    let sphere = AlgId::parse("fit.sphere.ls@1.0.0").unwrap();
    let cyl = AlgId::parse("fit.cylinder.ls@1.0.0").unwrap();
    let flat = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.5, 0.2, 0.0];
    let r = |a: &AlgId, xyz: &[f64]| {
        let inp = RadialInput { measure: Measure::Radius, feature_fit: a, association: &[a], xyz, gauge: None };
        evaluate(&inp).unwrap_err().to_string()
    };
    assert_eq!(r(&sphere, &flat[..9]), "fit_refused{too_few_points}");
    assert_eq!(r(&sphere, &flat), "fit_refused{coplanar}");
    assert_eq!(r(&cyl, &flat), "fit_refused{coplanar}");
    let mut nan = feature("f-sphere").xyz;
    nan[4] = f64::NAN;
    assert_eq!(r(&sphere, &nan), "fit_refused{non_finite}");
}
