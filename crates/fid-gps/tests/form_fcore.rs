//! Acceptance tests AC-1..AC-6 for `GDT.FORM.FCORE` (`fid_gps::form`).
//! Uses `include_str!` so every test runs identically on x86-64, ARM64, Windows and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_fit::contract::FittedFeature;
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::form::{
    evaluate_form, signed_residuals_of, FitOutcome, FitProvider, FormAssociation, FormElement, FormError, FormFlag,
    KrnFitProvider,
};
use std::collections::BTreeMap;

const FIXTURE_JSON: &str = include_str!("../../../fixtures/gdt/form/fcore_plane_mz.json");

struct Fixture {
    expected_mz_width_m: f64,
    mz_point: [f64; 3],
    mz_normal: [f64; 3],
    expected_peak_index: usize,
    expected_valley_index: usize,
    points: Vec<[f64; 3]>,
}

fn load_fixture() -> Fixture {
    let CValue::Obj(o) = parse(FIXTURE_JSON.as_bytes()).unwrap() else { panic!("fixture not object") };
    let num = |v: &CValue| match v {
        CValue::Num(x) => *x,
        _ => panic!("not num"),
    };
    let vec3 = |v: &CValue| {
        let CValue::Arr(a) = v else { panic!("not arr") };
        [num(&a[0]), num(&a[1]), num(&a[2])]
    };
    let CValue::Arr(pts) = o.get("points").unwrap() else { panic!("no points") };
    Fixture {
        expected_mz_width_m: num(o.get("expected_mz_width_m").unwrap()),
        mz_point: vec3(o.get("mz_plane_point").unwrap()),
        mz_normal: vec3(o.get("mz_plane_normal").unwrap()),
        expected_peak_index: num(o.get("expected_peak_index_mz").unwrap()) as usize,
        expected_valley_index: num(o.get("expected_valley_index_mz").unwrap()) as usize,
        points: pts.iter().map(vec3).collect(),
    }
}

fn chain_with(alg: &str, min_points: Option<i64>) -> OperatorChain {
    let mut params = BTreeMap::new();
    if let Some(m) = min_points {
        params.insert("min_points".to_owned(), Param::Int(m));
    }
    let op = Operator::new(OperatorKind::Association, AlgId::parse(alg).unwrap(), params).unwrap();
    OperatorChain::new("default", vec![op]).unwrap()
}

struct StubKrn {
    mz_avail: bool,
    mz_ref: FittedFeature,
    mz_width: f64,
    local_opt: bool,
}

impl FitProvider for StubKrn {
    fn mz_available(&self, _element: FormElement) -> bool {
        self.mz_avail
    }

    fn fit_minimum_zone(&self, _element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        let residuals = signed_residuals_of(&self.mz_ref, points)?;
        Ok(FitOutcome {
            reference: self.mz_ref.clone(),
            signed_residuals: residuals,
            active_zone_width: Some(self.mz_width),
            algorithm_id: "fit.plane.mz_stub@1.0.0".to_owned(),
            local_optimum_only: self.local_opt,
        })
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        let mut out = KrnFitProvider.fit_least_squares(element, points)?;
        out.local_optimum_only = self.local_opt;
        Ok(out)
    }
}

#[test]
fn ac1_ac2_ac4_ac5_ac6_plane_mz_fallback_tie_breaking_and_flags() {
    let fix = load_fixture();
    let mz_chain = chain_with("fit.plane.mz@1.0.0", Some(4));
    let ls_chain = chain_with("fit.plane.ls@1.0.0", Some(4));
    let mz_plane = FittedFeature::Plane { p: fix.mz_point, n: fix.mz_normal };

    // AC-1: MZ available -> exact width 0.010 mm, MinimumZone, no conservative flag.
    let stub_mz =
        StubKrn { mz_avail: true, mz_ref: mz_plane.clone(), mz_width: fix.expected_mz_width_m, local_opt: false };
    let p_mz = evaluate_form(FormElement::Plane, &fix.points, &mz_chain, &stub_mz).unwrap();
    assert!((p_mz.value_si - 0.00001).abs() <= 1e-12);
    assert_eq!(p_mz.association, FormAssociation::MinimumZone);
    assert!(!p_mz.flags.contains(&FormFlag::ConservativeUpperBound));
    assert!(!p_mz.flags.contains(&FormFlag::LocalOptimumOnly));
    // AC-4: points 2 and 7 tie at +5e-6 m -> lowest index (2) wins; points 1 and 3 tie at -5e-6 m -> lowest index (1) wins.
    assert_eq!(p_mz.peak_index, fix.expected_peak_index);
    assert_eq!(p_mz.valley_index, fix.expected_valley_index);

    // AC-2: MZ unavailable -> LS residual range (>= MZ width), LeastSquares + conservative_upper_bound.
    let stub_no_mz =
        StubKrn { mz_avail: false, mz_ref: mz_plane.clone(), mz_width: fix.expected_mz_width_m, local_opt: false };
    let p_fb = evaluate_form(FormElement::Plane, &fix.points, &mz_chain, &stub_no_mz).unwrap();
    assert!(p_fb.value_si >= p_mz.value_si - 1e-15, "LS range {} < MZ {}", p_fb.value_si, p_mz.value_si);
    assert_eq!(p_fb.association, FormAssociation::LeastSquares);
    assert!(p_fb.flags.contains(&FormFlag::ConservativeUpperBound));

    // REQ-004: chain explicitly names least_squares -> no conservative_upper_bound flag.
    let p_ls = evaluate_form(FormElement::Plane, &fix.points, &ls_chain, &KrnFitProvider).unwrap();
    assert_eq!(p_ls.association, FormAssociation::LeastSquares);
    assert!(!p_ls.flags.contains(&FormFlag::ConservativeUpperBound));
    assert_eq!(p_ls.value_si.to_bits(), p_fb.value_si.to_bits());

    // AC-5: local_optimum_only flag propagates.
    let stub_local = StubKrn { mz_avail: true, mz_ref: mz_plane, mz_width: fix.expected_mz_width_m, local_opt: true };
    let p_loc = evaluate_form(FormElement::Plane, &fix.points, &mz_chain, &stub_local).unwrap();
    assert!(p_loc.flags.contains(&FormFlag::LocalOptimumOnly));

    // AC-6: deterministic digest across repeated calls.
    let p_ls_2 = evaluate_form(FormElement::Plane, &fix.points, &ls_chain, &KrnFitProvider).unwrap();
    assert_eq!(p_ls.digest().unwrap(), p_ls_2.digest().unwrap());
}

#[test]
fn ac3_error_paths_and_real_krn_circle2d_mz() {
    let fix = load_fixture();
    let mz_chain = chain_with("fit.plane.mz@1.0.0", Some(4));

    // AC-3: 3 points with row minimum 4 -> too_few_points{required: 4, given: 3}.
    let err = evaluate_form(FormElement::Plane, &fix.points[..3], &mz_chain, &KrnFitProvider).unwrap_err();
    assert_eq!(err, FormError::TooFewPoints { required: 4, given: 3 });
    assert_eq!(err.code(), "too_few_points");

    // NaN coordinate fails closed.
    let mut bad_pts = fix.points.clone();
    bad_pts[0][2] = f64::NAN;
    assert_eq!(
        evaluate_form(FormElement::Plane, &bad_pts, &mz_chain, &KrnFitProvider).unwrap_err(),
        FormError::NanInput
    );

    // Unknown association method fails closed.
    let bad_chain = chain_with("fit.plane.mystery@1.0.0", Some(4));
    assert_eq!(
        evaluate_form(FormElement::Plane, &fix.points, &bad_chain, &KrnFitProvider).unwrap_err().code(),
        "association_unknown"
    );

    // Real KrnFitProvider supports Circle2d MZ via fid-fit Voronoi solver (KRN.MZ.VOR)!
    let mut circle_pts = Vec::new();
    for i in 0..24 {
        let a = (i as f64) * core::f64::consts::TAU / 24.0;
        let dr = if i % 2 == 0 { 0.000004 } else { -0.000004 };
        let r = 0.010 + dr;
        circle_pts.push([r * a.cos(), r * a.sin(), 0.0]);
    }
    let circ_chain = chain_with("fit.circle2d.mz_vor@1.0.0", Some(4));
    let circ_res = evaluate_form(FormElement::Circle2d, &circle_pts, &circ_chain, &KrnFitProvider).unwrap();
    assert_eq!(circ_res.association, FormAssociation::MinimumZone);
    assert!(!circ_res.flags.contains(&FormFlag::ConservativeUpperBound));
    assert!((circ_res.value_si - 0.000008).abs() <= 1e-12);
}
