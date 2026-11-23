//! Acceptance tests for `RPT.BALLOON.ENTRY.SERIES` (`AC-1`..`AC-5`, `REQ-001`..`REQ-005`).
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
    enter_series_command_def, enter_series_reading, replay_exact, AuthoredQuantity, DirectReadingEvaluator,
    EnterSeriesParams, EntryError, EntryTimestamp, ManualReadingEvent, ManualReadingStore, PrincipalId,
    ResultRecordRef, RunStageLookup, SeriesError, StageId, WARN_READING_EXCEEDS_RESOLUTION,
};
use fid_report::balloon::gauge::{
    CalendarDate, GaugeRecordView, GaugeStatus, GaugeUncertainty, GaugeUnit, MemoryGaugeRegister, ResolutionSpec,
};
use fid_report::balloon::sheet::{digest_bytes, DeterministicV7Gen};
use fid_report::fmt::GaugeId;
use std::collections::BTreeSet;
use std::time::Instant;

struct FivePartStages {
    run: RunId,
    stages: BTreeSet<StageId>,
}

impl RunStageLookup for FivePartStages {
    fn contains_stage(&self, run: &RunId, stage: &StageId) -> bool {
        run == &self.run && self.stages.contains(stage)
    }
}

struct SeriesEvaluator;

impl DirectReadingEvaluator for SeriesEvaluator {
    fn expected_quantity_kind(&self, char_id: &CharId) -> Option<&str> {
        (char_id.as_str() == "C1").then_some("length")
    }

    #[allow(clippy::disallowed_types)]
    fn build_direct_reading_record(
        &self,
        char_id: &CharId,
        run: &RunId,
        stage: Option<&StageId>,
        authored: &AuthoredQuantity,
        gauge: &GaugeId,
        uncertainty: &GaugeUncertainty,
    ) -> Result<ResultRecordRef, EntryError> {
        let uec = match uncertainty {
            GaugeUncertainty::Uec0 => "UEC-0".to_string(),
            GaugeUncertainty::Model { uec, .. } => uec.clone(),
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
        let key = format!(
            "{}:{}:{}:{:016x}:{}",
            char_id.as_str(),
            run.as_str(),
            stage.map_or("", StageId::as_str),
            authored.si_bits,
            gauge.as_str()
        );
        Ok(ResultRecordRef {
            record_digest: digest_bytes(key.as_bytes()),
            basis: EvaluationBasis::DirectReading,
            uncertainty_class: uec,
            model_ref: None,
            outcome: rec.outcome().wire().to_string(),
            rule: Some(rule_ref.to_string()),
        })
    }
}

#[test]
fn ac1_to_ac5_five_part_series_errors_principal_replay_and_perf_500() {
    let cat = Catalogue::geo_frame().unwrap();
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
    let r1 = RunId::parse("R1").unwrap();
    let c1 = CharId::parse("C1").unwrap();
    let g02 = GaugeId::new("G-02").unwrap();
    let op = PrincipalId::new("operator:bob").unwrap();
    let ts = EntryTimestamp::parse("2026-05-01T15:00:00Z").unwrap();

    let stages = FivePartStages {
        run: r1.clone(),
        stages: (1..=5).map(|i| StageId::parse(&format!("S{i}")).unwrap()).collect(),
    };
    let mut store = ManualReadingStore::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_000);

    let vals = vec![
        (StageId::parse("S1").unwrap(), "12.004mm".into()),
        (StageId::parse("S2").unwrap(), "12.006mm".into()),
        (StageId::parse("S3").unwrap(), "12.011mm".into()),
        (StageId::parse("S4").unwrap(), "12.002mm".into()),
        (StageId::parse("S5").unwrap(), "12.0195mm".into()), // exceeds 0.001 mm resolution -> per-index warning at index 4
    ];

    // AC-1: Five readings committed in ONE journal event in exact stage order S1..S5.
    let res = enter_series_reading(
        &mut store,
        Principal::Human,
        &cat,
        &reg,
        &stages,
        &SeriesEvaluator,
        &mut ids,
        &EnterSeriesParams {
            characteristic: c1.clone(),
            run: r1.clone(),
            gauge: Some(g02.clone()),
            values: vals,
            operator: op.clone(),
            entered_at: ts.clone(),
            dry_run: false,
        },
    )
    .expect("AC-1 five-part series");
    assert!(res.committed);
    assert_eq!(res.readings.len(), 5);
    assert_eq!(store.journal().len(), 1, "whole series commits as ONE journal event");
    let stage_names: Vec<&str> = res.readings.iter().map(|r| r.stage.as_ref().unwrap().as_str()).collect();
    assert_eq!(stage_names, vec!["S1", "S2", "S3", "S4", "S5"]);
    assert_eq!(res.readings[4].literal.raw_literal.untrusted_value().as_str(), "12.0195mm");
    assert_eq!(res.warnings.len(), 1);
    assert_eq!(res.warnings[0].0, 4);
    assert_eq!(res.warnings[0].1.code, WARN_READING_EXCEEDS_RESOLUTION);

    // AC-2: S3="12,011mm" (index 2) and S5="" (index 4) -> report.series_invalid listing indices 2 and 4; journal unchanged.
    let jlen = store.journal().len();
    let bad_vals = vec![
        (StageId::parse("S1").unwrap(), "12.004mm".into()),
        (StageId::parse("S2").unwrap(), "12.006mm".into()),
        (StageId::parse("S3").unwrap(), "12,011mm".into()),
        (StageId::parse("S4").unwrap(), "12.002mm".into()),
        (StageId::parse("S5").unwrap(), "".into()),
    ];
    let err2 = enter_series_reading(
        &mut store,
        Principal::Human,
        &cat,
        &reg,
        &stages,
        &SeriesEvaluator,
        &mut ids,
        &EnterSeriesParams {
            characteristic: c1.clone(),
            run: r1.clone(),
            gauge: Some(g02.clone()),
            values: bad_vals,
            operator: op.clone(),
            entered_at: ts.clone(),
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(err2.code(), "report.series_invalid");
    let SeriesError::SeriesInvalid { failures } = err2 else { panic!("expected SeriesInvalid") };
    assert_eq!(failures, vec![(2, "report.reading_invalid"), (4, "report.reading_invalid")]);
    assert_eq!(store.journal().len(), jlen);

    // AC-3a: Duplicate stage S2 -> report.series_invalid; stage outside run S99 -> report.series_invalid.
    let dup_vals = vec![
        (StageId::parse("S2").unwrap(), "12.004mm".into()),
        (StageId::parse("S2").unwrap(), "12.006mm".into()),
        (StageId::parse("S99").unwrap(), "12.008mm".into()),
    ];
    let err3a = enter_series_reading(
        &mut store,
        Principal::Human,
        &cat,
        &reg,
        &stages,
        &SeriesEvaluator,
        &mut ids,
        &EnterSeriesParams {
            characteristic: c1.clone(),
            run: r1.clone(),
            gauge: Some(g02.clone()),
            values: dup_vals,
            operator: op.clone(),
            entered_at: ts.clone(),
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(err3a.code(), "report.series_invalid");
    let SeriesError::SeriesInvalid { failures: f3 } = err3a else { panic!("expected SeriesInvalid") };
    assert_eq!(f3, vec![(1, "report.duplicate_stage"), (2, "report.stage_not_in_run")]);

    // AC-3b: 501 elements -> report.series_too_long.
    let too_many: Vec<(StageId, String)> =
        (1..=501).map(|i| (StageId::parse(&format!("P{i}")).unwrap(), "12.001mm".into())).collect();
    let err3b = enter_series_reading(
        &mut store,
        Principal::Human,
        &cat,
        &reg,
        &stages,
        &SeriesEvaluator,
        &mut ids,
        &EnterSeriesParams {
            characteristic: c1.clone(),
            run: r1.clone(),
            gauge: Some(g02.clone()),
            values: too_many,
            operator: op.clone(),
            entered_at: ts.clone(),
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(err3b.code(), "report.series_too_long");

    // AC-4: Agent and Script principals refused with core.human_only; journal unchanged.
    for p in [Principal::Agent, Principal::Script] {
        let ep = enter_series_reading(
            &mut store,
            p,
            &cat,
            &reg,
            &stages,
            &SeriesEvaluator,
            &mut ids,
            &EnterSeriesParams {
                characteristic: c1.clone(),
                run: r1.clone(),
                gauge: Some(g02.clone()),
                values: vec![(StageId::parse("S1").unwrap(), "12.004mm".into())],
                operator: op.clone(),
                entered_at: ts.clone(),
                dry_run: false,
            },
        )
        .unwrap_err();
        assert_eq!(ep, SeriesError::HumanOnly);
    }
    assert_eq!(store.journal().len(), jlen);

    // AC-5: Exact replay of series journal -> IDENTICAL!
    let roundtripped: Vec<ManualReadingEvent> =
        store.journal().iter().map(|ev| ManualReadingEvent::from_value(&ev.to_value()).unwrap()).collect();
    let run = replay_exact(&roundtripped);
    assert_eq!(run.outcome, ReplayOutcome::Identical, "mismatch at {:?}", run.first_mismatch);
    assert_eq!(run.store.list_reading(Some(&r1), None), store.list_reading(Some(&r1), None));

    // CommandDef check: human_only: true, W2, D0.
    let def = enter_series_command_def().expect("valid CommandDef");
    assert_eq!(def.id, "report.enter_series_reading");
    assert!(def.human_only);
    assert_eq!(def.side_effect, fid_types::SideEffect::W2);

    // Perf budget: 500-element series well under 100 ms.
    let stages_500 = FivePartStages {
        run: r1.clone(),
        stages: (1..=500).map(|i| StageId::parse(&format!("P{i}")).unwrap()).collect(),
    };
    let vals_500: Vec<(StageId, String)> =
        (1..=500).map(|i| (StageId::parse(&format!("P{i}")).unwrap(), "12.005mm".into())).collect();
    let t0 = Instant::now();
    let res500 = enter_series_reading(
        &mut store,
        Principal::Human,
        &cat,
        &reg,
        &stages_500,
        &SeriesEvaluator,
        &mut ids,
        &EnterSeriesParams {
            characteristic: c1,
            run: r1,
            gauge: Some(g02),
            values: vals_500,
            operator: op,
            entered_at: ts,
            dry_run: false,
        },
    )
    .expect("500-element series");
    assert_eq!(res500.readings.len(), 500);
    assert!(t0.elapsed().as_millis() < 2_000, "500-element series took {} ms", t0.elapsed().as_millis());
}
