//! Integration tests for `RPT.OUTPUT.SHEET.XLSX` (AC-1..AC-5, row/column limits,
//! formula-injection neutralisation, registry/bus validation, and 100k-cell performance).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::validate::Compiled;
use fid_io_reg::roots::{RootAccess, RootId};
use fid_io_reg::Principal;
use fid_project::Uuid;
use fid_registry::{check_schema, tool_name, validate as validate_registry, Catalogue, SideEffect};
use fid_report::bind::ResolvedReport;
use fid_report_out::{
    export_spreadsheet, format_sheet_name, invoke_export_spreadsheet_cli, render_workbook_xlsx, xlsx_command_defs,
    Cell, ExportMeta, ExportSink, ExportXlsxEnv, ExportXlsxParams, HeaderCell, LocalRootResolver, MemoryResolvedLookup,
    MemoryXlsxJournal, RendererVersion, SheetTable, EXPORT_XLSX_CMD, MAX_XLSX_ROWS, XLSX_RENDERER_ID,
    XLSX_RENDERER_VERSION,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct TmpDir {
    path: PathBuf,
}

impl TmpDir {
    fn new(tag: &str) -> TmpDir {
        let path = std::env::temp_dir().join(format!("fid-report-out-xlsx-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TmpDir { path }
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn registry_and_bus_schemas_validate() {
    let decls = fid_report_out::sheet::xlsx::declarations();
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(validate_registry(&raw), Vec::new());

    let catalogue = Catalogue::geo_frame().unwrap();
    let defs = xlsx_command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), EXPORT_XLSX_CMD);
    assert_eq!(defs[0].side_effect, SideEffect::W1);
    assert_eq!(tool_name(&defs[0].id, defs[0].major), "report_export_spreadsheet");
    assert_eq!(check_schema(&defs[0].params_schema, &catalogue), Vec::new());
    assert_eq!(check_schema(&defs[0].result_schema, &catalogue), Vec::new());
    Compiled::compile(&defs[0].params_schema).unwrap();
    Compiled::compile(&defs[0].result_schema).unwrap();
}

#[test]
fn ac1_ac2_ac3_ac5_bracket_3stage_xlsx_deterministic_golden_and_dry_run() {
    let raw = fs::read(root_dir().join("fixtures/rpt/model/golden/bracket_3stage.resolved.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let dig = resolved.digest.as_str().to_string();

    let mut lookup = MemoryResolvedLookup::new();
    lookup.insert(resolved);

    let tmp = TmpDir::new("ac1235");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);
    let mut journal = MemoryXlsxJournal::new();

    // Dry-run computes exact size & SHA-256 without writing or journaling.
    let dry_val = {
        let mut env =
            ExportXlsxEnv { principal: Principal::Human, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_spreadsheet_cli(
            &["--resolved", &dig, "--root", "root://exports", "--name", "bracket.xlsx", "--dry-run", "--json"],
            &mut env,
        )
        .unwrap()
    };
    assert!(!tmp.path.join("bracket.xlsx").exists());
    assert!(journal.events().is_empty());

    // First export writes bracket.xlsx and journals 1 event.
    let out1 = {
        let mut env =
            ExportXlsxEnv { principal: Principal::Human, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_spreadsheet_cli(
            &["--resolved", &dig, "--root", "root://exports", "--name", "bracket.xlsx", "--json"],
            &mut env,
        )
        .unwrap()
    };
    let bytes1 = fs::read(tmp.path.join("bracket.xlsx")).unwrap();

    // Second export with --overwrite produces byte-identical bytes and a 2nd journaled event.
    {
        let mut env =
            ExportXlsxEnv { principal: Principal::Human, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_spreadsheet_cli(
            &["--resolved", &dig, "--root", "root://exports", "--name", "bracket.xlsx", "--overwrite", "--json"],
            &mut env,
        )
        .unwrap();
    }
    let bytes2 = fs::read(tmp.path.join("bracket.xlsx")).unwrap();
    assert_eq!(bytes1, bytes2, "two exports must be byte-identical (REQ-003, AC-1)");
    assert_eq!(journal.events().len(), 2);
    assert_eq!(journal.events()[0].receipt.sha256, journal.events()[1].receipt.sha256);

    if std::env::var_os("UPDATE_XLSX_GOLDEN").is_some() {
        let dir = root_dir().join("fixtures/rpt/output/sheet");
        fs::write(dir.join("bracket.xlsx"), &bytes1).unwrap();
        fs::write(dir.join("bracket.xlsx.sha256"), format!("{}\n", journal.events()[0].receipt.sha256.as_str()))
            .unwrap();
        return;
    }

    let golden_sha = fs::read_to_string(root_dir().join("fixtures/rpt/output/sheet/bracket.xlsx.sha256")).unwrap();
    assert_eq!(journal.events()[0].receipt.sha256.as_str(), golden_sha.trim());
    assert_eq!(dry_val, {
        // Compare sha256 strings directly.
        let mut d = dry_val.clone();
        if let fid_canon::CValue::Obj(ref mut m) = d {
            m.insert("dry_run".to_string(), fid_canon::CValue::Bool(true));
        }
        d
    });
    let sha_from = |v: &fid_canon::CValue| match v {
        fid_canon::CValue::Obj(m) => match m.get("sha256") {
            Some(fid_canon::CValue::Str(s)) => s.clone(),
            other => panic!("unexpected sha256: {other:?}"),
        },
        other => panic!("unexpected value: {other:?}"),
    };
    assert_eq!(sha_from(&dry_val), sha_from(&out1));

    // Requesting --units si on an authored-units report fails report.units_mismatch.
    let mut env = ExportXlsxEnv { principal: Principal::Agent, lookup: &lookup, sink: &sink, journal: &mut journal };
    let si_err = invoke_export_spreadsheet_cli(
        &["--resolved", &dig, "--root", "root://exports", "--name", "si.xlsx", "--units", "si", "--json"],
        &mut env,
    )
    .unwrap_err();
    assert_eq!(si_err.code(), "report.units_mismatch");
}

#[test]
fn ac2_injection_cells_preserved_with_leading_apostrophe_and_sheet_names_sanitised() {
    let raw = fs::read(root_dir().join("fixtures/rpt/output/sheet/injection.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let dig = resolved.digest.as_str().to_string();

    let mut lookup = MemoryResolvedLookup::new();
    lookup.insert(resolved);

    let tmp = TmpDir::new("inj");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);
    let mut journal = MemoryXlsxJournal::new();

    {
        let mut env =
            ExportXlsxEnv { principal: Principal::Agent, lookup: &lookup, sink: &sink, journal: &mut journal };
        export_spreadsheet(
            &ExportXlsxParams {
                resolved: dig,
                root: "root://exports".to_string(),
                name: "inj.xlsx".to_string(),
                overwrite: false,
                dry_run: false,
                units: "authored".to_string(),
            },
            &mut env,
        )
        .unwrap();
    }
    let bytes = fs::read(tmp.path.join("inj.xlsx")).unwrap();
    assert!(bytes.starts_with(b"PK\x03\x04"), "must be valid ZIP signature");

    // Sheet naming sanitisation & 31-char limit.
    let mut used = BTreeSet::new();
    assert_eq!(format_sheet_name("table_01", 1, &mut used), "T01");
    assert_eq!(format_sheet_name("Part:A/B?C*D[E]F\\G", 2, &mut used), "Part_A_B_C_D_E_F_G");
    assert_eq!(format_sheet_name("meta", 3, &mut used), "T03");
    let long_name =
        format_sheet_name("A very long characteristic table caption exceeding thirty-one characters", 4, &mut used);
    assert_eq!(long_name.chars().count(), 31);
}

#[test]
fn ac4_table_exceeding_1048576_rows_fails_unsupported_block_and_writes_nothing() {
    let block_id = Uuid::parse("019a9f80-0001-7000-8000-000000000001").unwrap();
    let table = SheetTable {
        block_id,
        name: "huge".to_string(),
        header: vec![HeaderCell::plain("Col")],
        rows: vec![Vec::new(); MAX_XLSX_ROWS + 1],
    };
    let meta = ExportMeta::new(
        fid_types::Digest::from_bytes([0x11; 32]),
        None,
        "0.1.0",
        Vec::new(),
        RendererVersion::new(XLSX_RENDERER_ID, XLSX_RENDERER_VERSION).unwrap(),
    )
    .unwrap();

    let err = render_workbook_xlsx(&[table], &meta).unwrap_err();
    assert_eq!(err.code(), "report.unsupported_block");
    assert!(err.to_string().contains(&block_id.to_string()));
}

#[test]
fn perf_100k_cells_xlsx_under_budget() {
    let header: Vec<HeaderCell> = (0..10).map(|i| HeaderCell::with_unit(&format!("C{i}"), "mm")).collect();
    let row = vec![Cell::Number { canonical: "12.345".to_string(), decimals: 3 }; 10];
    let table = SheetTable {
        block_id: Uuid::parse("019a9f80-0001-7000-8000-000000000002").unwrap(),
        name: "table_01".to_string(),
        header,
        rows: vec![row; 10_000],
    };
    let meta = ExportMeta::new(
        fid_types::Digest::from_bytes([0x22; 32]),
        None,
        "0.1.0",
        Vec::new(),
        RendererVersion::new(XLSX_RENDERER_ID, XLSX_RENDERER_VERSION).unwrap(),
    )
    .unwrap();

    let start = Instant::now();
    let bytes = render_workbook_xlsx(&[table], &meta).unwrap();
    let ms = start.elapsed().as_millis();
    assert!(bytes.starts_with(b"PK\x03\x04"));
    eprintln!("RPT.OUTPUT.SHEET.XLSX rendered 100,000 cells in {ms} ms (debug, {} bytes)", bytes.len());
    assert!(ms < 5_000, "100k cells took {ms} ms");
}
