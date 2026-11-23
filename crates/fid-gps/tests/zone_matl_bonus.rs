//! Acceptance tests for `GDT.ZONE.MATL.BONUS` (`fid-gps::zone::matl`): AC-1..AC-4.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::zone::matl::{bonus, ActualSize, BonusFlag, FeatureKind, MaterialCondition, MatlError};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TOL: f64 = 1e-15;

fn fix(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/zone/matl").join(sub)
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

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("expected string"),
    }
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        _ => panic!("expected number"),
    }
}

fn make_chain(actual_def: Option<&str>, bonus_cap: Option<&str>) -> OperatorChain {
    let mut params = BTreeMap::new();
    if let Some(def) = actual_def {
        params.insert("actual_size_def".to_owned(), Param::Token(def.to_owned()));
    }
    if let Some(cap) = bonus_cap {
        params.insert("bonus_cap".to_owned(), Param::Token(cap.to_owned()));
    }
    let op = Operator::new(OperatorKind::Evaluation, AlgId::parse("syn.eval.bonus@1.0.0").unwrap(), params).unwrap();
    OperatorChain::new("bonus_test", vec![op]).unwrap()
}

#[test]
fn ac1_ac2_truth_table_and_opposite_limit_cap() {
    let doc = fid_canon::parse(&std::fs::read(fix("bonus_table.json")).unwrap()).unwrap();
    let root = obj(&doc);
    let def_id = st(&root["actual_size_def"]);

    for case in arr(&root["cases"]) {
        let c = obj(case);
        let name = st(&c["name"]);
        let kind = FeatureKind::parse(st(&c["kind"])).unwrap();
        let cond = MaterialCondition::parse(st(&c["cond"])).unwrap();
        let stated = num(&c["stated_m"]);
        let mmc = num(&c["mmc_m"]);
        let lmc = num(&c["lmc_m"]);
        let actual = num(&c["actual_m"]);
        let cap = match c.get("bonus_cap") {
            Some(CValue::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let chain = make_chain(Some(def_id), cap);

        let rec = bonus(stated, mmc, lmc, kind, cond, ActualSize::new(actual), &chain).unwrap();
        assert!((rec.stated_m - stated).abs() <= TOL, "{name}: stated");
        assert!((rec.bonus_m - num(&c["expected_bonus_m"])).abs() <= TOL, "{name}: bonus {}", rec.bonus_m);
        assert!((rec.total_m - num(&c["expected_total_m"])).abs() <= TOL, "{name}: total {}", rec.total_m);
        assert!((rec.actual_m - actual).abs() <= TOL, "{name}: actual");
        assert_eq!(rec.actual_def, def_id, "{name}: actual_def");

        let exp_flags: Vec<&str> = arr(&c["expected_flags"]).iter().map(st).collect();
        let got_flags: Vec<&str> = rec.flags.iter().map(|f| f.as_str()).collect();
        assert_eq!(got_flags, exp_flags, "{name}: flags");
    }
}

#[test]
fn ac3_local_optimum_only_passthrough() {
    let chain = make_chain(Some("size.ame"), None);
    let rec = bonus(
        0.00005,
        0.01,
        0.0101,
        FeatureKind::Internal,
        MaterialCondition::Mmc,
        ActualSize::new(0.01004).with_local_optimum_only(true),
        &chain,
    )
    .unwrap();
    assert!(rec.flags.contains(&BonusFlag::LocalOptimumOnly));
    assert!((rec.bonus_m - 0.00004).abs() <= TOL);
}

#[test]
fn ac4_error_paths() {
    let chain_ok = make_chain(Some("size.ame"), None);
    let err_nan = bonus(
        0.00005,
        0.01,
        0.0101,
        FeatureKind::Internal,
        MaterialCondition::Mmc,
        ActualSize::new(f64::NAN),
        &chain_ok,
    )
    .unwrap_err();
    assert_eq!(err_nan.code(), "bad_material_input");

    let err_neg = bonus(
        -0.00001,
        0.01,
        0.0101,
        FeatureKind::Internal,
        MaterialCondition::Mmc,
        ActualSize::new(0.01004),
        &chain_ok,
    )
    .unwrap_err();
    assert_eq!(err_neg.code(), "bad_material_input");

    let chain_missing = make_chain(None, None);
    let err_undef = bonus(
        0.00005,
        0.01,
        0.0101,
        FeatureKind::Internal,
        MaterialCondition::Mmc,
        ActualSize::new(0.01004),
        &chain_missing,
    )
    .unwrap_err();
    assert_eq!(err_undef, MatlError::ActualSizeUndefined);
    assert_eq!(err_undef.code(), "actual_size_undefined");
}
