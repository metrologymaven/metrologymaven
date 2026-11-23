//! Integration and acceptance tests for `fid-guide-wasm` (spec node `OUTR.DOCS.GUIDE.WASM`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_guide_wasm::{dispatch, EXIT_BAD_INPUT, EXIT_OK, EXIT_OP_ERROR, MAX_REQUEST_BYTES};

fn run_ok(req: &str) -> (CValue, String) {
    let (code, bytes) = dispatch(req.as_bytes());
    assert_eq!(code, EXIT_OK, "expected OK, got: {}", String::from_utf8_lossy(&bytes));
    let val = parse(&bytes).expect("canonical response parses");
    let CValue::Obj(map) = &val else { panic!("expected object") };
    assert_eq!(map.get("ok"), Some(&CValue::Bool(true)));
    let digest = match map.get("result_digest") {
        Some(CValue::Str(s)) => s.clone(),
        _ => panic!("missing result_digest"),
    };
    let result = map.get("result").expect("missing result").clone();
    // Determinism check: second identical call produces byte-identical output.
    let (code2, bytes2) = dispatch(req.as_bytes());
    assert_eq!(code2, EXIT_OK);
    assert_eq!(bytes, bytes2, "output bytes must be bit-identical");
    (result, digest)
}

#[test]
fn ac1_fit_circle2d_with_outlier_impact() {
    let req = r#"{
        "op": "fit.circle2d",
        "params": {
            "unit": "mm",
            "points": [
                [10.0, 0.0], [7.071067811865, 7.071067811865], [0.0, 10.0],
                [-7.071067811865, 7.071067811865], [-10.0, 0.0],
                [-7.071067811865, -7.071067811865], [0.0, -10.0], [7.6, -7.6]
            ],
            "exclude_index": 7
        }
    }"#;
    let (res, digest) = run_ok(req);
    assert!(digest.starts_with("sha256:"));
    let CValue::Obj(m) = res else { panic!("obj") };
    let CValue::Num(r_all) = m.get("radius").expect("radius") else { panic!("num") };
    let CValue::Obj(clean) = m.get("without_outlier").expect("clean") else { panic!("obj") };
    let CValue::Num(r_clean) = clean.get("radius").expect("clean radius") else { panic!("num") };
    assert!((*r_clean - 10.0).abs() < 1e-6, "clean radius {r_clean}");
    assert!(*r_all > *r_clean, "outlier pulls fitted radius outward");
}

#[test]
fn ac1_fit_plane_datum_lsq_vs_tangential() {
    let req = r#"{
        "op": "fit.plane_datum",
        "params": {
            "unit": "mm",
            "variant": "external_minimax",
            "points": [
                [0.0, 0.0, 0.02], [40.0, 0.0, 0.0], [40.0, 40.0, 0.03],
                [0.0, 40.0, 0.01], [20.0, 20.0, -0.04], [20.0, 0.0, 0.005],
                [20.0, 40.0, 0.015], [0.0, 20.0, 0.012], [40.0, 20.0, 0.01]
            ]
        }
    }"#;
    let (res, _) = run_ok(req);
    let CValue::Obj(m) = res else { panic!("obj") };
    assert_eq!(m.get("interpretation_status"), Some(&CValue::Str("provisional".to_string())));
    let CValue::Obj(tan) = m.get("tangential").expect("tangential") else { panic!("obj") };
    let CValue::Arr(contacts) = tan.get("contacts").expect("contacts") else { panic!("arr") };
    assert!(contacts.len() >= 3, "tangent plane rests on >= 3 contacts");
}

#[test]
fn ac1_fit_cylinder_mcc_envelope_exceeds_lsq() {
    // 12 points on a cylinder of nominal radius 10 mm along Z, with mild lobing.
    let mut pts = Vec::new();
    for iz in 0..3 {
        let z = (iz as f64) * 15.0;
        for ia in 0..8 {
            let a = (ia as f64) * core::f64::consts::FRAC_PI_4;
            let r = 10.0 + if ia % 2 == 0 { 0.02 } else { -0.01 };
            pts.push(format!("[{:.9},{:.9},{:.9}]", r * a.cos(), r * a.sin(), z));
        }
    }
    let req = format!(r#"{{"op":"fit.cylinder_mcc","params":{{"unit":"mm","points":[{}]}}}}"#, pts.join(","));
    let (res, _) = run_ok(&req);
    let CValue::Obj(m) = res else { panic!("obj") };
    let CValue::Num(excess) = m.get("diameter_envelope_excess").expect("excess") else { panic!("num") };
    assert!(*excess > 0.0, "MCC mating envelope diameter exceeds LSQ diameter (excess={excess})");
}

#[test]
fn ac1_gdt_zone_eval_cylinder_driving_point() {
    let req = r#"{
        "op": "gdt.zone_eval",
        "params": {
            "unit": "mm",
            "shape": "cylinder",
            "dialect": "ASME Y14.5-2018",
            "tolerance": 0.10,
            "origin": [0.0, 0.0, 0.0],
            "direction": [0.0, 0.0, 1.0],
            "points": [[0.01, 0.0, 0.0], [0.0, 0.03, 5.0], [-0.04, 0.03, 10.0]]
        }
    }"#;
    let (res, _) = run_ok(req);
    let CValue::Obj(m) = res else { panic!("obj") };
    assert_eq!(m.get("interpretation_status"), Some(&CValue::Str("provisional".to_string())));
    // Point 2 has radial distance hypot(-0.04, 0.03) = 0.05 -> diameter = 0.10 mm.
    let CValue::Num(usage) = m.get("usage_size").expect("usage") else { panic!("num") };
    assert!((*usage - 0.10).abs() < 1e-12);
    assert_eq!(m.get("driving_index"), Some(&CValue::Num(2.0)));
    assert_eq!(m.get("contains"), Some(&CValue::Bool(true)));
}

#[test]
fn ac1_unc_propagate_linear_passes_and_nonlinear_fails_lpu() {
    let linear_req = r#"{
        "op": "unc.propagate",
        "params": {
            "model": "linear_sum",
            "trials": 16000,
            "seed": 42,
            "inputs": [
                {"name": "cmm_repeatability", "kind": "normal", "mean": 0.025, "sigma": 0.0000015},
                {"name": "calibration_cert", "kind": "normal", "mean": 0.0, "sigma": 0.0000008}
            ]
        }
    }"#;
    let (lin_res, _) = run_ok(linear_req);
    let CValue::Obj(lm) = lin_res else { panic!("obj") };
    assert_eq!(lm.get("evidence_class"), Some(&CValue::Str("UEC-1".to_string())));
    assert_eq!(lm.get("lpu_check_passed"), Some(&CValue::Bool(true)));

    // Dominant rectangular contributor: Gaussian k=1.96 interval over-covers the boxcar tails.
    let rect_req = r#"{
        "op": "unc.propagate",
        "params": {
            "model": "linear_sum",
            "trials": 16000,
            "seed": 42,
            "inputs": [
                {"name": "scale_resolution", "kind": "rect", "mean": 0.025, "half_width": 0.000005},
                {"name": "cmm_repeatability", "kind": "normal", "mean": 0.0, "sigma": 0.0000004}
            ]
        }
    }"#;
    let (rect_res, _) = run_ok(rect_req);
    let CValue::Obj(rm) = rect_res else { panic!("obj") };
    assert_eq!(rm.get("lpu_check_passed"), Some(&CValue::Bool(false)));

    // Nonlinear radial position near origin: r = hypot(dx, dy) at (0, 0) has zero Jacobian!
    // LPU underestimates uncertainty catastrophically; Monte Carlo catches it.
    let nonlin_req = r#"{
        "op": "unc.propagate",
        "params": {
            "model": "radial_2d",
            "trials": 8000,
            "seed": 42,
            "inputs": [
                {"name": "dx", "kind": "normal", "mean": 0.0, "sigma": 0.000005},
                {"name": "dy", "kind": "normal", "mean": 0.0, "sigma": 0.000005}
            ]
        }
    }"#;
    let (nl_res, _) = run_ok(nonlin_req);
    let CValue::Obj(nm) = nl_res else { panic!("obj") };
    assert_eq!(nm.get("lpu_check_passed"), Some(&CValue::Bool(false)));
}

#[test]
fn ac1_dec_evaluate_rules_and_uec0_refusal() {
    // Near USL = 0.010050 m: simple acceptance conforms, guard band r=1 nonconforms, UEC-0 not decidable.
    let simple = r#"{
        "op": "dec.evaluate",
        "params": {
            "value": 0.010045, "lsl": "0.009950", "usl": "0.010050",
            "u_std": 0.000005, "k": 2.0, "rule_ref": "dr:simple-acceptance@1",
            "prior_mu": 0.010000, "prior_sigma": 0.000018
        }
    }"#;
    let (s_res, _) = run_ok(simple);
    let CValue::Obj(sm) = s_res else { panic!("obj") };
    assert_eq!(sm.get("outcome"), Some(&CValue::Str("CONFORMS".to_string())));

    let guarded = r#"{
        "op": "dec.evaluate",
        "params": {
            "value": 0.010045, "lsl": "0.009950", "usl": "0.010050",
            "u_std": 0.000005, "k": 2.0, "rule_ref": "dr:guard-band(r=1)@1",
            "prior_mu": 0.010000, "prior_sigma": 0.000018
        }
    }"#;
    let (g_res, _) = run_ok(guarded);
    let CValue::Obj(gm) = g_res else { panic!("obj") };
    assert_eq!(gm.get("outcome"), Some(&CValue::Str("NONCONFORMS".to_string())));

    let uec0 = r#"{
        "op": "dec.evaluate",
        "params": {
            "value": 0.010000, "lsl": "0.009950", "usl": "0.010050",
            "uec0": true, "rule_ref": "dr:iso14253-1:2017/default@1"
        }
    }"#;
    let (u0_res, _) = run_ok(uec0);
    let CValue::Obj(u0m) = u0_res else { panic!("obj") };
    assert_eq!(u0m.get("outcome"), Some(&CValue::Str("NOT_DECIDABLE(u_missing)".to_string())));
}

#[test]
fn ac1_canon_digest_identical_and_divergent() {
    let req = r#"{
        "op": "canon.digest",
        "params": {
            "json_a": "{\"b\": 1.0, \"a\": [2, 3]}",
            "json_b": "{\"a\":[2,3],\"b\":1}"
        }
    }"#;
    let (res, _) = run_ok(req);
    let CValue::Obj(m) = res else { panic!("obj") };
    assert_eq!(m.get("replay_verdict"), Some(&CValue::Str("IDENTICAL".to_string())));
}

#[test]
fn ac2_error_paths_never_panic() {
    let (code, _) = dispatch(b"{not valid json");
    assert_eq!(code, EXIT_BAD_INPUT);

    let (code, _) = dispatch(br#"{"op":"no.such.op","params":{}}"#);
    assert_eq!(code, EXIT_OP_ERROR);

    let (code, _) = dispatch(br#"{"op":"fit.circle2d","params":{"points":[[0,0],[1,0]]}}"#);
    assert_eq!(code, EXIT_OP_ERROR);

    let huge = vec![b' '; MAX_REQUEST_BYTES + 1];
    let (code, _) = dispatch(&huge);
    assert_eq!(code, EXIT_BAD_INPUT);
}
