//! Kernel acceptance tests for `GDT.FORM.FLAT` (`fid_gps::form::flat`): AC-1, AC-2, AC-4, REQ-002.
//! Uses `include_str!` so tests run identically on x86-64, ARM64, Windows and `wasm32-wasip1`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::form::flat::{evaluate_flatness, FilterProvider, FlatnessKrnProvider, NoFilterProvider};
use fid_gps::form::{FormAssociation, FormError, FormFlag, KrnFitProvider};
use std::collections::BTreeMap;

const FIXTURE_JSON: &str = include_str!("../../../fixtures/gdt/form/flat_known_mz.json");

fn load_fixture() -> (Vec<[f64; 3]>, f64, usize, usize) {
    let CValue::Obj(o) = parse(FIXTURE_JSON.as_bytes()).unwrap() else { panic!() };
    let num = |v: &CValue| match v {
        CValue::Num(x) => *x,
        _ => panic!(),
    };
    let CValue::Arr(pts_raw) = o.get("points").unwrap() else { panic!() };
    let pts: Vec<[f64; 3]> = pts_raw
        .iter()
        .map(|p| {
            let CValue::Arr(a) = p else { panic!() };
            [num(&a[0]), num(&a[1]), num(&a[2])]
        })
        .collect();
    let width = num(o.get("expected_mz_width_m").unwrap());
    let peak = num(o.get("expected_peak_index").unwrap()) as usize;
    let valley = num(o.get("expected_valley_index").unwrap()) as usize;
    (pts, width, peak, valley)
}

fn mz_chain() -> OperatorChain {
    let mut p = BTreeMap::new();
    p.insert("min_points".to_owned(), Param::Int(4));
    let op = Operator::new(OperatorKind::Association, AlgId::parse("fit.plane.mz@1.0.0").unwrap(), p).unwrap();
    OperatorChain::new("default", vec![op]).unwrap()
}

#[test]
fn ac1_ac2_ac4_flat_known_mz_exact_width_error_and_fallback() {
    let (pts, exp_w, exp_peak, exp_valley) = load_fixture();
    assert_eq!(pts.len(), 500);
    let chain = mz_chain();

    // AC-1: Exact KNP-1 MZ plane fit achieves 0.008 mm within 1e-12 m and identifies planted extremes.
    let res = evaluate_flatness(&pts, &chain, &NoFilterProvider, &FlatnessKrnProvider).unwrap();
    assert!((res.payload.value_si - exp_w).abs() <= 1e-12, "got {} want {}", res.payload.value_si, exp_w);
    assert_eq!(res.payload.association, FormAssociation::MinimumZone);
    assert_eq!(res.payload.peak_index, exp_peak);
    assert_eq!(res.payload.valley_index, exp_valley);
    assert!(res.payload.flags.is_empty());
    assert!(res.filter_algorithms.is_empty());

    // AC-2: 3 points fails with too_few_points.
    let err = evaluate_flatness(&pts[..3], &chain, &NoFilterProvider, &FlatnessKrnProvider).unwrap_err();
    assert_eq!(err, FormError::TooFewPoints { required: 4, given: 3 });

    // AC-4: KrnFitProvider (MZ unavailable for Plane) falls back to LS with conservative_upper_bound.
    let fb = evaluate_flatness(&pts, &chain, &NoFilterProvider, &KrnFitProvider).unwrap();
    assert_eq!(fb.payload.association, FormAssociation::LeastSquares);
    assert!(fb.payload.flags.contains(&FormFlag::ConservativeUpperBound));
    assert!(fb.payload.value_si >= exp_w - 1e-15);
}

#[test]
fn req002_optional_filter_applied_only_when_chain_names_it() {
    let (pts, _, _, _) = load_fixture();
    struct IdentityFilter;
    impl FilterProvider for IdentityFilter {
        fn apply(&self, _step: &Operator, points: &[[f64; 3]]) -> Result<Vec<[f64; 3]>, FormError> {
            Ok(points.to_vec())
        }
    }
    let f_op =
        Operator::new(OperatorKind::Filtration, AlgId::parse("filter.gaussian.linear@1.0.0").unwrap(), BTreeMap::new())
            .unwrap();
    let a_op =
        Operator::new(OperatorKind::Association, AlgId::parse("fit.plane.mz@1.0.0").unwrap(), BTreeMap::new()).unwrap();
    let chain = OperatorChain::new("filtered", vec![f_op, a_op]).unwrap();

    let res = evaluate_flatness(&pts, &chain, &IdentityFilter, &FlatnessKrnProvider).unwrap();
    assert_eq!(res.filter_algorithms, vec!["filter.gaussian.linear@1.0.0".to_owned()]);
}
