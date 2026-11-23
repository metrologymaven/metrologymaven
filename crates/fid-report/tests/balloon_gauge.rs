//! Acceptance tests for `RPT.BALLOON.GAUGE` (`AC-1`..`AC-5`, `REQ-001`..`REQ-006`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_report::balloon::gauge::{
    check, check_gauge_def, max_decimals_for_unit, CalendarDate, CalibrationStatus, GaugeError, GaugeRecordView,
    GaugeStatus, GaugeUncertainty, GaugeUnit, InstrumentModelLink, MemoryGaugeRegister, ResolutionSpec,
};
use fid_report::fmt::GaugeId;
use std::time::Instant;

/// Fixture `fixtures/rpt/balloon/gauge_register`:
/// - G-01: caliper, resolution 0.01 mm, due 2026-12-31, active, no model
/// - G-02: micrometer, resolution 0.001 mm, due 2026-06-30, active, accepted model `unc.inst.micrometer@1` (UEC-2)
/// - G-03: retired height gauge, resolution 0.01 mm, no due date, retired
fn build_gauge_register(with_model_on_g02: bool) -> MemoryGaugeRegister {
    let mut reg = MemoryGaugeRegister::new();
    reg.insert(GaugeRecordView {
        id: GaugeId::new("G-01").unwrap(),
        kind: "caliper".into(),
        resolution: ResolutionSpec::parse("0.01", GaugeUnit::Mm).unwrap(),
        calibration_due: Some(CalendarDate::parse("2026-12-31").unwrap()),
        certificate_ref: Some("CERT-2026-001".into()),
        status: GaugeStatus::Active,
        model: None,
    });
    reg.insert(GaugeRecordView {
        id: GaugeId::new("G-02").unwrap(),
        kind: "micrometer".into(),
        resolution: ResolutionSpec::parse("0.001", GaugeUnit::Mm).unwrap(),
        calibration_due: Some(CalendarDate::parse("2026-06-30").unwrap()),
        certificate_ref: Some("CERT-2026-002".into()),
        status: GaugeStatus::Active,
        model: with_model_on_g02.then(|| InstrumentModelLink {
            model_ref: "unc.inst.micrometer@1".into(),
            accepted: true,
            assigned_uec: "UEC-2".into(),
        }),
    });
    reg.insert(GaugeRecordView {
        id: GaugeId::new("G-03").unwrap(),
        kind: "height_gauge".into(),
        resolution: ResolutionSpec::parse("0.01", GaugeUnit::Mm).unwrap(),
        calibration_due: None,
        certificate_ref: None,
        status: GaugeStatus::Retired,
        model: None,
    });
    reg
}

#[test]
fn ac1_ac2_ac3_ac4_ac5_gauge_checks_and_command_def() {
    let reg = build_gauge_register(true);
    let g01 = GaugeId::new("G-01").unwrap();
    let g02 = GaugeId::new("G-02").unwrap();
    let g03 = GaugeId::new("G-03").unwrap();
    let g99 = GaugeId::new("G-99").unwrap();

    // AC-1: G-01 on exact due date 2026-12-31 in mm -> current, 2 decimals, UEC-0.
    let r1 = check(&reg, &g01, CalendarDate::parse("2026-12-31").unwrap(), Some(GaugeUnit::Mm)).unwrap();
    assert_eq!(r1.status, GaugeStatus::Active);
    assert_eq!(r1.calibration, CalibrationStatus::Current);
    assert_eq!(r1.max_decimals, Some(2));
    assert_eq!(r1.uncertainty, GaugeUncertainty::Uec0);

    // AC-2a: G-01 on 2027-01-01 -> overdue.
    let r2a = check(&reg, &g01, CalendarDate::parse("2027-01-01").unwrap(), Some(GaugeUnit::Mm)).unwrap();
    assert_eq!(r2a.calibration, CalibrationStatus::Overdue);

    // AC-2b: G-02 (0.001 mm) checked in inches -> 5 decimals (0.00003937... in, smallest d with 10^-d <= res is 5).
    let r2b = check(&reg, &g02, CalendarDate::parse("2026-06-30").unwrap(), Some(GaugeUnit::In)).unwrap();
    assert_eq!(r2b.calibration, CalibrationStatus::Current);
    assert_eq!(r2b.max_decimals, Some(5));

    // AC-3: G-02 linked to accepted instrument model reports model_ref and assigned UEC; unaccepted model falls back to UEC-0.
    assert_eq!(
        r2b.uncertainty,
        GaugeUncertainty::Model { model_ref: "unc.inst.micrometer@1".into(), uec: "UEC-2".into() }
    );
    let mut reg_unacc = build_gauge_register(false);
    reg_unacc.insert(GaugeRecordView {
        id: GaugeId::new("G-04").unwrap(),
        kind: "micrometer".into(),
        resolution: ResolutionSpec::parse("0.001", GaugeUnit::Mm).unwrap(),
        calibration_due: None,
        certificate_ref: None,
        status: GaugeStatus::Active,
        model: Some(InstrumentModelLink {
            model_ref: "unc.inst.draft@1".into(),
            accepted: false,
            assigned_uec: "UEC-2".into(),
        }),
    });
    let r_unacc =
        check(&reg_unacc, &GaugeId::new("G-04").unwrap(), CalendarDate::parse("2026-05-01").unwrap(), None).unwrap();
    assert_eq!(r_unacc.uncertainty, GaugeUncertainty::Uec0);
    assert_eq!(r_unacc.calibration, CalibrationStatus::NoDueDate);
    assert_eq!(r_unacc.max_decimals, None);

    // AC-4: Retired G-03 still resolves and reports status=retired; unknown G-99 fails with report.gauge_unknown.
    let r3 = check(&reg, &g03, CalendarDate::parse("2026-05-01").unwrap(), Some(GaugeUnit::Mm)).unwrap();
    assert_eq!(r3.status, GaugeStatus::Retired);
    assert_eq!(r3.calibration, CalibrationStatus::NoDueDate);
    let err_unknown = check(&reg, &g99, CalendarDate::parse("2026-05-01").unwrap(), None).unwrap_err();
    assert_eq!(err_unknown.code(), "report.gauge_unknown");

    // Error paths: unit_incompatible and resolution_too_fine.
    let err_unit = check(&reg, &g01, CalendarDate::parse("2026-05-01").unwrap(), Some(GaugeUnit::Deg)).unwrap_err();
    assert_eq!(err_unit.code(), "report.unit_incompatible");
    let fine = ResolutionSpec::parse("0.0000000001", GaugeUnit::Mm).unwrap();
    let err_fine = max_decimals_for_unit(fine, GaugeUnit::Mm).unwrap_err();
    assert_eq!(err_fine, GaugeError::ResolutionTooFine);
    assert_eq!(err_fine.code(), "report.resolution_too_fine");

    // AC-5: report.check_gauge@1 is R0, D0, zero journal effects.
    let def = check_gauge_def().expect("valid CommandDef");
    assert_eq!(def.id, "report.check_gauge");
    assert_eq!(def.major, 1);
    assert_eq!(def.side_effect, fid_types::SideEffect::R0);
    assert_eq!(def.determinism, fid_types::Determinism::D0);
    assert!(def.effects.is_empty(), "R0 command must write nothing to Record Journal");

    // Performance check: 1000 checks well under 100 ms (<0.1 ms/check).
    let date = CalendarDate::parse("2026-05-01").unwrap();
    let t0 = Instant::now();
    for _ in 0..1_000 {
        let _ = check(&reg, &g02, date, Some(GaugeUnit::In)).unwrap();
    }
    assert!(t0.elapsed().as_millis() < 100);
}
