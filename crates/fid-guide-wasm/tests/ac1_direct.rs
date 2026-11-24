//! AC-1 (spec node `OUTR.DOCS.GUIDE.WASM`): every lab operation agrees byte for byte with a
//! direct call to the underlying workspace crate.
//!
//! Each test sends one request through `dispatch`, then calls the kernel crate itself with the
//! same inputs and compares the RFC 8785 canonical bytes of every reported number, vector and
//! string. All geometry requests use `unit: "m"` so the lab's unit scale is exactly 1.0 and no
//! conversion stands between the two sides.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_canon::{digest_bytes, parse, parse_with_limits, to_canonical_bytes, CValue};
use fid_fit::contract::{Fit, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_lin::PlaneLs;
use fid_fit::ls_rnd::{self, Circle2Ls};
use fid_fit::tan_plane::{self, TanPlaneParams};
use fid_fit::tan_rock::{MaterialSide, TanVariant};
use fid_fit::{ls_cyl, mcc_cyl};
use fid_gps::zone::shape::{Pose, ZoneInput, ZoneShape};
use fid_guide_wasm::{dispatch, EXIT_OK};
use fid_kdec::eval::{evaluate, EvalInput, Tolerance, DEFAULT_WARN_RATIO};
use fid_kdec::rule::{Catalog, Outcome, RuleRef};
use fid_kunc::dist::{Distribution, Quantity, UnitCode};
use fid_kunc::prop::mcm::interval::summarize;
use fid_kunc::prop::valid::{validate_lpu, ValidateInput};
use fid_kunc::statement::{
    EvidenceClass, Expanded, IntervalKind, Method as UncMethod, ModelRef, Provenance, SourceKind, SourceRef,
    StatementParts, UncertaintyStatement,
};
use fid_num::kdec::Decimal;
use fid_num::red;
use fid_types::Digest;

/// Runs one request; returns the `result` object after checking the envelope's
/// `result_digest` is the SHA-256 of the result's own canonical bytes.
fn lab(req: &str) -> BTreeMap<String, CValue> {
    let (code, bytes) = dispatch(req.as_bytes());
    assert_eq!(code, EXIT_OK, "{}", String::from_utf8_lossy(&bytes));
    let CValue::Obj(env) = parse(&bytes).unwrap() else { panic!("envelope not an object") };
    let result = env.get("result").unwrap().clone();
    let Some(CValue::Str(digest)) = env.get("result_digest") else { panic!("no result_digest") };
    assert_eq!(*digest, digest_bytes(&to_canonical_bytes(&result).unwrap()).to_string());
    let CValue::Obj(m) = result else { panic!("result not an object") };
    m
}

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

fn field<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> &'a CValue {
    m.get(key).unwrap_or_else(|| panic!("missing field `{key}`"))
}

fn obj<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> &'a BTreeMap<String, CValue> {
    let CValue::Obj(o) = field(m, key) else { panic!("`{key}` not an object") };
    o
}

#[track_caller]
fn same_num(m: &BTreeMap<String, CValue>, key: &str, direct: f64) {
    assert_eq!(canon(field(m, key)), canon(&CValue::Num(direct)), "`{key}` differs from the direct call");
}

#[track_caller]
fn same_nums(m: &BTreeMap<String, CValue>, key: &str, direct: &[f64]) {
    let want = CValue::Arr(direct.iter().map(|x| CValue::Num(*x)).collect());
    assert_eq!(canon(field(m, key)), canon(&want), "`{key}` differs from the direct call");
}

#[track_caller]
fn same_u32s(m: &BTreeMap<String, CValue>, key: &str, direct: &[u32]) {
    let want = CValue::Arr(direct.iter().map(|x| CValue::Num(f64::from(*x))).collect());
    assert_eq!(canon(field(m, key)), canon(&want), "`{key}` differs from the direct call");
}

#[track_caller]
fn same_str(m: &BTreeMap<String, CValue>, key: &str, direct: &str) {
    assert_eq!(canon(field(m, key)), canon(&CValue::Str(direct.to_string())), "`{key}` differs");
}

#[track_caller]
fn same_bool(m: &BTreeMap<String, CValue>, key: &str, direct: bool) {
    assert_eq!(canon(field(m, key)), canon(&CValue::Bool(direct)), "`{key}` differs");
}

fn points_json(pts: &[[f64; 3]]) -> String {
    let rows: Vec<String> = pts.iter().map(|p| format!("[{:?},{:?},{:?}]", p[0], p[1], p[2])).collect();
    format!("[{}]", rows.join(","))
}

fn flat(pts: &[[f64; 3]]) -> Vec<f64> {
    pts.iter().flat_map(|p| p.iter().copied()).collect()
}

#[test]
fn ac1_circle2d_matches_circle2ls() {
    let pts2: [[f64; 2]; 7] = [
        [0.010, 0.0],
        [0.0, 0.0101],
        [-0.0099, 0.0],
        [0.0, -0.010],
        [0.00707, 0.00708],
        [-0.00705, 0.0071],
        [0.0071, -0.00706],
    ];
    let pts3: Vec<[f64; 3]> = pts2.iter().map(|p| [p[0], p[1], 0.0]).collect();
    let rows: Vec<String> = pts2.iter().map(|p| format!("[{:?},{:?}]", p[0], p[1])).collect();
    let m = lab(&format!(r#"{{"op":"fit.circle2d","params":{{"unit":"m","points":[{}]}}}}"#, rows.join(",")));

    let data = flat(&pts3);
    let view = PointsView::new(&data, None).unwrap();
    let res = Circle2Ls.fit(&view, &ls_rnd::default_params(), None).unwrap();
    let FittedFeature::Circle2 { c, r } = res.feature else { panic!("not a circle") };

    same_str(&m, "algorithm_id", &res.algorithm.to_string());
    same_num(&m, "center_x", c[0]);
    same_num(&m, "center_y", c[1]);
    same_num(&m, "radius", r);
    same_num(&m, "radius_m", r);
    same_num(&m, "diameter", 2.0 * r);
    same_num(&m, "rms", res.stats.rms);
    same_num(&m, "min_residual", res.stats.min);
    same_num(&m, "max_residual", res.stats.max);
    same_num(&m, "roundness_pv", res.stats.max - res.stats.min);
    same_nums(&m, "residuals", &res.residuals);
}

#[test]
fn ac1_plane_datum_matches_planels_and_tan_plane() {
    let pts: [[f64; 3]; 9] = [
        [0.0, 0.0, 2.0e-5],
        [0.04, 0.0, 0.0],
        [0.04, 0.04, 3.0e-5],
        [0.0, 0.04, 1.0e-5],
        [0.02, 0.02, -4.0e-5],
        [0.02, 0.0, 5.0e-6],
        [0.02, 0.04, 1.5e-5],
        [0.0, 0.02, 1.2e-5],
        [0.04, 0.02, 1.0e-5],
    ];
    let m = lab(&format!(
        r#"{{"op":"fit.plane_datum","params":{{"unit":"m","variant":"external_minimax","points":{}}}}}"#,
        points_json(&pts)
    ));

    let data = flat(&pts);
    let view = PointsView::new(&data, None).unwrap();
    let ls = PlaneLs.fit(&view, &FitParams::new(Method::LeastSquares), None).unwrap();
    let FittedFeature::Plane { p: ls_p, n: ls_n_raw } = ls.feature else { panic!("not a plane") };
    let outward = [0.0, 0.0, 1.0];
    let ls_n = if red::dot(&ls_n_raw, &outward).unwrap() < 0.0 {
        [-ls_n_raw[0], -ls_n_raw[1], -ls_n_raw[2]]
    } else {
        ls_n_raw
    };
    let tp = TanPlaneParams::new(Some(TanVariant::ExternalMinimax), MaterialSide::Outward(outward), 1e-9, 0.05);
    let tan = tan_plane::fit_tan_plane(&view, &FitParams::new(Method::Tangential), &tp, Some(7)).unwrap();
    let FittedFeature::Plane { p: tan_p, n: tan_n } = tan.feature else { panic!("not a plane") };

    same_str(&m, "interpretation_status", "provisional");
    let l = obj(&m, "least_squares");
    same_str(l, "algorithm_id", &ls.algorithm.to_string());
    same_nums(l, "origin", &ls_p);
    same_nums(l, "normal", &ls_n);
    same_num(l, "rms", ls.stats.rms);
    same_num(l, "flatness_span", ls.stats.max - ls.stats.min);

    let t = obj(&m, "tangential");
    same_str(t, "algorithm_id", &tan.algorithm.to_string());
    same_nums(t, "origin", &tan_p);
    same_nums(t, "normal", &tan_n);
    same_num(t, "objective", tan.objective);
    same_num(t, "max_depth", tan.stats.max);
    same_u32s(t, "contacts", &tan.contacts);
    same_nums(t, "depths", &tan.residuals);
    same_str(t, "optimality", tan.optimality.as_str());
    same_bool(t, "rocking", tan.rock.rocking);
    same_num(t, "max_violation_m", tan.max_violation);
}

#[test]
fn ac1_cylinder_mcc_matches_fit_cylinder_and_mcc() {
    let mut pts = Vec::new();
    for iz in 0..3 {
        let z = f64::from(iz) * 0.015;
        for ia in 0..8 {
            let a = f64::from(ia) * core::f64::consts::FRAC_PI_4;
            let r = 0.010 + if ia % 2 == 0 { 2.0e-5 } else { -1.0e-5 };
            pts.push([r * a.cos(), r * a.sin(), z]);
        }
    }
    let m = lab(&format!(r#"{{"op":"fit.cylinder_mcc","params":{{"unit":"m","points":{}}}}}"#, points_json(&pts)));

    // `{:?}` prints the shortest round-trip form, so the lab parses the identical binary64 inputs.
    let data = flat(&pts);
    let view = PointsView::new(&data, None).unwrap();
    let ls = ls_cyl::fit_cylinder(&view, &FitParams::new(Method::LeastSquares), Some(0)).unwrap();
    let FittedFeature::Cylinder { p: ls_p, d: ls_d, r: ls_r } = ls.result.feature else { panic!("not a cylinder") };
    let mcc = mcc_cyl::fit_mcc_cylinder(&view, &FitParams::new(Method::MinimumCircumscribed), Some(0)).unwrap();
    let FittedFeature::Cylinder { p: mc_p, d: mc_d, r: mc_r } = mcc.feature else { panic!("not a cylinder") };

    same_str(&m, "interpretation_status", "provisional");
    same_num(&m, "diameter_envelope_excess", 2.0 * (mc_r - ls_r));
    let l = obj(&m, "least_squares");
    same_str(l, "algorithm_id", &ls.result.algorithm.to_string());
    same_nums(l, "axis_point", &ls_p);
    same_nums(l, "axis_dir", &ls_d);
    same_num(l, "diameter", 2.0 * ls_r);
    same_num(l, "rms", ls.result.stats.rms);
    let c = obj(&m, "minimum_circumscribed");
    same_str(c, "algorithm_id", &mcc.algorithm.to_string());
    same_nums(c, "axis_point", &mc_p);
    same_nums(c, "axis_dir", &mc_d);
    same_num(c, "diameter", 2.0 * mc_r);
    same_u32s(c, "contacts", &mcc.contacts);
    same_bool(c, "local_optimum_only", mcc.local_optimum_only);
}

#[test]
fn ac1_zone_eval_matches_zone_shape() {
    let pts: [[f64; 3]; 4] =
        [[1.0e-5, 0.0, 0.0], [0.0, 3.0e-5, 0.005], [-4.0e-5, 3.0e-5, 0.010], [2.0e-5, -2.0e-5, 0.012]];
    let m = lab(&format!(
        r#"{{"op":"gdt.zone_eval","params":{{"unit":"m","shape":"cylinder","tolerance":0.0001,"points":{}}}}}"#,
        points_json(&pts)
    ));

    let pose = Pose::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let usage = ZoneShape::Cylinder.usage(&pose, ZoneInput::Points(&pts)).unwrap();
    let contains = ZoneShape::Cylinder.contains(&pose, 0.0001, ZoneInput::Points(&pts)).unwrap();

    same_str(&m, "shape", ZoneShape::Cylinder.name());
    same_num(&m, "usage_size_m", usage.size_m);
    same_num(&m, "usage_size", usage.size_m);
    same_num(&m, "driving_index", f64::from(usage.driving_index));
    same_bool(&m, "contains", contains);
    same_str(&m, "interpretation_status", "provisional");
}

fn zero_digest() -> Digest {
    Digest::parse("sha256:0000000000000000000000000000000000000000000000000000000000000000").unwrap()
}

#[test]
fn ac1_unc_propagate_matches_kunc() {
    let (seed, trials, p, k) = (42_u64, 16_000_u64, 0.95, 1.959_963_984_540_054);
    let m = lab(r#"{"op":"unc.propagate","params":{"model":"linear_sum","trials":16000,"seed":42,"inputs":[
        {"name":"a","kind":"normal","mean":0.025,"sigma":0.0000015},
        {"name":"b","kind":"rect","mean":0.0,"half_width":0.000002}]}}"#);

    let a =
        Distribution::normal(Quantity::new(0.025, UnitCode::Metre), Quantity::new(0.0000015, UnitCode::Metre)).unwrap();
    let b = Distribution::rect(Quantity::new(-0.000002, UnitCode::Metre), Quantity::new(0.000002, UnitCode::Metre))
        .unwrap();
    let (ma, mb) = (a.expectation().unwrap(), b.expectation().unwrap());
    let (ua, ub) = (a.std_uncertainty().unwrap(), b.std_uncertainty().unwrap());
    let fx = red::sum(&[1.0 * ma, 1.0 * mb]).unwrap();
    let uc = red::norm2(&[(1.0 * ua).abs(), (1.0 * ub).abs()]).unwrap();
    let sa = a.sample(seed, 1, 0..trials).unwrap();
    let sb = b.sample(seed, 2, 0..trials).unwrap();
    let ys: Vec<f64> = sa.iter().zip(&sb).map(|(x, y)| 0.0 + 1.0 * x + 1.0 * y).collect();
    let mcm = summarize(&ys, p, fx).unwrap();
    let big_u = k * uc;
    let v = validate_lpu(&ValidateInput::new(fx, big_u, p, mcm.sym.low, mcm.sym.high, p, mcm.u).with_n_dig(2)).unwrap();

    same_num(&m, "fx", fx);
    same_num(&m, "lpu_u_std", uc);
    same_num(&m, "lpu_expanded", big_u);
    same_num(&m, "mcm_mean", mcm.mean);
    same_num(&m, "mcm_u_std", mcm.u);
    same_num(&m, "mcm_sym_low", mcm.sym.low);
    same_num(&m, "mcm_sym_high", mcm.sym.high);
    same_num(&m, "mcm_shortest_low", mcm.shortest.low);
    same_num(&m, "mcm_shortest_high", mcm.shortest.high);
    same_num(&m, "mcm_bias", mcm.mean_minus_fx);
    same_bool(&m, "lpu_check_passed", v.validated);
    same_str(&m, "mc_validation", "ran");
    same_num(&m, "validation_d_low", v.d_low);
    same_num(&m, "validation_d_high", v.d_high);
    same_num(&m, "validation_delta", v.delta);

    // The statement is an LPU statement carrying the LPU numbers; the flag records the
    // validation that really ran above.
    let mut sp = StatementParts::new(EvidenceClass::Uec1);
    sp.method = Some(UncMethod::Lpu);
    sp.unit = Some("si:m".to_string());
    sp.u_std = Some(uc);
    sp.expanded = Some(Expanded { value: k * uc, k });
    sp.interval =
        Some(fid_kunc::CoverageInterval { low: fx - big_u, high: fx + big_u, p, kind: IntervalKind::ProbSymmetric });
    sp.model = Some(ModelRef { id: "um:guide.lab".to_string(), semver: "1.0.0".to_string(), digest: zero_digest() });
    sp.sources = vec![SourceRef {
        id: "guide-lab-budget".to_string(),
        kind: SourceKind::Declared,
        digest: zero_digest(),
        unverified: true,
    }];
    sp.lpu_validated_by_mc = Some(v.validated);
    sp.provenance = Provenance::default();
    let stmt = UncertaintyStatement::new(sp).unwrap();
    same_str(&m, "statement_digest", stmt.digest().as_ref());
    same_str(&m, "statement_method", "LPU");
}

fn dec_stmt(u: f64, k: f64) -> UncertaintyStatement {
    let mut sp = StatementParts::new(EvidenceClass::Uec1);
    sp.method = Some(UncMethod::Lpu);
    sp.unit = Some("si:m".to_string());
    sp.u_std = Some(u);
    sp.expanded = Some(Expanded { value: k * u, k });
    sp.model = Some(ModelRef { id: "um:guide.dec".to_string(), semver: "1.0.0".to_string(), digest: zero_digest() });
    sp.sources = vec![SourceRef {
        id: "guide-dec-budget".to_string(),
        kind: SourceKind::Declared,
        digest: zero_digest(),
        unverified: true,
    }];
    // No Monte Carlo run backs the decision lab: the validation flag stays unset.
    sp.lpu_validated_by_mc = None;
    sp.provenance = Provenance::default();
    UncertaintyStatement::new(sp).unwrap()
}

fn outcome_wire(o: Outcome) -> String {
    match o {
        Outcome::Conforms => "CONFORMS".to_string(),
        Outcome::Nonconforms => "NONCONFORMS".to_string(),
        Outcome::Indeterminate => "INDETERMINATE".to_string(),
        Outcome::NotDecidable(r) => format!("NOT_DECIDABLE({})", r.wire()),
    }
}

#[test]
fn ac1_dec_evaluate_matches_kdec_and_never_claims_mc_validation() {
    let catalog = Catalog::builtin().unwrap();
    let tol = Tolerance {
        lsl: Some(Decimal::parse("0.009950").unwrap()),
        usl: Some(Decimal::parse("0.010050").unwrap()),
        unit: "si:m".to_string(),
    };
    let warn_ratio = Decimal::parse(DEFAULT_WARN_RATIO).unwrap();
    let stmt = dec_stmt(0.000005, 2.0);
    assert_eq!(stmt.lpu_validated_by_mc(), None);

    for rule_ref in ["dr:iso14253-1:2017/default@1", "dr:simple-acceptance@1", "dr:guard-band(r=1)@1"] {
        let m = lab(&format!(
            r#"{{"op":"dec.evaluate","params":{{"value":0.010045,"lsl":"0.009950","usl":"0.010050","u_std":0.000005,"k":2.0,"rule_ref":"{rule_ref}"}}}}"#
        ));
        let rule = catalog.resolve(&RuleRef::parse(rule_ref).unwrap()).unwrap();
        let rec = evaluate(&EvalInput { value: 0.010045, statement: &stmt, tolerance: &tol, rule: &rule, warn_ratio })
            .unwrap();

        same_str(&m, "outcome", &outcome_wire(rec.outcome()));
        same_str(&m, "rule_ref", &rule.rule_ref().to_string());
        same_str(&m, "mc_validation", "not_run");
        match rec.expanded_u() {
            Some(u) => same_num(&m, "expanded_u", u),
            None => assert!(!m.contains_key("expanded_u")),
        }
        match rec.p_conformance() {
            Some(p) => same_num(&m, "p_conformance", p),
            None => assert!(!m.contains_key("p_conformance"), "{rule_ref}: p reported without a PDF"),
        }
        let flags = CValue::Arr(rec.flags().iter().map(|f| CValue::Str(f.wire().to_string())).collect());
        assert_eq!(canon(field(&m, "flags")), canon(&flags));
    }

    // The conformance-probability rule has no admissible PDF without a real MC validation.
    let m = lab(
        r#"{"op":"dec.evaluate","params":{"value":0.010000,"lsl":"0.009950","usl":"0.010050","u_std":0.000005,"k":2.0,"rule_ref":"dr:iso14253-1:2017/default@1"}}"#,
    );
    same_str(&m, "outcome", "NOT_DECIDABLE(pdf_unavailable)");
    assert!(!m.contains_key("p_conformance"));
}

#[test]
fn ac1_canon_digest_matches_fid_canon() {
    let (a, b) = (r#"{"b": 1.0, "a": [2, 3], "c": "é"}"#, r#"{"a":[2,3],"b":1,"c":"é"}"#);
    let req = CValue::Obj(BTreeMap::from([
        ("op".to_string(), CValue::Str("canon.digest".to_string())),
        (
            "params".to_string(),
            CValue::Obj(BTreeMap::from([
                ("json_a".to_string(), CValue::Str(a.to_string())),
                ("json_b".to_string(), CValue::Str(b.to_string())),
            ])),
        ),
    ]));
    let m = lab(std::str::from_utf8(&canon(&req)).unwrap());

    let ca = to_canonical_bytes(&parse_with_limits(a.as_bytes(), 65_536, 32).unwrap()).unwrap();
    let cb = to_canonical_bytes(&parse_with_limits(b.as_bytes(), 65_536, 32).unwrap()).unwrap();
    same_str(&m, "canonical_a", std::str::from_utf8(&ca).unwrap());
    same_str(&m, "canonical_b", std::str::from_utf8(&cb).unwrap());
    same_str(&m, "digest_a", &digest_bytes(&ca).to_string());
    same_str(&m, "digest_b", &digest_bytes(&cb).to_string());
    same_num(&m, "byte_length_a", ca.len() as f64);
    same_str(&m, "replay_verdict", if ca == cb { "IDENTICAL" } else { "DIVERGENT" });
}
