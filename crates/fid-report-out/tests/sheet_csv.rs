//! Integration tests for `RPT.OUTPUT.SHEET.CSV` (AC-1 through AC-5, dry-run, rollback,
//! registry/bus schema validation, and 100k-cell performance).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::validate::Compiled;
use fid_io_reg::roots::{RootAccess, RootId};
use fid_io_reg::Principal;
use fid_project::Uuid;
use fid_registry::{validate as validate_registry, SideEffect};
use fid_report::bind::ResolvedReport;
use fid_report_out::{
    csv_command_defs, invoke_export_csv_cli, render_table_csv, Cell, ExportCsvEnv, ExportMeta, ExportSink, HeaderCell,
    LocalRootResolver, MemoryCsvJournal, MemoryResolvedLookup, SheetTable, CSV_RENDERER_ID, EXPORT_CSV_CMD,
};
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
        let path = std::env::temp_dir().join(format!("fid-report-out-csv-{}-{tag}", std::process::id()));
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
    let decls = fid_report_out::sheet::csv::declarations();
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(validate_registry(&raw), Vec::new());

    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    let defs = csv_command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(format!("{}@{}", defs[0].id, defs[0].major), EXPORT_CSV_CMD);
    assert_eq!(defs[0].side_effect, SideEffect::W1);
    assert_eq!(fid_registry::tool_name(&defs[0].id, defs[0].major), "report_export_csv");
    assert_eq!(fid_registry::check_schema(&defs[0].params_schema, &catalogue), Vec::new());
    assert_eq!(fid_registry::check_schema(&defs[0].result_schema, &catalogue), Vec::new());
    Compiled::compile(&defs[0].params_schema).unwrap();
    Compiled::compile(&defs[0].result_schema).unwrap();
}

#[test]
fn ac1_ac2_ac5_bracket_3stage_csv_export_golden_determinism_dry_run_and_crosscheck() {
    let raw = fs::read(root_dir().join("fixtures/rpt/model/golden/bracket_3stage.resolved.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let dig = resolved.digest.as_str().to_string();

    let mut lookup = MemoryResolvedLookup::new();
    lookup.insert(resolved.clone());

    let tmp = TmpDir::new("ac12");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);
    let mut journal = MemoryCsvJournal::new();

    // Dry-run first: computes exact digests/sizes without writing or journaling.
    let (dry, dry_json) = {
        let mut env = ExportCsvEnv { principal: Principal::Agent, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_csv_cli(
            &["--resolved", &dig, "--root", "root://exports", "--base", "bracket", "--dry-run", "--json"],
            &mut env,
        )
        .unwrap()
    };
    assert!(dry_json);
    assert!(dry.dry_run);
    assert_eq!(dry.files.len(), 3);
    assert!(dry.receipts.is_empty());
    assert_eq!(journal.events().len(), 0);
    assert_eq!(fs::read_dir(&tmp.path).unwrap().count(), 0);

    // AC-1: Real export produces bracket-01.csv, bracket-02.csv, bracket-meta.json and journals 1 event with 3 receipts.
    let (res1, is_json) = {
        let mut env = ExportCsvEnv { principal: Principal::Agent, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_csv_cli(
            &["--resolved", &dig, "--root", "root://exports", "--base", "bracket", "--json"],
            &mut env,
        )
        .unwrap()
    };
    assert!(is_json);
    assert!(!res1.dry_run);
    assert_eq!(res1.receipts.len(), 3);
    assert_eq!(res1.files, dry.files);
    assert_eq!(journal.events().len(), 1);
    assert_eq!(journal.events()[0].receipts.len(), 3);

    let b01 = fs::read(tmp.path.join("bracket-01.csv")).unwrap();
    let b02 = fs::read(tmp.path.join("bracket-02.csv")).unwrap();
    let bmeta = fs::read(tmp.path.join("bracket-meta.json")).unwrap();

    // UTF-8 without BOM and CRLF line ends (REQ-002).
    assert!(!b01.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(!b02.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(b01.ends_with(b"\r\n"));
    assert!(b02.ends_with(b"\r\n"));

    // Verify metadata has renderer "csv" and report_digest == resolved.digest.
    let parsed_meta = ExportMeta::from_canonical_bytes(&bmeta).unwrap();
    assert_eq!(parsed_meta.renderer.id, CSV_RENDERER_ID);
    assert_eq!(parsed_meta.report_digest, resolved.digest);

    // Golden byte-for-byte comparison (AC-1).
    let golden_dir = root_dir().join("fixtures/rpt/output/sheet");
    for (fname, actual) in [("bracket-01.csv", &b01), ("bracket-02.csv", &b02), ("bracket-meta.json", &bmeta)] {
        let path = golden_dir.join(fname);
        if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
            fs::write(&path, actual).unwrap();
        }
        let expected = fs::read(&path).unwrap_or_else(|e| panic!("missing golden {}: {e}", path.display()));
        assert_eq!(actual, &expected, "golden drift on {fname}");
    }

    // AC-2: Run again with --overwrite -> byte-identical files.
    let (res2, _) = {
        let mut env = ExportCsvEnv { principal: Principal::Agent, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_csv_cli(
            &["--resolved", &dig, "--root", "root://exports", "--base", "bracket", "--overwrite", "--json"],
            &mut env,
        )
        .unwrap()
    };
    assert_eq!(res2.receipts, res1.receipts);
    assert_eq!(fs::read(tmp.path.join("bracket-01.csv")).unwrap(), b01);
    assert_eq!(fs::read(tmp.path.join("bracket-02.csv")).unwrap(), b02);
    assert_eq!(fs::read(tmp.path.join("bracket-meta.json")).unwrap(), bmeta);
    assert_eq!(journal.events().len(), 2);

    // AC-5 cross-check: every numeric CSV field equals the canonical ValueText string in resolved report.
    let csv_text = String::from_utf8(b02).unwrap();
    let lines: Vec<&str> = csv_text.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 51, "1 header + 50 characteristic rows");
    assert!(lines[0].contains("Nominal [mm],Stage 1 Measured [mm],Stage 1 Deviation [mm]"));
    let row0: Vec<&str> = lines[1].split(',').collect();
    assert_eq!(row0[1], "C01");
    assert_eq!(row0[2], "11.000");
    assert_eq!(row0[3], "11.001");
    assert_eq!(row0[4], "0.001");
    assert_eq!(row0[5], "0.003");
    assert_eq!(row0[7], "11.002");
    assert_eq!(row0[11], "11.003");
}

#[test]
fn ac3_injection_csv_quotes_and_never_starts_field_with_unprefixed_equals() {
    let raw = fs::read(root_dir().join("fixtures/rpt/output/sheet/injection.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let dig = resolved.digest.as_str().to_string();

    let mut lookup = MemoryResolvedLookup::new();
    lookup.insert(resolved);

    let tmp = TmpDir::new("ac3");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);
    let mut journal = MemoryCsvJournal::new();

    {
        let mut env = ExportCsvEnv { principal: Principal::Agent, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_csv_cli(&["--resolved", &dig, "--root", "root://exports", "--base", "inj", "--json"], &mut env)
            .unwrap();
    }

    let csv_text = fs::read_to_string(tmp.path.join("inj-01.csv")).unwrap();
    // Quoted RFC 4180 field with doubled quotes AND leading apostrophe.
    assert!(
        csv_text.contains("\"'=HYPERLINK(\"\"https://example.invalid\"\",\"\"click\"\")\""),
        "expected RFC 4180 quoted neutralised HYPERLINK cell in:\n{csv_text}"
    );
    // No CSV field after header starts with unprefixed `=`.
    for line in csv_text.split("\r\n").skip(1).filter(|l| !l.is_empty()) {
        assert!(!line.contains(",="), "found unprefixed = field in line: {line}");
        assert!(!line.contains(",\"="), "found unprefixed quoted = field in line: {line}");
    }
}

#[test]
fn ac4_partial_failure_rolls_back_earlier_files_and_journals_nothing() {
    let raw = fs::read(root_dir().join("fixtures/rpt/model/golden/bracket_3stage.resolved.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let dig = resolved.digest.as_str().to_string();

    let mut lookup = MemoryResolvedLookup::new();
    lookup.insert(resolved);

    let tmp = TmpDir::new("ac4");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);
    let mut journal = MemoryCsvJournal::new();

    // Pre-create bracket-02.csv so writing bracket-01.csv succeeds, then bracket-02.csv fails report.export_exists!
    fs::write(tmp.path.join("bracket-02.csv"), b"existing-sentinel").unwrap();

    let err = {
        let mut env = ExportCsvEnv { principal: Principal::Agent, lookup: &lookup, sink: &sink, journal: &mut journal };
        invoke_export_csv_cli(&["--resolved", &dig, "--root", "root://exports", "--base", "bracket"], &mut env)
            .unwrap_err()
    };
    assert_eq!(err.code(), "report.export_exists");
    // bracket-01.csv created during this invocation MUST be removed (rollback!).
    assert!(!tmp.path.join("bracket-01.csv").exists(), "bracket-01.csv must be rolled back");
    assert!(!tmp.path.join("bracket-meta.json").exists());
    // Pre-existing bracket-02.csv is completely untouched, and nothing was journaled!
    assert_eq!(fs::read(tmp.path.join("bracket-02.csv")).unwrap(), b"existing-sentinel");
    assert_eq!(journal.events().len(), 0);
}

#[test]
fn perf_100k_cells_csv_render_fast() {
    let header: Vec<HeaderCell> = (0..10).map(|i| HeaderCell::with_unit(&format!("C{i}"), "mm")).collect();
    let row = vec![Cell::Number { canonical: "12.345".to_string(), decimals: 3 }; 10];
    let table = SheetTable {
        block_id: Uuid::parse("019a9f10-0001-7000-8000-000000000601").unwrap(),
        name: "perf".to_string(),
        header,
        rows: vec![row; 10_000],
    };
    let start = Instant::now();
    let bytes = render_table_csv(&table);
    let ms = start.elapsed().as_millis();
    assert!(!bytes.is_empty());
    eprintln!("RPT.OUTPUT.SHEET.CSV rendered 100,000 cells in {ms} ms (debug)");
    assert!(ms < 5_000);
}
