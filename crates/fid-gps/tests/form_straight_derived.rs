//! Acceptance tests for `GDT.FORM.STRAIGHT.DERIVED` (`AC-1`..`AC-4`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind, Param};
use fid_gps::form::straightness::derived::{
    chain_has_median_line_reduction, evaluate_derived_straightness, DerivedElement, DerivedStraightnessError,
    DerivedToleranceSpec, IdentityMedianLineDeriver, MaterialAllowance, MaterialConditionProvider, MaterialModifier,
    MedianLineDeriver, MzLine3dFitProvider, NoMaterialBonusProvider, MZ_LINE3D_ALGORITHM_ID,
};
use fid_gps::form::{FormAssociation, FormFlag, KrnFitProvider};
use std::collections::BTreeMap;

fn load_bowed_fixture() -> (Vec<[f64; 3]>, f64, usize, Vec<usize>) {
    let raw = include_str!("../../../fixtures/gdt/form/median_line_bowed.json");
    let doc = parse(raw.as_bytes()).unwrap();
    let CValue::Obj(root) = doc else { panic!("fixture must be object") };
    let CValue::Num(expected_diam) = root.get("expected_mz_diameter_m").unwrap() else {
        panic!("expected_mz_diameter_m")
    };
    let CValue::Num(apex_f) = root.get("planted_apex_index").unwrap() else { panic!("planted_apex_index") };
    let CValue::Arr(contacts_arr) = root.get("planted_contact_indices").unwrap() else { panic!("contacts") };
    let contacts: Vec<usize> = contacts_arr
        .iter()
        .map(|v| match v {
            CValue::Num(n) => *n as usize,
            _ => panic!("contact index"),
        })
        .collect();
    let CValue::Arr(pts_arr) = root.get("points").unwrap() else { panic!("points") };
    let points: Vec<[f64; 3]> = pts_arr
        .iter()
        .map(|item| {
            let CValue::Arr(c) = item else { panic!("point array") };
            let CValue::Num(x) = c[0] else { panic!("x") };
            let CValue::Num(y) = c[1] else { panic!("y") };
            let CValue::Num(z) = c[2] else { panic!("z") };
            [x, y, z]
        })
        .collect();
    (points, *expected_diam, *apex_f as usize, contacts)
}

fn median_line_chain(assoc_alg: &str) -> OperatorChain {
    OperatorChain::new(
        "default",
        vec![
            Operator::new(
                OperatorKind::Reduction,
                AlgId::parse("loc.median_line.ls_sections@1.0.0").unwrap(),
                BTreeMap::new(),
            )
            .unwrap(),
            Operator::new(OperatorKind::Association, AlgId::parse(assoc_alg).unwrap(), BTreeMap::new()).unwrap(),
        ],
    )
    .unwrap()
}

struct FixedBonusStub {
    bonus_si: f64,
}

impl MaterialConditionProvider for FixedBonusStub {
    fn compute_allowance(&self, spec: &DerivedToleranceSpec) -> Result<MaterialAllowance, String> {
        Ok(MaterialAllowance { stated_si: spec.stated_si, bonus_si: self.bonus_si })
    }
}

struct FailingDeriver {
    reason: &'static str,
}

impl MedianLineDeriver for FailingDeriver {
    fn derive(
        &self,
        _points: &[[f64; 3]],
        _reduction_alg: &str,
        _params: &BTreeMap<String, Param>,
    ) -> Result<DerivedElement, String> {
        Err(self.reason.to_owned())
    }
}

/// AC-1: Given `fixtures/gdt/form/median_line_bowed.json` (20 points on a circular bow with
/// planted zone diameter 0.010 mm) and a chain naming minimum zone, value = 0.010 mm within
/// `1e-12 m` and `extreme_index` is on the planted active contact set.
#[test]
fn ac1_median_line_bowed_mz_matches_0_010mm_within_1e_12() {
    let (points, expected_diam, apex_idx, contacts) = load_bowed_fixture();
    assert_eq!(points.len(), 20);
    assert_eq!(apex_idx, 10);

    let chain = median_line_chain(MZ_LINE3D_ALGORITHM_ID);
    assert!(chain_has_median_line_reduction(&chain));

    let tol = DerivedToleranceSpec { stated_si: 1.0e-5, modifier: MaterialModifier::Rfs };
    let payload = evaluate_derived_straightness(
        &points,
        &chain,
        &IdentityMedianLineDeriver,
        &MzLine3dFitProvider,
        &tol,
        &NoMaterialBonusProvider,
    )
    .unwrap();

    assert_eq!(payload.association, FormAssociation::MinimumZone);
    assert!(payload.flags.is_empty(), "exact MZ must not set conservative flag: {:?}", payload.flags);
    assert!(
        (payload.value_si - expected_diam).abs() <= 1.0e-12,
        "expected {expected_diam}, got {} (err {})",
        payload.value_si,
        (payload.value_si - expected_diam).abs()
    );
    assert!(
        contacts.contains(&payload.extreme_index),
        "extreme_index {} must be a planted contact point in {contacts:?}",
        payload.extreme_index
    );
    assert_eq!(payload.stated_si, 1.0e-5);
    assert_eq!(payload.bonus_si, None);
    assert_eq!(payload.point_count, 20);
}

/// AC-2: Given a KRN provider with MZ unavailable for 3D lines (`KrnFitProvider`), returns the
/// LS-referenced diameter (`>= 0.010 mm`) with `FormFlag::ConservativeUpperBound`.
#[test]
fn ac2_mz_unavailable_falls_back_to_ls_with_conservative_upper_bound() {
    let (points, expected_diam, _, _) = load_bowed_fixture();
    let chain = median_line_chain(MZ_LINE3D_ALGORITHM_ID);
    let tol = DerivedToleranceSpec { stated_si: 1.0e-5, modifier: MaterialModifier::Rfs };

    let payload = evaluate_derived_straightness(
        &points,
        &chain,
        &IdentityMedianLineDeriver,
        &KrnFitProvider,
        &tol,
        &NoMaterialBonusProvider,
    )
    .unwrap();

    assert_eq!(payload.association, FormAssociation::LeastSquares);
    assert!(payload.flags.contains(&FormFlag::ConservativeUpperBound), "fallback must carry conservative_upper_bound");
    assert!(
        payload.value_si + 1.0e-15 >= expected_diam,
        "LS diameter {} must be >= MZ diameter {expected_diam}",
        payload.value_si
    );
    assert!(payload.value_si > expected_diam + 1.0e-7, "LS line of circular bow strictly exceeds MZ width");
}

/// AC-3: Given an MMC characteristic and a material-condition stub returning bonus 0.004 mm,
/// the payload reports stated = 0.010 mm and bonus = 0.004 mm as separate fields.
#[test]
fn ac3_mmc_reports_stated_and_bonus_as_separate_fields() {
    let (points, expected_diam, _, _) = load_bowed_fixture();
    let chain = median_line_chain(MZ_LINE3D_ALGORITHM_ID);
    let tol = DerivedToleranceSpec { stated_si: 1.0e-5, modifier: MaterialModifier::Mmc };
    let matl = FixedBonusStub { bonus_si: 4.0e-6 };

    let payload =
        evaluate_derived_straightness(&points, &chain, &IdentityMedianLineDeriver, &MzLine3dFitProvider, &tol, &matl)
            .unwrap();

    assert!((payload.value_si - expected_diam).abs() <= 1.0e-12);
    assert_eq!(payload.stated_si, 1.0e-5);
    assert_eq!(payload.bonus_si, Some(4.0e-6));

    // Verify canonical CValue serialises stated_si and bonus_si as separate fields.
    let cv = payload.to_cvalue();
    let CValue::Obj(m) = cv else { panic!("payload object") };
    assert_eq!(m.get("stated_si"), Some(&CValue::Num(1.0e-5)));
    assert_eq!(m.get("bonus_si"), Some(&CValue::Num(4.0e-6)));
    assert!(payload.canonical_digest().is_ok());
}

/// AC-4: Given a `derive()` stub failing with `too_few_sections`, returns
/// `derivation_failed{reason: too_few_sections}` and produces no value.
#[test]
fn ac4_derivation_failure_passes_through_reason() {
    let (points, _, _, _) = load_bowed_fixture();
    let chain = median_line_chain(MZ_LINE3D_ALGORITHM_ID);
    let tol = DerivedToleranceSpec { stated_si: 1.0e-5, modifier: MaterialModifier::Rfs };
    let failing = FailingDeriver { reason: "too_few_sections" };

    let err =
        evaluate_derived_straightness(&points, &chain, &failing, &MzLine3dFitProvider, &tol, &NoMaterialBonusProvider)
            .unwrap_err();

    assert_eq!(err, DerivedStraightnessError::DerivationFailed { reason: "too_few_sections".to_owned() });
    assert_eq!(err.code(), "derivation_failed");
}
