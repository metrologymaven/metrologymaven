//! Acceptance criteria tests for spec node `UNC.DEC.RISK` (AC-1..AC-5, REQ-001..REQ-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kdec::eval::DEFAULT_WARN_RATIO;
use fid_kdec::risk::{
    acceptance_zone_for_rule, compute, global_risk, specific_risk, AcceptanceZone, Prior, RiskError, RiskFlag,
    ToleranceF64, ALGORITHM_ID,
};
use fid_kdec::{
    evaluate, DecisionRule, EvalInput, NotDecidableReason, Outcome, ParamDecimal, PdfSource, RiskReporting, RuleId,
    RuleParams, RuleParts, Tolerance,
};
use fid_kunc::{
    EvidenceClass, Expanded, Method, ModelRef, SourceKind, SourceRef, StatementParts, UncertaintyStatement,
};
use fid_num::kdec::Decimal;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/dec/risk/");

fn read_csv(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn parse_opt_f64(s: &str) -> Option<f64> {
    if s.is_empty() {
        None
    } else {
        Some(s.parse::<f64>().unwrap_or_else(|e| panic!("bad f64 {s:?}: {e}")))
    }
}

fn sample_statement(u_std: f64, expanded_u: f64) -> UncertaintyStatement {
    let mut p = StatementParts::new(EvidenceClass::Uec1);
    p.method = Some(Method::Lpu);
    p.unit = Some("si:m".to_owned());
    p.u_std = Some(u_std);
    p.expanded = Some(Expanded { value: expanded_u, k: 2.0 });
    p.lpu_validated_by_mc = Some(true);
    p.model = Some(ModelRef {
        id: "um:risk-test".to_owned(),
        semver: "1.0.0".to_owned(),
        digest: fid_kci::kci::sha256_digest(b"um:risk-test"),
    });
    p.sources.push(SourceRef {
        id: "src:cmm".to_owned(),
        kind: SourceKind::Dcc,
        digest: fid_kci::kci::sha256_digest(b"src:cmm"),
        unverified: false,
    });
    UncertaintyStatement::new(p).unwrap()
}

fn custom_rule(id: &str, params: RuleParams, risk_reporting: RiskReporting) -> DecisionRule {
    DecisionRule::customer(RuleParts {
        id: RuleId::parse(id).unwrap(),
        version: 1,
        standards_ref: None,
        params,
        pdf_source: PdfSource::McOrValidatedNormal,
        risk_reporting,
    })
    .unwrap()
}

#[test]
fn ac1_specific_risk_matches_fixture_exact_f64() {
    let text = read_csv("specific.csv");
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("case,outcome,p,expected_consumer,expected_producer"));

    let mut count = 0usize;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 5, "bad row: {line}");
        let case = cols[0];
        let outcome = match cols[1] {
            "CONFORMS" => Outcome::Conforms,
            "NONCONFORMS" => Outcome::Nonconforms,
            "INDETERMINATE" => Outcome::Indeterminate,
            "NOT_DECIDABLE" => Outcome::NotDecidable(NotDecidableReason::UMissing),
            other => panic!("unexpected outcome {other}"),
        };
        let p = parse_opt_f64(cols[2]);
        let exp_c = parse_opt_f64(cols[3]);
        let exp_p = parse_opt_f64(cols[4]);

        let (got_c, got_p) = specific_risk(outcome, p).unwrap();
        assert_eq!(got_c.map(f64::to_bits), exp_c.map(f64::to_bits), "consumer mismatch on {case}");
        assert_eq!(got_p.map(f64::to_bits), exp_p.map(f64::to_bits), "producer mismatch on {case}");
        count += 1;
    }
    assert!(count >= 10);
}

#[test]
fn ac2_global_risk_within_1e9_of_50_digit_references() {
    let text = read_csv("global.csv");
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("case,lsl,usl,guard_band,mu0,sigma0,u,global_consumer,global_producer"));

    let mut max_err = 0.0_f64;
    let mut count = 0usize;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 9, "bad row: {line}");
        let case = cols[0];
        let lsl = parse_opt_f64(cols[1]);
        let usl = parse_opt_f64(cols[2]);
        let gb = cols[3].parse::<f64>().unwrap();
        let mu0 = cols[4].parse::<f64>().unwrap();
        let sigma0 = cols[5].parse::<f64>().unwrap();
        let u = cols[6].parse::<f64>().unwrap();
        let ref_c = cols[7].parse::<f64>().unwrap();
        let ref_p = cols[8].parse::<f64>().unwrap();

        let tol = ToleranceF64::new(lsl, usl).unwrap();
        let zone = AcceptanceZone::from_guard_band(tol, gb).unwrap();
        let prior = Prior::new(mu0, sigma0, "spc-lot-2026-q4").unwrap();

        let (got_c, got_p) = global_risk(tol, zone, u, &prior).unwrap();
        let err_c = (got_c - ref_c).abs();
        let err_p = (got_p - ref_p).abs();
        if err_c > max_err {
            max_err = err_c;
        }
        if err_p > max_err {
            max_err = err_p;
        }
        assert!(err_c < 1e-9, "consumer error {err_c:e} >= 1e-9 on {case}: got {got_c}, ref {ref_c}");
        assert!(err_p < 1e-9, "producer error {err_p:e} >= 1e-9 on {case}: got {got_p}, ref {ref_p}");
        count += 1;
    }
    assert!(count >= 13);
    assert!(max_err < 1e-9);
}

#[test]
fn ac3_missing_prior_sets_prior_missing_flag_and_omits_global_figures() {
    let stmt = sample_statement(0.1, 0.2);
    let tol = Tolerance {
        lsl: Some(Decimal::parse("-1.0").unwrap()),
        usl: Some(Decimal::parse("1.0").unwrap()),
        unit: "si:m".to_owned(),
    };
    let rule = custom_rule(
        "dr:customer/acme/risk-global",
        RuleParams::GuardBand { r: Some(ParamDecimal::parse("1").unwrap()) },
        RiskReporting::SpecificAndGlobal,
    );
    let input = EvalInput {
        value: 0.0,
        statement: &stmt,
        tolerance: &tol,
        rule: &rule,
        warn_ratio: Decimal::parse(DEFAULT_WARN_RATIO).unwrap(),
    };
    let record = evaluate(&input).unwrap();
    assert_eq!(record.outcome(), Outcome::Conforms);

    let figs = compute(&record, &tol, &rule, 0.1, None).unwrap();
    assert!(figs.specific_consumer.is_some());
    assert!(figs.specific_producer.is_none());
    assert!(figs.global_consumer.is_none(), "must never assume a prior");
    assert!(figs.global_producer.is_none(), "must never assume a prior");
    assert!(figs.prior_source.is_none());
    assert!(figs.has_flag(RiskFlag::PriorMissing));

    // When prior is supplied, global figures and prior_source are recorded and flag is absent.
    let prior = Prior::new(0.0, 0.5, "SPC study #42 (lot A)").unwrap();
    let figs_with = compute(&record, &tol, &rule, 0.1, Some(&prior)).unwrap();
    assert!(!figs_with.has_flag(RiskFlag::PriorMissing));
    assert_eq!(figs_with.prior_source.as_ref().map(|s| s.as_raw()), Some("SPC study #42 (lot A)"));
    let gc = figs_with.global_consumer.unwrap();
    let gp = figs_with.global_producer.unwrap();
    // Matches two_sided_guard_pos_centered (w = 1.0 * U = 0.2).
    assert!((gc - 1.607_002_360_364_580_3e-4).abs() < 1e-9);
    assert!((gp - 7.132_490_112_070_144e-2).abs() < 1e-9);
}

#[test]
fn ac4_tiny_u_centered_process_both_global_risks_below_1e9() {
    // Tolerance width T = 2.0 ([-1.0, 1.0]), u = 1e-6 * T = 2e-6, centered Cp = 1.33 process (sigma0 = 0.25).
    let tol = ToleranceF64::new(Some(-1.0), Some(1.0)).unwrap();
    let zone = AcceptanceZone::from_guard_band(tol, 0.0).unwrap();
    let prior = Prior::new(0.0, 0.25, "capable-centered-process").unwrap();

    let (gc, gp) = global_risk(tol, zone, 2e-6, &prior).unwrap();
    assert!((0.0..1e-9).contains(&gc), "global consumer risk {gc:e} must be < 1e-9");
    assert!((0.0..1e-9).contains(&gp), "global producer risk {gp:e} must be < 1e-9");
}

#[test]
fn ac5_deterministic_bit_identical_output_and_error_paths() {
    assert_eq!(ALGORITHM_ID, "dec.risk.compute@1.0.0");

    let stmt = sample_statement(0.1, 0.2);
    let tol = Tolerance {
        lsl: Some(Decimal::parse("-1.0").unwrap()),
        usl: Some(Decimal::parse("1.0").unwrap()),
        unit: "si:m".to_owned(),
    };
    let rule = custom_rule(
        "dr:customer/acme/cp-global",
        RuleParams::ConformanceProbability { p_min: ParamDecimal::parse("0.95").unwrap() },
        RiskReporting::SpecificAndGlobal,
    );
    let input = EvalInput {
        value: 0.2,
        statement: &stmt,
        tolerance: &tol,
        rule: &rule,
        warn_ratio: Decimal::parse(DEFAULT_WARN_RATIO).unwrap(),
    };
    let record = evaluate(&input).unwrap();
    let prior = Prior::new(0.0, 0.5, "spc-ref").unwrap();

    let f1 = compute(&record, &tol, &rule, 0.1, Some(&prior)).unwrap();
    let f2 = compute(&record, &tol, &rule, 0.1, Some(&prior)).unwrap();
    assert_eq!(f1.to_canonical_bytes().unwrap(), f2.to_canonical_bytes().unwrap());
    assert_eq!(f1.digest().unwrap(), f2.digest().unwrap());

    // Conformance-probability acceptance zone shrinks symmetrically inward.
    let az = acceptance_zone_for_rule(ToleranceF64::new(Some(-1.0), Some(1.0)).unwrap(), &rule, 0.1, None).unwrap();
    assert!(az.lower.unwrap() > -1.0 && az.upper.unwrap() < 1.0);
    assert!((az.lower.unwrap() + az.upper.unwrap()).abs() < 1e-12);

    // Invalid prior / uncertainty / probability errors.
    assert_eq!(Prior::new(0.0, 0.0, "src").unwrap_err().code(), "invalid_prior");
    assert_eq!(Prior::new(0.0, -1.0, "src").unwrap_err().code(), "invalid_prior");
    assert_eq!(Prior::new(f64::NAN, 1.0, "src").unwrap_err().code(), "invalid_prior");
    assert_eq!(Prior::new(0.0, 1.0, "   ").unwrap_err().code(), "invalid_prior");
    assert_eq!(
        global_risk(
            ToleranceF64::new(Some(-1.0), Some(1.0)).unwrap(),
            AcceptanceZone { lower: Some(-1.0), upper: Some(1.0) },
            0.0,
            &prior
        )
        .unwrap_err()
        .code(),
        "invalid_uncertainty"
    );
    assert_eq!(
        specific_risk(Outcome::Conforms, Some(1.5)),
        Err(RiskError::InvalidProbability { reason: "out_of_range" })
    );
    assert_eq!(
        specific_risk(Outcome::Conforms, Some(f64::NAN)),
        Err(RiskError::InvalidProbability { reason: "non_finite" })
    );
}
