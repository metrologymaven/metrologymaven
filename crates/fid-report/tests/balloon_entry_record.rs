//! Acceptance tests for `RPT.BALLOON.ENTRY.RECORD` (`AC-1`..`AC-7`, `REQ-001`..`REQ-010`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gdt::chars::dprof::CharId;
use fid_gdt::chars::record::{EvaluationBasis, RunId};
use fid_io_reg::Principal;
use fid_kdec::{evaluate, Catalog as DecCatalog, EvalInput, RuleRef, Tolerance};
use fid_kunc::{EvidenceClass, StatementParts, Uec0Reason, UncertaintyStatement};
use fid_num::kdec::Decimal;
use fid_registry::Catalogue;
use fid_replay::ReplayOutcome;
use fid_report::balloon::entry::{
    command_defs, replay_exact, AuthoredQuantity, DirectReadingEvaluator, EnterReadingParams, EntryError,
    EntryTimestamp, ManualReadingEvent, ManualReadingStore, PrincipalId, ReadingFlag, ResultRecordRef, StageId,
    VoidReadingParams, WARN_GAUGE_CALIBRATION_OVERDUE, WARN_READING_EXCEEDS_RESOLUTION,
};
use fid_report::balloon::gauge::{
    CalendarDate, GaugeRecordView, GaugeStatus, GaugeUncertainty, GaugeUnit, MemoryGaugeRegister, ResolutionSpec,
};
use fid_report::balloon::sheet::{digest_bytes, DeterministicV7Gen};
use fid_report::fmt::GaugeId;

/// Test evaluator bridging real `fid-kdec` + `GDT.CHAR.RECORD` semantics (C1: UNC decides, never RPT).
struct TestUncEvaluator;

impl DirectReadingEvaluator for TestUncEvaluator {
    fn expected_quantity_kind(&self, char_id: &CharId) -> Option<&str> {
        match char_id.as_str() {
            "C1" | "C2" | "C3" => Some("length"),
            _ => None,
        }
    }

    #[allow(clippy::disallowed_types)]
    fn build_direct_reading_record(
        &self,
        char_id: &CharId,
        run: &RunId,
        _stage: Option<&StageId>,
        authored: &AuthoredQuantity,
        gauge: &GaugeId,
        uncertainty: &GaugeUncertainty,
    ) -> Result<ResultRecordRef, EntryError> {
        let (uec, model_ref) = match uncertainty {
            GaugeUncertainty::Uec0 => ("UEC-0".to_string(), None),
            GaugeUncertainty::Model { model_ref, uec } => (uec.clone(), Some(model_ref.clone())),
        };
        let rule_ref = RuleRef::parse("dr:simple-acceptance@1").unwrap();
        let rule = DecCatalog::builtin().unwrap().resolve(&rule_ref).unwrap();
        let mut parts = StatementParts::new(EvidenceClass::Uec0);
        parts.uec0_reason = Some(Uec0Reason::UMissing);
        let stmt = UncertaintyStatement::new(parts).unwrap();
        let tol = Tolerance {
            lsl: Some(Decimal::parse("0.012000").unwrap()),
            usl: Some(Decimal::parse("0.012018").unwrap()),
            unit: "si:m".to_string(),
        };
        let input = EvalInput {
            value: f64::from_bits(authored.si_bits),
            statement: &stmt,
            tolerance: &tol,
            rule: &rule,
            warn_ratio: Decimal::parse("0.25").unwrap(),
        };
        let rec = evaluate(&input).unwrap();
        let digest_input = format!(
            "{}:{}:{}:{:016x}:{}:{}:{}",
            char_id.as_str(),
            run.as_str(),
            authored.raw_literal.untrusted_value().as_str(),
            authored.si_bits,
            gauge.as_str(),
            uec,
            rec.outcome().wire()
        );
        Ok(ResultRecordRef {
            record_digest: digest_bytes(digest_input.as_bytes()),
            basis: EvaluationBasis::DirectReading,
            uncertainty_class: uec,
            model_ref,
            outcome: rec.outcome().wire().to_string(),
            rule: Some(rule_ref.to_string()),
        })
    }
}

fn build_fixture() -> (Catalogue, MemoryGaugeRegister, TestUncEvaluator, ManualReadingStore, DeterministicV7Gen) {
    let cat = Catalogue::geo_frame().expect("geo_frame catalogue");
    let mut reg = MemoryGaugeRegister::new();
    reg.insert(GaugeRecordView {
        id: GaugeId::new("G-02").unwrap(),
        kind: "micrometer".into(),
        resolution: ResolutionSpec::parse("0.001", GaugeUnit::Mm).unwrap(),
        calibration_due: Some(CalendarDate::parse("2026-06-30").unwrap()),
        certificate_ref: Some("CERT-2026-002".into()),
        status: GaugeStatus::Active,
        model: None,
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
    (cat, reg, TestUncEvaluator, ManualReadingStore::new(), DeterministicV7Gen::new(1_760_000_000_000))
}

#[test]
fn ac1_to_ac7_full_manual_reading_lifecycle_principal_void_and_replay() {
    let (cat, reg, eval, mut store, mut ids) = build_fixture();
    let c1 = CharId::parse("C1").unwrap();
    let r1 = RunId::parse("R1").unwrap();
    let g02 = GaugeId::new("G-02").unwrap();
    let g03 = GaugeId::new("G-03").unwrap();
    let op = PrincipalId::new("operator:alice").unwrap();
    let ts_ok = EntryTimestamp::parse("2026-05-01T14:22:00Z").unwrap();

    // AC-1: Human enters "12.009 mm" with G-02 on 2026-05-01 -> DIRECT_READING, literal preserved, UEC-0, UNC outcome.
    let res1 = store
        .enter_reading(
            Principal::Human,
            &cat,
            &reg,
            &eval,
            &mut ids,
            &EnterReadingParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                stage: None,
                value: "12.009 mm".into(),
                gauge: Some(g02.clone()),
                note: Some("clean bore".into()),
                operator: op.clone(),
                entered_at: ts_ok.clone(),
                dry_run: false,
            },
        )
        .expect("AC-1 enter reading");
    assert!(res1.committed);
    assert!(res1.warnings.is_empty());
    assert!(res1.reading.flags.is_empty());
    assert_eq!(res1.reading.result.basis, EvaluationBasis::DirectReading);
    assert_eq!(res1.reading.literal.raw_literal.untrusted_value().as_str(), "12.009 mm");
    assert_eq!(res1.reading.literal.decimal, "12.009");
    assert_eq!(res1.reading.literal.unit, "mm");
    assert_eq!(res1.reading.gauge, g02);
    assert_eq!(res1.reading.result.uncertainty_class, "UEC-0");
    assert_eq!(store.journal().len(), 1);

    // AC-2: Missing gauge -> report.reading_without_gauge; "12.0O9 mm" -> reading_invalid; "12.009 s" -> reading_invalid; retired gauge -> gauge_retired.
    let jlen = store.journal().len();
    let e_nogauge = store
        .enter_reading(
            Principal::Human,
            &cat,
            &reg,
            &eval,
            &mut ids,
            &EnterReadingParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                stage: None,
                value: "12.009 mm".into(),
                gauge: None,
                note: None,
                operator: op.clone(),
                entered_at: ts_ok.clone(),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(e_nogauge, EntryError::ReadingWithoutGauge);
    assert_eq!(e_nogauge.code(), "report.reading_without_gauge");

    let e_typo = store
        .enter_reading(
            Principal::Human,
            &cat,
            &reg,
            &eval,
            &mut ids,
            &EnterReadingParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                stage: None,
                value: "12.0O9 mm".into(),
                gauge: Some(g02.clone()),
                note: None,
                operator: op.clone(),
                entered_at: ts_ok.clone(),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(e_typo.code(), "report.reading_invalid");

    let e_sec = store
        .enter_reading(
            Principal::Human,
            &cat,
            &reg,
            &eval,
            &mut ids,
            &EnterReadingParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                stage: None,
                value: "12.009 s".into(),
                gauge: Some(g02.clone()),
                note: None,
                operator: op.clone(),
                entered_at: ts_ok.clone(),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(e_sec.code(), "report.reading_invalid");

    let e_retired = store
        .enter_reading(
            Principal::Human,
            &cat,
            &reg,
            &eval,
            &mut ids,
            &EnterReadingParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                stage: None,
                value: "12.009 mm".into(),
                gauge: Some(g03),
                note: None,
                operator: op.clone(),
                entered_at: ts_ok.clone(),
                dry_run: false,
            },
        )
        .unwrap_err();
    assert_eq!(e_retired.code(), "report.gauge_retired");
    assert_eq!(store.journal().len(), jlen, "no event journaled on any AC-2 error");

    // AC-3: Entry date 2026-07-01 (after 2026-06-30) -> flag gauge_calibration_overdue + warning.
    let ts_overdue = EntryTimestamp::parse("2026-07-01T08:00:00Z").unwrap();
    let res3 = store
        .enter_reading(
            Principal::Human,
            &cat,
            &reg,
            &eval,
            &mut ids,
            &EnterReadingParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                stage: None,
                value: "12.008 mm".into(),
                gauge: Some(g02.clone()),
                note: None,
                operator: op.clone(),
                entered_at: ts_overdue,
                dry_run: false,
            },
        )
        .expect("AC-3 overdue");
    assert!(res3.reading.flags.contains(&ReadingFlag::GaugeCalibrationOverdue));
    assert!(res3.warnings.iter().any(|w| w.code == WARN_GAUGE_CALIBRATION_OVERDUE));

    // AC-4: "12.0091 mm" with 0.001 mm micrometer G-02 -> exceeds_resolution flag + warning, literal kept verbatim!
    let res4 = store
        .enter_reading(
            Principal::Human,
            &cat,
            &reg,
            &eval,
            &mut ids,
            &EnterReadingParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                stage: None,
                value: "12.0091 mm".into(),
                gauge: Some(g02.clone()),
                note: None,
                operator: op.clone(),
                entered_at: ts_ok.clone(),
                dry_run: false,
            },
        )
        .expect("AC-4 exceeds resolution");
    assert!(res4.reading.flags.contains(&ReadingFlag::ExceedsResolution));
    assert!(res4.warnings.iter().any(|w| w.code == WARN_READING_EXCEEDS_RESOLUTION));
    assert_eq!(res4.reading.literal.raw_literal.untrusted_value().as_str(), "12.0091 mm");

    // AC-5: Agent and Script principals refused for BOTH enter_reading and void_reading; journal unchanged.
    let jlen_p = store.journal().len();
    for bad_principal in [Principal::Agent, Principal::Script] {
        let e1 = store
            .enter_reading(
                bad_principal,
                &cat,
                &reg,
                &eval,
                &mut ids,
                &EnterReadingParams {
                    characteristic: c1.clone(),
                    run: r1.clone(),
                    stage: None,
                    value: "12.005 mm".into(),
                    gauge: Some(g02.clone()),
                    note: None,
                    operator: op.clone(),
                    entered_at: ts_ok.clone(),
                    dry_run: false,
                },
            )
            .unwrap_err();
        assert_eq!(e1, EntryError::HumanOnly);
        let e2 = store
            .void_reading(
                bad_principal,
                &VoidReadingParams {
                    reading: res1.reading.id,
                    reason: "gauge dropped".into(),
                    by: op.clone(),
                    at: ts_ok.clone(),
                },
            )
            .unwrap_err();
        assert_eq!(e2, EntryError::HumanOnly);
    }
    assert_eq!(store.journal().len(), jlen_p);

    // AC-6: Void first reading with reason "gauge dropped" -> list_reading includes it as voided,
    // original EnterReading event stays in journal, active_readings excludes it!
    let voided = store
        .void_reading(
            Principal::Human,
            &VoidReadingParams {
                reading: res1.reading.id,
                reason: "gauge dropped".into(),
                by: op.clone(),
                at: EntryTimestamp::parse("2026-05-01T14:25:00Z").unwrap(),
            },
        )
        .expect("AC-6 void");
    assert!(voided.is_voided());
    assert_eq!(voided.void_info.as_ref().unwrap().reason.untrusted_value().as_str(), "gauge dropped");

    let listed = store.list_reading(Some(&r1), None);
    assert_eq!(listed.len(), 3);
    assert!(listed[0].is_voided());
    assert!(!listed[1].is_voided());
    assert!(!listed[2].is_voided());

    let active = store.active_readings(Some(&r1), None);
    assert_eq!(active.len(), 2);
    assert!(active.iter().all(|r| r.id != res1.reading.id));
    assert_eq!(store.journal().len(), 4, "original 3 enter events + 1 void event preserved");

    // Canonical event roundtrip & AC-7 exact replay -> IDENTICAL!
    let roundtripped: Vec<ManualReadingEvent> = store
        .journal()
        .iter()
        .map(|ev| ManualReadingEvent::from_value(&ev.to_value()).expect("roundtrip event"))
        .collect();
    assert_eq!(roundtripped, store.journal());
    let run = replay_exact(&roundtripped);
    assert_eq!(run.outcome, ReplayOutcome::Identical, "mismatch at {:?}", run.first_mismatch);
    assert_eq!(run.store.list_reading(Some(&r1), None), listed);
    assert_eq!(run.store.active_readings(Some(&r1), None), active);

    // CommandDefs validate and declare human_only: true + request_only on enter & void.
    let defs = command_defs().expect("valid CommandDefs");
    assert_eq!(defs.len(), 3);
    assert!(defs[0].human_only && defs[1].human_only && !defs[2].human_only);
}
