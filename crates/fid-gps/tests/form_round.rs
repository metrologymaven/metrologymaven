//! Kernel acceptance tests for `GDT.FORM.ROUND` (`crates/fid-gps/src/form/round.rs`):
//! - AC-1: `fixtures/gdt/form/round_known_mz.json` (three explicit 2D sections with planted
//!   widths 0.004, 0.006, 0.005 mm -> worst = 0.006 mm within 1e-12 m, all 3 listed).
//! - AC-2: `fixtures/gdt/form/round_from_cylinder_cloud.json` (3D cylinder cloud with sections
//!   at z = 5, 10, 15 mm, band half-width 0.1 mm -> matches within 1e-9 m, axis/positions/band recorded).
//! - AC-3: section 1 with 2 points -> `too_few_points{section: 1}`.
//! - Missing section spec on 3D input -> `section_spec_missing`.
//! - Tie breaking chooses lowest section index.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::form::round::{evaluate_roundness_3d, evaluate_roundness_sections, RoundnessError, SectionPoints};
use fid_gps::form::{FormAssociation, KrnFitProvider};
use std::collections::BTreeMap;

const ROUND_KNOWN_JSON: &str = include_str!("../../../fixtures/gdt/form/round_known_mz.json");
const ROUND_CLOUD_JSON: &str = include_str!("../../../fixtures/gdt/form/round_from_cylinder_cloud.json");

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

fn mz_chain(with_sections: bool) -> OperatorChain {
    let mut ex_params = BTreeMap::new();
    ex_params.insert("min_points".to_owned(), Param::Int(4));
    if with_sections {
        ex_params.insert("axis_fit".to_owned(), Param::Token("cylinder".to_owned()));
        ex_params.insert("band_half_width_m".to_owned(), Param::F64Bits(0.0001_f64.to_bits()));
        ex_params.insert("section_0_m".to_owned(), Param::F64Bits(0.005_f64.to_bits()));
        ex_params.insert("section_1_m".to_owned(), Param::F64Bits(0.010_f64.to_bits()));
        ex_params.insert("section_2_m".to_owned(), Param::F64Bits(0.015_f64.to_bits()));
    }
    let ext = Operator::new(OperatorKind::Extraction, AlgId::parse("syn.extract.sections@0.1.0").unwrap(), ex_params)
        .unwrap();
    let assoc =
        Operator::new(OperatorKind::Association, AlgId::parse("syn.assoc.min_zone@0.1.0").unwrap(), BTreeMap::new())
            .unwrap();
    OperatorChain::new("default", vec![ext, assoc]).unwrap()
}

#[test]
fn ac1_explicit_sections_match_known_mz_within_1e_12() {
    let doc = fid_canon::parse(ROUND_KNOWN_JSON.as_bytes()).unwrap();
    let o = obj(&doc);
    let exp_worst = num(&o["expected_worst_width_m"]);
    let exp_idx = num(&o["expected_worst_section_index"]) as usize;

    let mut sections = Vec::new();
    let mut exp_widths = Vec::new();
    for s in arr(&o["sections"]) {
        let so = obj(s);
        let pos = num(&so["position_m"]);
        exp_widths.push(num(&so["expected_mz_width_m"]));
        let pts: Vec<[f64; 2]> = arr(&so["points2d"])
            .iter()
            .map(|p| {
                let a = arr(p);
                [num(&a[0]), num(&a[1])]
            })
            .collect();
        sections.push(SectionPoints { position_m: pos, points2d: pts });
    }

    let res = evaluate_roundness_sections(&sections, &mz_chain(false), &KrnFitProvider).unwrap();
    assert_eq!(res.worst_section_index, exp_idx);
    assert!((res.value_si - exp_worst).abs() < 1e-12, "got {} exp {}", res.value_si, exp_worst);
    assert_eq!(res.sections.len(), 3);
    for (i, sec) in res.sections.iter().enumerate() {
        assert_eq!(sec.section_index, i);
        assert_eq!(sec.payload.association, FormAssociation::MinimumZone);
        assert!(
            (sec.payload.value_si - exp_widths[i]).abs() < 1e-12,
            "section {i}: got {} exp {}",
            sec.payload.value_si,
            exp_widths[i]
        );
    }
    assert!(res.extracted.is_none());
}

#[test]
fn ac2_cylinder_cloud_extraction_matches_planted_lobing_within_1e_9() {
    let doc = fid_canon::parse(ROUND_CLOUD_JSON.as_bytes()).unwrap();
    let o = obj(&doc);
    let exp_worst = num(&o["expected_worst_width_m"]);
    let exp_idx = num(&o["expected_worst_section_index"]) as usize;
    let exp_widths: Vec<f64> = arr(&o["expected_section_widths_m"]).iter().map(num).collect();
    let pts: Vec<[f64; 3]> = arr(&o["points"])
        .iter()
        .map(|p| {
            let a = arr(p);
            [num(&a[0]), num(&a[1]), num(&a[2])]
        })
        .collect();

    let res = evaluate_roundness_3d(&pts, &mz_chain(true), &KrnFitProvider).unwrap();
    assert_eq!(res.worst_section_index, exp_idx);
    assert!((res.value_si - exp_worst).abs() < 1e-9, "got {} exp {}", res.value_si, exp_worst);
    assert_eq!(res.sections.len(), 3);
    for (i, sec) in res.sections.iter().enumerate() {
        assert!(
            (sec.payload.value_si - exp_widths[i]).abs() < 1e-9,
            "section {i}: got {} exp {}",
            sec.payload.value_si,
            exp_widths[i]
        );
    }
    let ext = res.extracted.expect("3D extraction summary must be recorded");
    assert_eq!(ext.positions_m, vec![0.005, 0.010, 0.015]);
    assert!((ext.band_half_width_m - 0.0001).abs() < 1e-15);
    assert!(ext.axis.direction[2].abs() > 0.999999);
}

#[test]
fn ac3_error_paths_and_lowest_index_tie_breaking() {
    let doc = fid_canon::parse(ROUND_KNOWN_JSON.as_bytes()).unwrap();
    let s0_obj = obj(&arr(&obj(&doc)["sections"])[0]);
    let s0_pts: Vec<[f64; 2]> = arr(&s0_obj["points2d"])
        .iter()
        .map(|p| {
            let a = arr(p);
            [num(&a[0]), num(&a[1])]
        })
        .collect();

    // AC-3: section 1 has only 2 points -> too_few_points{section: 1}.
    let bad_sections = vec![
        SectionPoints { position_m: 0.005, points2d: s0_pts.clone() },
        SectionPoints { position_m: 0.010, points2d: vec![[0.01, 0.0], [-0.01, 0.0]] },
    ];
    let err = evaluate_roundness_sections(&bad_sections, &mz_chain(false), &KrnFitProvider).unwrap_err();
    assert_eq!(err.code(), "too_few_points");
    assert!(matches!(err, RoundnessError::TooFewPoints { section: 1, required: 4, given: 2 }));
    assert!(err.to_string().contains("section: 1"));

    // Missing section spec for 3D input -> section_spec_missing.
    let err3 = evaluate_roundness_3d(&[[0.01, 0.0, 0.005]; 8], &mz_chain(false), &KrnFitProvider).unwrap_err();
    assert_eq!(err3, RoundnessError::SectionSpecMissing);
    assert_eq!(err3.code(), "section_spec_missing");

    // Equal sections -> lowest section index wins ties.
    let tied = vec![
        SectionPoints { position_m: 0.005, points2d: s0_pts.clone() },
        SectionPoints { position_m: 0.010, points2d: s0_pts },
    ];
    let res = evaluate_roundness_sections(&tied, &mz_chain(false), &KrnFitProvider).unwrap();
    assert_eq!(res.worst_section_index, 0);
}
