#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]
//! Acceptance tests for `RPT.MODEL.BLOCKS.TABLE` (AC-1..AC-5) in `fid-report`.

use fid_canon::{parse, CValue};
use fid_gdt::chars::record::EvaluationBasis;
use fid_kdec::{Catalog as RuleCatalog, Decision, EvidenceBasis, Outcome as DecOutcome, RuleRef};
use fid_kunc::UncertaintyStatement;
use fid_num::fmt::{FormatterId, Unit};
use fid_num::kdec::Decimal;
use fid_project::{Bits, IdError, ObservationSource, Uuid};
use fid_registry::{validate, SideEffect};
use fid_replay::ReplayOutcome;
use fid_report::bind::{
    bind_block, resolve_block, BindBlockParams, BindCtx, Caller, EntityKind, JournalHead, MemoryJournal, Principal,
    RefKind, RefParam, ResolveContext, ResolverRegistry,
};
use fid_report::blocks::table::{
    command_defs, declarations, execute_set_table_block, invoke_cli, resolve_table_cells, set_table_block, CellText,
    ColumnSource, ColumnSpec, ColumnSpecParam, RowFilter, RowFilterParam, SetTableBlockParams, TableBlockKind,
    TableBlockResolver, TableCmdCtx, TableError, TablePayload, TableRowInput, MISSING_TEXT, TABLE_KIND_ID,
};
use fid_report::doc::{
    add_block, create_report, replay_exact, AddBlock, BlockKindId, CreateReport, KindRegistry, MemoryBackend,
    ReportBackend, ReportId, TextFault, Untrusted, SET_TABLE_BLOCK,
};
use fid_report::fmt::{
    Catalog, Characteristic, DefaultDecimals, FmtContext, InterpretationStatus, LimitSet, LimitUnit, LocaleRules,
    SiValue, UncertaintyText, UnitOverride, ValueKind, ValueRecord,
};
use fid_types::Digest;
use std::path::{Path, PathBuf};

const BASE_MS: u64 = 1_763_510_400_000;

struct Counter {
    next: u64,
}

impl ObservationSource for Counter {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(BASE_MS + self.next)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let b = Bits::new(n, u128::from(self.next) + 1)?;
        self.next += 1;
        Ok(b)
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap()
}

fn load_stmt(name: &str) -> UncertaintyStatement {
    UncertaintyStatement::parse_canonical(&read(&format!("fixtures/unc/stmt/valid/{name}"))).unwrap()
}

fn signed_off_record_bytes() -> Vec<u8> {
    let bytes = read("fixtures/gdt/interp/cases.json");
    let cases = parse(&bytes).unwrap();
    let CValue::Obj(top) = cases else { panic!("cases") };
    let CValue::Obj(all) = top.get("cases").unwrap() else { panic!("all") };
    let CValue::Obj(case) = all.get("flatness_signed").unwrap() else { panic!("case") };
    let CValue::Obj(mut expect) = case.get("expect").unwrap().clone() else { panic!("expect") };
    expect.remove("verification");
    expect.insert("schema".to_string(), CValue::Str("fid.gdt-result/2".to_string()));
    expect.insert("interpretation".to_string(), CValue::Str("signed_off".to_string()));
    if let Some(CValue::Arr(rules)) = expect.get_mut("interpretation_rules") {
        for r in rules.iter_mut() {
            if let CValue::Obj(m) = r {
                m.insert("status".to_string(), CValue::Str("signed_off".to_string()));
                m.insert("reason".to_string(), CValue::Null);
            }
        }
    }
    fid_canon::to_canonical_bytes(&CValue::Obj(expect)).unwrap()
}

fn make_decision(rule: Option<&str>, outcome: DecOutcome, st: &UncertaintyStatement) -> Decision {
    let cat = RuleCatalog::builtin().unwrap();
    let rule = rule.map(|r| cat.resolve(&RuleRef::parse(r).unwrap()).unwrap());
    Decision::new(rule.as_ref(), outcome, Some(EvidenceBasis::of(st).unwrap())).unwrap()
}

fn en_locale() -> LocaleRules {
    LocaleRules::new("en", "test-1", '.', Some(','), 3, 3, "-").unwrap()
}

fn make_kinds() -> KindRegistry {
    let mut kinds = KindRegistry::new();
    kinds.register(Box::new(TableBlockKind)).unwrap();
    kinds
}

fn setup_report_with_table_block(project: Uuid) -> (MemoryBackend, KindRegistry, ReportId, Uuid) {
    let kinds = make_kinds();
    let mut backend = MemoryBackend::new(project);
    let mut ids = Counter { next: 0 };
    let created = create_report(
        &mut backend,
        &kinds,
        &mut ids,
        &CreateReport { title: Untrusted::text("Bracket 3-stage FAI").unwrap(), project },
    )
    .unwrap();
    let report_id = created.id();
    let page_uuid = created.pages()[0].id().uuid();
    let initial = TablePayload::new(vec![ColumnSpec::new(ColumnSource::Id)], RowFilter::default(), None).unwrap();
    let added = add_block(
        &mut backend,
        &kinds,
        &mut ids,
        &AddBlock {
            report: report_id.uuid(),
            page: page_uuid,
            after: None,
            kind: BlockKindId::new(TABLE_KIND_ID),
            payload: initial.to_value(),
        },
    )
    .unwrap();
    (backend, kinds, report_id, added.uuid())
}

fn bracket_project_id() -> Uuid {
    let bytes = read("fixtures/rpt/model/bracket_3stage/cases.json");
    let doc = parse(&bytes).unwrap();
    let CValue::Obj(o) = doc else { panic!("cases obj") };
    let CValue::Str(s) = o.get("project").unwrap() else { panic!("project") };
    Uuid::parse(s).unwrap()
}

fn load_bracket_journal() -> MemoryJournal {
    let doc = parse(&read("fixtures/rpt/model/bracket_3stage/journal.json")).unwrap();
    let CValue::Obj(jo) = doc else { panic!("journal") };
    let CValue::Arr(entries) = jo.get("entries").unwrap() else { panic!("entries") };
    let mut j = MemoryJournal::new();
    for entry in entries {
        let CValue::Obj(eo) = entry else { panic!("entry") };
        let st = |k: &str| match eo.get(k).unwrap() {
            CValue::Str(s) => s.as_str(),
            _ => panic!("{k} not string"),
        };
        let kind = match st("kind") {
            "result" => EntityKind::Result,
            "characteristic" => EntityKind::Characteristic,
            "named_view" => EntityKind::NamedView,
            "series" => EntityKind::Series,
            "statement" => EntityKind::Statement,
            other => panic!("kind {other}"),
        };
        let id = match eo.get("id").unwrap() {
            CValue::Str(s) => Some(Uuid::parse(s).unwrap()),
            _ => None,
        };
        let head = JournalHead::parse(st("head")).unwrap();
        let proj = Uuid::parse(st("project")).unwrap();
        j.append(head, proj, kind, id, st("text").as_bytes().to_vec()).unwrap();
    }
    j
}

#[test]
fn registry_definition_is_valid() {
    let decls = declarations();
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(validate(&raw), Vec::new());
    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].id.as_str(), "report.set_table_block");
    assert_eq!(defs[0].major, 1);
    assert_eq!(defs[0].side_effect, SideEffect::W2);
    assert_eq!(fid_registry::tool_name(&defs[0].id, defs[0].major), "report_set_table_block");
    for schema in [&defs[0].params_schema, &defs[0].result_schema] {
        assert_eq!(fid_registry::check_schema(schema, &catalogue), Vec::new());
    }
}

#[test]
fn ac1_set_table_block_cli_stores_columns_rename_and_journals_one_event() {
    let project = bracket_project_id();
    let (mut backend, kinds, report_id, block_id) = setup_report_with_table_block(project);
    let events_before = backend.journal().len();

    let report_s = report_id.uuid().to_string();
    let block_s = block_id.to_string();
    let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
    let res = invoke_cli(
        &mut ctx,
        &[
            "set-table-block",
            "--report",
            &report_s,
            "--block",
            &block_s,
            "--columns",
            "id,nominal,measured,deviation,decision",
            "--rename",
            "measured=Actual",
            "--json",
        ],
    )
    .unwrap();

    let CValue::Obj(ro) = res else { panic!("result object") };
    assert_eq!(ro.get("column_count"), Some(&CValue::U64(5)));

    // Exactly one event journaled.
    assert_eq!(backend.journal().len(), events_before + 1);
    let last_event = backend.journal().last().unwrap();
    assert_eq!(last_event.event_type(), SET_TABLE_BLOCK);

    // Reload report and verify payload order and renamed header.
    let stored = backend.load(&report_id).unwrap().unwrap();
    let report = stored.open(&report_id, &kinds).unwrap();
    let block = report.block(&block_id).unwrap();
    let parsed = TablePayload::from_value(block.payload().value()).unwrap();
    let sources: Vec<ColumnSource> = parsed.columns.iter().map(|c| c.source).collect();
    assert_eq!(
        sources,
        vec![
            ColumnSource::Id,
            ColumnSource::Nominal,
            ColumnSource::Measured,
            ColumnSource::Deviation,
            ColumnSource::Decision,
        ]
    );
    let headers: Vec<&str> = parsed.columns.iter().map(ColumnSpec::effective_header).collect();
    assert_eq!(headers, vec!["ID", "Nominal", "Actual", "Deviation", "Decision"]);

    // AC-5: exact replay of the whole journal is IDENTICAL.
    let replay = replay_exact(&backend.journal_values(), &kinds, project);
    assert_eq!(replay.outcome, ReplayOutcome::Identical);
    assert_eq!(replay.first_mismatch, None);
    assert_eq!(replay.reports.get(&report_id).map(|(_, d)| *d), Some(stored.digest));
}

#[test]
fn ac2_and_ac4_row_filters_formatter_ids_uec0_and_missing_values() {
    let project = bracket_project_id();
    let (mut backend, kinds, report_id, block_id) = setup_report_with_table_block(project);
    let report_s = report_id.uuid().to_string();
    let block_s = block_id.to_string();

    let st_uec1 = load_stmt("uec1_lpu_unverified.json");
    let st_uec0 = load_stmt("uec0_instrument_missing.json");
    let rec_bytes = signed_off_record_bytes();
    let rec_digest = Digest::from_bytes(fid_canon::digest_bytes(&rec_bytes).0);
    let interp = InterpretationStatus::test_of_record_bytes(&rec_bytes);

    let d_conf = make_decision(Some("dr:simple-acceptance@1"), DecOutcome::Conforms, &st_uec1);
    let d_nonconf = make_decision(Some("dr:simple-acceptance@1"), DecOutcome::Nonconforms, &st_uec1);
    let d_uec0 = make_decision(
        Some("dr:as-reported-by-source@1"),
        DecOutcome::NotDecidable(fid_kdec::NotDecidableReason::LabelOnly),
        &st_uec0,
    );

    let lims = LimitSet {
        lower: Some(Decimal::parse("0.0099").unwrap()),
        upper: Some(Decimal::parse("0.0101").unwrap()),
        unit: LimitUnit::Si,
    };
    let make_char = |id: &str| Characteristic {
        id: id.to_string(),
        kind: Some(ValueKind::Length),
        unit: Unit::Mm,
        decimals: Some(3),
        limits: Some(lims),
        nominal: Some(SiValue::new(0.010)),
    };

    let ch1 = make_char("C01");
    let ch2 = make_char("C02");
    let ch3 = make_char("C03");
    let ch4 = make_char("C04");
    let ch5 = Characteristic {
        id: "C05".to_string(),
        kind: Some(ValueKind::Length),
        unit: Unit::Mm,
        decimals: Some(3),
        limits: None,
        nominal: None,
    };

    let r1 = ValueRecord {
        id: "r1".to_string(),
        value: SiValue::new(0.01001),
        basis: EvaluationBasis::Points,
        decision: &d_conf,
        statement: &st_uec1,
        acceptance: None,
        transcription: None,
        gauge: None,
        record_digest: rec_digest,
        interpretation: interp,
    };
    let r2 = ValueRecord {
        id: "r2".to_string(),
        value: SiValue::new(0.01025),
        basis: EvaluationBasis::Points,
        decision: &d_nonconf,
        statement: &st_uec1,
        acceptance: None,
        transcription: None,
        gauge: None,
        record_digest: rec_digest,
        interpretation: interp,
    };
    let r3 = ValueRecord {
        id: "r3".to_string(),
        value: SiValue::new(0.00980),
        basis: EvaluationBasis::Points,
        decision: &d_nonconf,
        statement: &st_uec1,
        acceptance: None,
        transcription: None,
        gauge: None,
        record_digest: rec_digest,
        interpretation: interp,
    };
    let r4 = ValueRecord {
        id: "r4".to_string(),
        value: SiValue::new(0.01002),
        basis: EvaluationBasis::Points,
        decision: &d_uec0,
        statement: &st_uec0,
        acceptance: None,
        transcription: None,
        gauge: None,
        record_digest: rec_digest,
        interpretation: interp,
    };

    let t_crit = vec!["critical".to_string()];
    let t_major = vec!["major".to_string()];
    let t_empty: Vec<String> = Vec::new();
    let rows = vec![
        TableRowInput {
            characteristic_id: "C01",
            characteristic_name: "Bore 1",
            characteristic_type: "Diameter",
            characteristic: &ch1,
            tags: &t_crit,
            stage: Some("Stage 1"),
            run: Some("Run 1"),
            result: Some(&r1),
        },
        TableRowInput {
            characteristic_id: "C02",
            characteristic_name: "Bore 2",
            characteristic_type: "Diameter",
            characteristic: &ch2,
            tags: &t_crit,
            stage: Some("Stage 1"),
            run: Some("Run 1"),
            result: Some(&r2),
        },
        TableRowInput {
            characteristic_id: "C03",
            characteristic_name: "Bore 3",
            characteristic_type: "Diameter",
            characteristic: &ch3,
            tags: &t_major,
            stage: Some("Stage 2"),
            run: Some("Run 1"),
            result: Some(&r3),
        },
        TableRowInput {
            characteristic_id: "C04",
            characteristic_name: "Bore 4 (UEC-0)",
            characteristic_type: "Diameter",
            characteristic: &ch4,
            tags: &t_major,
            stage: Some("Stage 3"),
            run: Some("Run 1"),
            result: Some(&r4),
        },
        TableRowInput {
            characteristic_id: "C05",
            characteristic_name: "Unmeasured",
            characteristic_type: "Diameter",
            characteristic: &ch5,
            tags: &t_empty,
            stage: None,
            run: None,
            result: None,
        },
    ];

    let catalog = Catalog::english();
    let fmt_ctx = FmtContext {
        locale: en_locale(),
        units: UnitOverride::NONE,
        default_decimals: DefaultDecimals::initial(),
        catalog: &catalog,
    };

    // Unfiltered table with all key columns including uncertainty and limits (AC-4 & REQ-005).
    {
        let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        invoke_cli(
            &mut ctx,
            &[
                "set-table-block",
                "--report",
                &report_s,
                "--block",
                &block_s,
                "--columns",
                "id,nominal,lower_limit,upper_limit,measured,deviation,decision,uncertainty",
                "--rename",
                "measured=Actual",
                "--json",
            ],
        )
        .unwrap();
    }
    let stored = backend.load(&report_id).unwrap().unwrap();
    let rep = stored.open(&report_id, &kinds).unwrap();
    let payload = TablePayload::from_value(rep.block(&block_id).unwrap().payload().value()).unwrap();
    let cells = resolve_table_cells(&payload, &rows, &fmt_ctx).unwrap();
    assert_eq!(cells.rows.len(), 5);
    assert_eq!(cells.header[4], "Actual");

    // Every numeric cell in row 0 records FormatterId::LENGTH_V1 (AC-4).
    for col_idx in [1usize, 2, 3, 4, 5] {
        let vt = cells.rows[0][col_idx].value_text().expect("numeric ValueText");
        assert_eq!(vt.formatter(), FormatterId::LENGTH_V1);
    }

    // UEC-0 row (C04, index 3) shows literal "not evaluated" in uncertainty column (AC-4).
    assert_eq!(cells.rows[3][7].display(), "not evaluated");
    assert!(matches!(cells.rows[3][7], CellText::Uncertainty(UncertaintyText::NotEvaluated(_))));

    // Missing row (C05, index 4) shows literal "no result" in missing columns (REQ-005).
    for col_idx in [1usize, 2, 3, 4, 5, 6, 7] {
        assert_eq!(cells.rows[4][col_idx], CellText::Missing);
        assert_eq!(cells.rows[4][col_idx].display(), MISSING_TEXT);
    }

    // AC-2 part 1: filter `--outcome NONCONFORMS` -> only C02 and C03.
    {
        let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        invoke_cli(
            &mut ctx,
            &[
                "set-table-block",
                "--report",
                &report_s,
                "--block",
                &block_s,
                "--columns",
                "id,nominal,measured,deviation,decision",
                "--rename",
                "measured=Actual",
                "--outcome",
                "NONCONFORMS",
                "--json",
            ],
        )
        .unwrap();
    }
    let payload_nc = TablePayload::from_value(
        backend
            .load(&report_id)
            .unwrap()
            .unwrap()
            .open(&report_id, &kinds)
            .unwrap()
            .block(&block_id)
            .unwrap()
            .payload()
            .value(),
    )
    .unwrap();
    let cells_nc = resolve_table_cells(&payload_nc, &rows, &fmt_ctx).unwrap();
    let ids_nc: Vec<&str> = cells_nc.rows.iter().map(|r| r[0].display()).collect();
    assert_eq!(ids_nc, vec!["C02", "C03"]);

    // AC-2 part 2: `--outcome NONCONFORMS --tag critical` -> only C02.
    {
        let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        invoke_cli(
            &mut ctx,
            &[
                "set-table-block",
                "--report",
                &report_s,
                "--block",
                &block_s,
                "--columns",
                "id,nominal,measured,deviation,decision",
                "--rename",
                "measured=Actual",
                "--outcome",
                "NONCONFORMS",
                "--tag",
                "critical",
                "--json",
            ],
        )
        .unwrap();
    }
    let payload_both = TablePayload::from_value(
        backend
            .load(&report_id)
            .unwrap()
            .unwrap()
            .open(&report_id, &kinds)
            .unwrap()
            .block(&block_id)
            .unwrap()
            .payload()
            .value(),
    )
    .unwrap();
    let cells_both = resolve_table_cells(&payload_both, &rows, &fmt_ctx).unwrap();
    let ids_both: Vec<&str> = cells_both.rows.iter().map(|r| r[0].display()).collect();
    assert_eq!(ids_both, vec!["C02"]);
}

#[test]
fn ac3_unknown_column_and_65_columns_and_header_too_long_are_refused() {
    let project = bracket_project_id();
    let (mut backend, kinds, report_id, block_id) = setup_report_with_table_block(project);
    let events_before = backend.journal().len();
    let report_s = report_id.uuid().to_string();
    let block_s = block_id.to_string();

    // Unknown column source `colour` -> report.column_unknown.
    {
        let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        let err = invoke_cli(
            &mut ctx,
            &["set-table-block", "--report", &report_s, "--block", &block_s, "--columns", "id,colour", "--json"],
        )
        .unwrap_err();
        assert_eq!(err.code(), "report.column_unknown");
        assert!(matches!(err, TableError::ColumnUnknown(ref c) if c == "colour"));
    }

    // 65 columns -> report.invalid_block_payload.
    {
        let cols: Vec<ColumnSpecParam> =
            (0..65).map(|_| ColumnSpecParam { source: "id".to_string(), header: None }).collect();
        let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        let err = set_table_block(
            &mut ctx,
            SetTableBlockParams {
                report: report_s.clone(),
                block: block_s.clone(),
                columns: cols,
                filter: RowFilterParam::default(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "report.invalid_block_payload");
    }

    // 0 columns -> report.invalid_block_payload.
    {
        let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        let err = set_table_block(
            &mut ctx,
            SetTableBlockParams {
                report: report_s.clone(),
                block: block_s.clone(),
                columns: Vec::new(),
                filter: RowFilterParam::default(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "report.invalid_block_payload");
    }

    // Header > 128 bytes -> report.text_invalid.
    {
        let long_header = "A".repeat(129);
        let mut ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        let err = set_table_block(
            &mut ctx,
            SetTableBlockParams {
                report: report_s.clone(),
                block: block_s.clone(),
                columns: vec![ColumnSpecParam { source: "id".to_string(), header: Some(long_header) }],
                filter: RowFilterParam::default(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "report.text_invalid");
        assert_eq!(err, TableError::TextInvalid(TextFault::TooLong));
    }

    // Failed calls journaled nothing.
    assert_eq!(backend.journal().len(), events_before);
}

#[test]
fn table_resolver_integrates_with_bind_block_and_json_executor() {
    let cases_doc = parse(&read("fixtures/rpt/model/bracket_3stage/cases.json")).unwrap();
    let CValue::Obj(co) = cases_doc else { panic!("cases") };
    let st = |k: &str| match co.get(k).unwrap() {
        CValue::Str(s) => s.clone(),
        _ => panic!("{k}"),
    };
    let project = Uuid::parse(&st("project")).unwrap();
    let r1 = st("r1");
    let head = JournalHead::parse(&st("current_head")).unwrap();
    let journal = load_bracket_journal();

    let (mut backend, kinds, report_id, block_id) = setup_report_with_table_block(project);
    let mut resolvers = ResolverRegistry::new();
    resolvers.register(Box::new(TableBlockResolver)).unwrap();

    // Update columns via execute_set_table_block (--json path).
    {
        let mut cmd_ctx = TableCmdCtx { backend: &mut backend, kinds: &kinds };
        let params = parse(
            format!(
                r#"{{"report":"{}","block":"{}","columns":[{{"source":"id"}},{{"source":"measured","header":"Actual"}}],"filter":{{"outcomes":[],"tags":[]}}}}"#,
                report_id.uuid(),
                block_id
            )
            .as_bytes(),
        )
        .unwrap();
        execute_set_table_block(&mut cmd_ctx, &params).unwrap();
    }

    // Bind r1 to the table block through RPT.MODEL.BIND.RESOLVE.
    {
        let mut bind_ctx = BindCtx {
            backend: &mut backend,
            kinds: &kinds,
            resolvers: &resolvers,
            journal: &journal,
            sources: &[],
            caller: Caller { principal: Principal::Human, surface: fid_registry::Surface::Cli },
            branch: "main",
        };
        bind_block(
            &mut bind_ctx,
            BindBlockParams {
                report: report_id.uuid().to_string(),
                block: block_id.to_string(),
                refs: vec![RefParam { kind: RefKind::Result, id: r1 }],
            },
        )
        .unwrap();
    }

    // Resolve the bound table block at the pinned head.
    let stored = backend.load(&report_id).unwrap().unwrap();
    let report = stored.open(&report_id, &kinds).unwrap();
    let block = report.block(&block_id).unwrap();
    let resolve_ctx = ResolveContext { project, head, stages: Vec::new(), runs: Vec::new(), sources: Vec::new() };
    let resolved = resolve_block(block, &resolve_ctx, &kinds, &resolvers, &journal).unwrap();
    assert_eq!(resolved.view().values().len(), 1);

    // Exact replay over create + add_block + set_table_block + bind_block is IDENTICAL.
    let replay = replay_exact(&backend.journal_values(), &kinds, project);
    assert_eq!(replay.outcome, ReplayOutcome::Identical);
}
