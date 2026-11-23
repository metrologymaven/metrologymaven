//! Acceptance tests for `GDT.FORM.STRAIGHT.LINEEL` (`crates/fid-gps/src/form/straightness/line_elements.rs`):
//! - AC-1: `fixtures/gdt/form/straight_lines_planted.json` (5 line elements at 2 mm spacing,
//!   element 3 = 0.006 mm, others = 0.002 mm -> values match within 1e-12 m, max = 0.006 mm,
//!   attaining element = 3).
//! - AC-2: narrower band half-width leaving element 5 with 1 point skips element 5 with
//!   `too_few_points` while evaluating elements 1..=4; skipping all fails with `no_line_elements`.
//! - AC-3: chain without `spacing` returns `extraction_params_missing{param: spacing}`.
//! - AC-4: KRN stub with MZ unavailable for `Line2d` flags every element `conservative_upper_bound`.
//! - AC-5: deterministic canonical digest is identical across repeated runs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::form::straightness::line_elements::{
    evaluate_line_elements, Line2dKrnProvider, LineElementError, LineElementParams,
};
use fid_gps::form::{FormAssociation, FormFlag, KrnFitProvider};
use std::collections::BTreeMap;

const STRAIGHT_JSON: &str = include_str!("../../../fixtures/gdt/form/straight_lines_planted.json");

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

fn vec3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    [num(&a[0]), num(&a[1]), num(&a[2])]
}

fn build_chain(
    plane_normal: [f64; 3],
    origin: [f64; 3],
    spacing: Option<f64>,
    range_start: i64,
    range_end: i64,
    band_half_width: f64,
    line_direction: [f64; 3],
) -> OperatorChain {
    let mut ex = BTreeMap::new();
    ex.insert("min_points".to_owned(), Param::Int(3));
    ex.insert("plane_normal_x".to_owned(), Param::F64Bits(plane_normal[0].to_bits()));
    ex.insert("plane_normal_y".to_owned(), Param::F64Bits(plane_normal[1].to_bits()));
    ex.insert("plane_normal_z".to_owned(), Param::F64Bits(plane_normal[2].to_bits()));
    ex.insert("origin_x".to_owned(), Param::F64Bits(origin[0].to_bits()));
    ex.insert("origin_y".to_owned(), Param::F64Bits(origin[1].to_bits()));
    ex.insert("origin_z".to_owned(), Param::F64Bits(origin[2].to_bits()));
    if let Some(sp) = spacing {
        ex.insert("spacing".to_owned(), Param::F64Bits(sp.to_bits()));
    }
    ex.insert("range_start".to_owned(), Param::Int(range_start));
    ex.insert("range_end".to_owned(), Param::Int(range_end));
    ex.insert("band_half_width".to_owned(), Param::F64Bits(band_half_width.to_bits()));
    ex.insert("line_direction_x".to_owned(), Param::F64Bits(line_direction[0].to_bits()));
    ex.insert("line_direction_y".to_owned(), Param::F64Bits(line_direction[1].to_bits()));
    ex.insert("line_direction_z".to_owned(), Param::F64Bits(line_direction[2].to_bits()));

    let ext =
        Operator::new(OperatorKind::Extraction, AlgId::parse("syn.extract.line_elements@0.1.0").unwrap(), ex).unwrap();
    let assoc =
        Operator::new(OperatorKind::Association, AlgId::parse("syn.assoc.min_zone@0.1.0").unwrap(), BTreeMap::new())
            .unwrap();
    OperatorChain::new("default", vec![ext, assoc]).unwrap()
}

#[test]
fn ac1_ac2_ac3_ac4_ac5_line_element_straightness() {
    let doc = fid_canon::parse(STRAIGHT_JSON.as_bytes()).unwrap();
    let o = obj(&doc);
    let plane_normal = vec3(&o["plane_normal"]);
    let origin = vec3(&o["origin"]);
    let spacing = num(&o["spacing_m"]);
    let range_start = num(&o["range_start"]) as i64;
    let range_end = num(&o["range_end"]) as i64;
    let band_hw = num(&o["band_half_width_m"]);
    let narrow_hw = num(&o["ac2_narrow_band_half_width_m"]);
    let line_direction = vec3(&o["line_direction"]);
    let exp_max = num(&o["expected_max_width_m"]);
    let exp_att = num(&o["expected_attaining_element"]) as i64;
    let exp_per = obj(&o["expected_per_element_m"]);
    let pts: Vec<[f64; 3]> = arr(&o["points"]).iter().map(vec3).collect();

    // AC-1: all 5 elements match planted values within 1e-12 m, max = 0.006 mm, attaining = 3.
    let chain = build_chain(plane_normal, origin, Some(spacing), range_start, range_end, band_hw, line_direction);
    let params = LineElementParams::from_chain(&chain).unwrap();
    let res = evaluate_line_elements(&pts, &params, &chain, &Line2dKrnProvider).unwrap();
    assert_eq!(res.attaining_element, exp_att);
    assert!((res.max_value_si - exp_max).abs() < 1e-12, "got {} exp {}", res.max_value_si, exp_max);
    assert_eq!(res.per_element.len(), 5);
    assert!(res.skipped_elements.is_empty());
    for el in &res.per_element {
        let exp = num(&exp_per[&el.index.to_string()]);
        assert_eq!(el.payload.association, FormAssociation::MinimumZone);
        assert!(el.flags.is_empty());
        assert!((el.value_si - exp).abs() < 1e-12, "el {}: got {} exp {}", el.index, el.value_si, exp);
    }

    // AC-5: deterministic canonical digest is bit-identical across runs.
    let res2 = evaluate_line_elements(&pts, &params, &chain, &Line2dKrnProvider).unwrap();
    assert_eq!(res.digest().unwrap(), res2.digest().unwrap());

    // AC-2: narrower band leaves element 5 with 1 point -> skipped with `too_few_points`, others evaluated.
    let narrow_chain =
        build_chain(plane_normal, origin, Some(spacing), range_start, range_end, narrow_hw, line_direction);
    let narrow_params = LineElementParams::from_chain(&narrow_chain).unwrap();
    let res_n = evaluate_line_elements(&pts, &narrow_params, &narrow_chain, &Line2dKrnProvider).unwrap();
    assert_eq!(res_n.per_element.len(), 4);
    assert_eq!(res_n.attaining_element, 3);
    assert_eq!(res_n.skipped_elements.len(), 1);
    assert_eq!(res_n.skipped_elements[0].index, 5);
    assert_eq!(res_n.skipped_elements[0].reason, "too_few_points");
    assert_eq!(res_n.skipped_elements[0].given, 1);

    // No remaining elements -> `no_line_elements`.
    let empty_chain = build_chain(plane_normal, [0.0, 1.0, 0.0], Some(spacing), 1, 5, narrow_hw, line_direction);
    let empty_params = LineElementParams::from_chain(&empty_chain).unwrap();
    let err_none = evaluate_line_elements(&pts, &empty_params, &empty_chain, &Line2dKrnProvider).unwrap_err();
    assert_eq!(err_none, LineElementError::NoLineElements);
    assert_eq!(err_none.code(), "no_line_elements");

    // AC-3: chain without `spacing` fails `extraction_params_missing{param: spacing}`.
    let no_spacing = build_chain(plane_normal, origin, None, range_start, range_end, band_hw, line_direction);
    let err_sp = LineElementParams::from_chain(&no_spacing).unwrap_err();
    assert_eq!(err_sp, LineElementError::ExtractionParamsMissing { param: "spacing" });
    assert_eq!(err_sp.to_string(), "extraction_params_missing{param: spacing}");

    // AC-4: KrnFitProvider reports MZ unavailable for Line2d -> LS fallback with conservative_upper_bound.
    let res_cub = evaluate_line_elements(&pts, &params, &chain, &KrnFitProvider).unwrap();
    assert_eq!(res_cub.per_element.len(), 5);
    for el in &res_cub.per_element {
        let exp = num(&exp_per[&el.index.to_string()]);
        assert_eq!(el.payload.association, FormAssociation::LeastSquares);
        assert!(el.flags.contains(&FormFlag::ConservativeUpperBound));
        assert!(el.value_si + 1e-15 >= exp);
    }
}
