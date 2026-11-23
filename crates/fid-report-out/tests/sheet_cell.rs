//! Integration tests for `RPT.OUTPUT.SHEET.CELL` (AC-1 through AC-5 + performance check).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_project::Uuid;
use fid_report::bind::{GeneratedBlock, JournalHead, Provenance, ResolvedPage, ResolvedReport, UnitOverrideMode};
use fid_report::doc::BlockKindId;
use fid_report_out::{convert_report, Cell};
use fid_types::Digest;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn root_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn synthetic_report(blocks: Vec<GeneratedBlock>, units: Option<UnitOverrideMode>) -> ResolvedReport {
    let page_id = Uuid::parse("019a9f10-0001-7000-8000-000000000011").unwrap();
    let prov = Provenance {
        head: JournalHead::new(Digest::from_bytes([0x11; 32])),
        template: None,
        kernel_version: "0.1.0".to_string(),
        formatters: Vec::new(),
        locale: "en".to_string(),
        locale_release: "i18n-locale@1".to_string(),
        units,
    };
    ResolvedReport::finalize(prov, vec![ResolvedPage { id: page_id, blocks }], Vec::new()).unwrap()
}

#[test]
fn ac1_bracket_3stage_golden_converts_with_exact_canonical_decimals_and_unit_headers() {
    let raw = fs::read(root_dir().join("fixtures/rpt/model/golden/bracket_3stage.resolved.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let tables = convert_report(&resolved, false).unwrap();

    // Two table blocks in `tmpl:multistage-table`: stage column group + 50-row 3-stage table.
    assert_eq!(tables.len(), 2);
    assert_eq!(tables[0].rows.len(), 3);
    let main = &tables[1];
    assert_eq!(main.rows.len(), 50);

    // Headers carry `[mm]` for numeric columns.
    assert_eq!(main.header[0].text.safe, "Characteristic ID");
    assert_eq!(main.header[1].text.safe, "Name");
    assert_eq!(main.header[2].text.safe, "Nominal [mm]");
    assert_eq!(main.header[2].unit.as_deref(), Some("mm"));
    assert_eq!(main.header[3].text.safe, "Stage 1 Measured [mm]");
    assert_eq!(main.header[4].text.safe, "Stage 1 Deviation [mm]");
    assert_eq!(main.header[5].text.safe, "Stage 1 Uncertainty [mm]");
    assert_eq!(main.header[6].text.safe, "Stage 1 Decision");

    // Row 0 (`C01`): Nominal 11.000, Stage 1 Measured 11.001, Stage 2 Measured 11.002, Stage 3 Measured 11.003.
    assert_eq!(main.rows[0][2], Cell::Number { canonical: "11.000".to_string(), decimals: 3 });
    assert_eq!(main.rows[0][3], Cell::Number { canonical: "11.001".to_string(), decimals: 3 });
    assert_eq!(main.rows[0][4], Cell::Number { canonical: "0.001".to_string(), decimals: 3 });
    assert_eq!(main.rows[0][7], Cell::Number { canonical: "11.002".to_string(), decimals: 3 });
    assert_eq!(main.rows[0][11], Cell::Number { canonical: "11.003".to_string(), decimals: 3 });
}

#[test]
fn ac2_mixed_unit_column_splits_in_first_appearance_order() {
    let payload = parse(
        br#"{
            "header": ["Char", "Value"],
            "rows": [
                [{"kind":"text","text":"C1"}, {"kind":"value","value":{"canonical":"12.500","display":"12.500","markers":[],"unit":"mm"}}],
                [{"kind":"text","text":"C2"}, {"kind":"value","value":{"canonical":"45.0000","display":"45.0000","markers":[],"unit":"deg"}}],
                [{"kind":"text","text":"C3"}, {"kind":"value","value":{"canonical":"8.250","display":"8.250","markers":[],"unit":"mm"}}]
            ],
            "schema": "resolved_table@1"
        }"#,
    )
    .unwrap();
    let blk = GeneratedBlock {
        id: Uuid::parse("019a9f10-0001-7000-8000-000000000201").unwrap(),
        kind: BlockKindId::new("table").unwrap(),
        kind_version: 1,
        frame: None,
        payload,
    };
    let report = synthetic_report(vec![blk], None);
    let tables = convert_report(&report, false).unwrap();
    assert_eq!(tables.len(), 1);
    let t = &tables[0];
    assert_eq!(t.header.len(), 3);
    assert_eq!(t.header[0].text.safe, "Char");
    assert_eq!(t.header[1].text.safe, "Value [mm]");
    assert_eq!(t.header[2].text.safe, "Value [deg]");

    assert_eq!(t.rows[0][1], Cell::Number { canonical: "12.500".to_string(), decimals: 3 });
    assert_eq!(t.rows[0][2], Cell::Empty);
    assert_eq!(t.rows[1][1], Cell::Empty);
    assert_eq!(t.rows[1][2], Cell::Number { canonical: "45.0000".to_string(), decimals: 4 });
    assert_eq!(t.rows[2][1], Cell::Number { canonical: "8.250".to_string(), decimals: 3 });
    assert_eq!(t.rows[2][2], Cell::Empty);
}

#[test]
fn ac3_formula_injection_neutralised_preserving_source() {
    let raw = fs::read(root_dir().join("fixtures/rpt/output/sheet/injection.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let tables = convert_report(&resolved, false).unwrap();
    assert_eq!(tables.len(), 1);
    let rows = &tables[0].rows;
    assert_eq!(rows.len(), 11);

    // First 10 rows are hostile formula prefixes -> all prefixed with `'`, original kept in `source`.
    for (idx, row) in rows[..10].iter().enumerate() {
        let Cell::Text(ct) = &row[1] else {
            panic!("row {idx} expected Text cell");
        };
        assert!(ct.neutralised, "row {idx} ({:?}) must be neutralised", ct.source);
        assert!(ct.safe.starts_with('\''), "row {idx} safe text {:?} must start with apostrophe", ct.safe);
        assert_eq!(&ct.safe[1..], ct.source.as_str());
    }
    // Last row ("Safe label") is untouched.
    let Cell::Text(safe_row) = &rows[10][1] else {
        panic!("last row expected Text cell");
    };
    assert!(!safe_row.neutralised);
    assert_eq!(safe_row.safe, "Safe label");
    assert_eq!(safe_row.source, "Safe label");
}

#[test]
fn ac4_error_paths_no_table_and_si_units_mismatch() {
    // Report with only a text block -> report.unsupported_block.
    let txt = GeneratedBlock {
        id: Uuid::parse("019a9f10-0001-7000-8000-000000000301").unwrap(),
        kind: BlockKindId::new("text").unwrap(),
        kind_version: 1,
        frame: None,
        payload: parse(br#"{"schema":"resolved_text@1","text":"hello"}"#).unwrap(),
    };
    let text_only = synthetic_report(vec![txt], None);
    let err = convert_report(&text_only, false).unwrap_err();
    assert_eq!(err.code(), "report.unsupported_block");

    // Requesting SI on a report not generated with SI -> report.units_mismatch.
    let raw = fs::read(root_dir().join("fixtures/rpt/model/golden/bracket_3stage.resolved.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&raw).unwrap();
    let err_si = convert_report(&resolved, true).unwrap_err();
    assert_eq!(err_si.code(), "report.units_mismatch");

    // Unrepresentable cell kind inside a table -> report.unsupported_block naming the block.
    let bad_id = Uuid::parse("019a9f10-0001-7000-8000-000000000302").unwrap();
    let bad_tbl = GeneratedBlock {
        id: bad_id,
        kind: BlockKindId::new("table").unwrap(),
        kind_version: 1,
        frame: None,
        payload: parse(br#"{"header":["X"],"rows":[[{"kind":"sparkline"}]],"schema":"resolved_table@1"}"#).unwrap(),
    };
    let bad_rep = synthetic_report(vec![bad_tbl], Some(UnitOverrideMode::Si));
    let err_cell = convert_report(&bad_rep, true).unwrap_err();
    assert_eq!(err_cell.code(), "report.unsupported_block");
    assert!(err_cell.to_string().contains(&bad_id.to_string()));
}

#[test]
fn ac5_tie_or_nonconformance_marker_becomes_text_cell_with_marker() {
    let payload = parse(
        br#"{
            "header": ["Measured"],
            "rows": [
                [{"kind":"value","value":{
                    "canonical":"10.000",
                    "display":"10.000",
                    "markers":[{"key":"report.marker.nonconformance","text":"NONCONFORMS"},{"key":"report.marker.tie","text":"TIE"}],
                    "unit":"mm"
                }}]
            ],
            "schema": "resolved_table@1"
        }"#,
    )
    .unwrap();
    let blk = GeneratedBlock {
        id: Uuid::parse("019a9f10-0001-7000-8000-000000000401").unwrap(),
        kind: BlockKindId::new("table").unwrap(),
        kind_version: 1,
        frame: None,
        payload,
    };
    let report = synthetic_report(vec![blk], None);
    let tables = convert_report(&report, false).unwrap();
    let Cell::Text(ct) = &tables[0].rows[0][0] else {
        panic!("marked value must become a Text cell, not a bare Number");
    };
    assert!(ct.safe.contains("10.000"));
    assert!(ct.safe.contains("NONCONFORMS"));
    assert!(ct.safe.contains("TIE"));
}

#[test]
fn perf_100k_cells_is_fast() {
    let mut rows = Vec::with_capacity(10_000);
    let num_cell =
        parse(br#"{"kind":"value","value":{"canonical":"12.345","display":"12.345","markers":[],"unit":"mm"}}"#)
            .unwrap();
    let row_template = CValue::Arr(vec![num_cell; 10]);
    for _ in 0..10_000 {
        rows.push(row_template.clone());
    }
    let mut obj = std::collections::BTreeMap::new();
    obj.insert("header".to_string(), CValue::Arr((0..10).map(|i| CValue::Str(format!("C{i}"))).collect()));
    obj.insert("rows".to_string(), CValue::Arr(rows));
    obj.insert("schema".to_string(), CValue::Str("resolved_table@1".to_string()));
    let blk = GeneratedBlock {
        id: Uuid::parse("019a9f10-0001-7000-8000-000000000501").unwrap(),
        kind: BlockKindId::new("table").unwrap(),
        kind_version: 1,
        frame: None,
        payload: CValue::Obj(obj),
    };
    let report = synthetic_report(vec![blk], None);
    let start = Instant::now();
    let tables = convert_report(&report, false).unwrap();
    let elapsed_ms = start.elapsed().as_millis();
    assert_eq!(tables[0].rows.len(), 10_000);
    assert_eq!(tables[0].header.len(), 10);
    eprintln!("RPT.OUTPUT.SHEET.CELL converted 100,000 cells in {elapsed_ms} ms (debug)");
    assert!(elapsed_ms < 5_000);
}
