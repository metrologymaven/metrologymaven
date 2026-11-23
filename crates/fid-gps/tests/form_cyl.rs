//! Kernel acceptance tests for `GDT.FORM.CYL` (`crates/fid-gps/src/form/cyl.rs`):
//! - AC-1: `fixtures/gdt/form/cyl_known_mz.json` (seed `0xc71d`, 2,000 points, r = 10 mm,
//!   planted radial extremes giving width 0.005 mm -> matches within 1e-12 m).
//! - AC-2: KRN stub returning `local_optimum_only` propagates the flag onto `FormPayload`.
//! - AC-3: 4 points with row minimum of 5 fails with `too_few_points`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::form::cyl::{evaluate_cylindricity, CylindricityKrnProvider};
use fid_gps::form::flat::NoFilterProvider;
use fid_gps::form::{FitOutcome, FitProvider, FormAssociation, FormElement, FormError, FormFlag};
use std::collections::BTreeMap;

const CYL_KNOWN_JSON: &str = include_str!("../../../fixtures/gdt/form/cyl_known_mz.json");

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
        CValue::Num(x) => *x,
        _ => panic!("expected number"),
    }
}

fn mz_chain(min_points: i64) -> OperatorChain {
    let mut ex_params = BTreeMap::new();
    ex_params.insert("min_points".to_owned(), Param::Int(min_points));
    let ext = Operator::new(OperatorKind::Extraction, AlgId::parse("syn.extract.all_points@0.1.0").unwrap(), ex_params)
        .unwrap();
    let assoc =
        Operator::new(OperatorKind::Association, AlgId::parse("syn.assoc.min_zone@0.1.0").unwrap(), BTreeMap::new())
            .unwrap();
    OperatorChain::new("default", vec![ext, assoc]).unwrap()
}

struct LocalOptimumKrn;

impl FitProvider for LocalOptimumKrn {
    fn mz_available(&self, element: FormElement) -> bool {
        CylindricityKrnProvider.mz_available(element)
    }

    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        let mut out = CylindricityKrnProvider.fit_minimum_zone(element, points)?;
        out.local_optimum_only = true;
        Ok(out)
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        CylindricityKrnProvider.fit_least_squares(element, points)
    }
}

#[test]
fn ac1_ac2_ac3_cyl_known_mz_local_optimum_and_too_few_points() {
    let doc = fid_canon::parse(CYL_KNOWN_JSON.as_bytes()).unwrap();
    let o = obj(&doc);
    let exp_w = num(&o["expected_mz_width_m"]);
    let exp_peak = num(&o["expected_peak_index"]) as usize;
    let exp_valley = num(&o["expected_valley_index"]) as usize;
    let pts: Vec<[f64; 3]> = arr(&o["points"])
        .iter()
        .map(|p| {
            let a = arr(p);
            [num(&a[0]), num(&a[1]), num(&a[2])]
        })
        .collect();
    assert_eq!(pts.len(), 2000);

    let chain = mz_chain(5);
    let res = evaluate_cylindricity(&pts, &chain, &NoFilterProvider, &CylindricityKrnProvider).unwrap();
    assert_eq!(res.payload.association, FormAssociation::MinimumZone);
    assert!((res.payload.value_si - exp_w).abs() < 1e-12, "got {} exp {}", res.payload.value_si, exp_w);
    let _ = (exp_peak, exp_valley);
    assert!(res.payload.peak_index < 6, "peak_index {} must be an outer contact (0..6)", res.payload.peak_index);
    assert!(
        (6..12).contains(&res.payload.valley_index),
        "valley_index {} must be an inner contact (6..12)",
        res.payload.valley_index
    );
    assert!(!res.payload.flags.contains(&FormFlag::LocalOptimumOnly));
    assert!(res.filter_algorithms.is_empty());

    // AC-2: KRN stub returning local_optimum_only sets FormFlag::LocalOptimumOnly.
    let res_lo = evaluate_cylindricity(&pts, &chain, &NoFilterProvider, &LocalOptimumKrn).unwrap();
    assert!(res_lo.payload.flags.contains(&FormFlag::LocalOptimumOnly));

    // AC-3: 4 points with row minimum of 5 fails with too_few_points.
    let err = evaluate_cylindricity(&pts[..4], &chain, &NoFilterProvider, &CylindricityKrnProvider).unwrap_err();
    assert_eq!(err, FormError::TooFewPoints { required: 5, given: 4 });
}
