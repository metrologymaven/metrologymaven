#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! RPT.OUTPUT.PDF.DRAW acceptance tests (AC-1..AC-5, JPEG pass-through, custom painter, perf).
//!
//! Fixtures are compiled in via `include_bytes!`, so all tests run identically on x86-64,
//! ARM64 and `wasm32-wasip1`.

use fid_report_pdf::draw::{extract_painted_strings, paint_document, MemoryImageStore, PaintError, PainterRegistry};
use fid_report_pdf::image_codec::{decode_image, PdfColorSpace, PdfImageFilter};
use fid_report_pdf::layout::{layout_document, MeasureRegistry, ResolvedDoc};
use fid_report_pdf::text::pdf::{fmt_milli, milli_points};
use fid_report_pdf::text::FontSet;
use std::collections::BTreeMap;

const BRACKET_RESOLVED: &[u8] = include_bytes!("../../../fixtures/rpt/model/golden/bracket_3stage.resolved.json");
const BALLOONED_PNG: &[u8] = include_bytes!("../../../fixtures/rpt/output/draw/ballooned_sheet.png");
const BALLOONED_RESOLVED: &[u8] = include_bytes!("../../../fixtures/rpt/output/draw/ballooned_sheet.resolved.json");

fn is_numeric_token(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let rest = t.strip_prefix('-').or_else(|| t.strip_prefix('+')).unwrap_or(t);
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
}

#[test]
fn ac1_bracket_3stage_text_extraction_matches_every_value_text_once_and_nothing_else_numeric() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();
    let painters = PainterRegistry::default_builtin();
    let store = MemoryImageStore::new();

    let doc = ResolvedDoc::from_json_bytes(BRACKET_RESOLVED).unwrap();
    let layout = layout_document(&doc, &fonts, &measurers).unwrap();
    let painted = paint_document(&doc, &layout, &fonts, &store, &painters).unwrap();

    let extracted = extract_painted_strings(&painted);
    let numeric_extracted: Vec<String> = extracted.iter().filter(|s| is_numeric_token(s)).cloned().collect();

    // Collect every ValueText.display occurrence from bracket_3stage.resolved.json directly.
    let mut expected_counts: BTreeMap<String, usize> = BTreeMap::new();
    for page in &doc.pages {
        for block in &page.blocks {
            if block.kind != "table" {
                continue;
            }
            if let Some(rows) = block.payload.get("rows").and_then(|v| v.as_arr()) {
                for row in rows {
                    if let Some(nom) = row.get("nominal").and_then(|v| v.get("display")).and_then(|v| v.as_str()) {
                        *expected_counts.entry(nom.to_string()).or_default() += 1;
                    }
                    if let Some(cells) = row.get("cells").and_then(|v| v.as_arr()) {
                        for stage in cells {
                            for field in ["measured", "deviation"] {
                                if let Some(d) =
                                    stage.get(field).and_then(|v| v.get("display")).and_then(|v| v.as_str())
                                {
                                    *expected_counts.entry(d.to_string()).or_default() += 1;
                                }
                            }
                            if let Some(d) = stage
                                .get("uncertainty")
                                .and_then(|u| u.get("value"))
                                .and_then(|v| v.get("display"))
                                .and_then(|v| v.as_str())
                            {
                                *expected_counts.entry(d.to_string()).or_default() += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    let mut actual_counts: BTreeMap<String, usize> = BTreeMap::new();
    for tok in &numeric_extracted {
        *actual_counts.entry(tok.clone()).or_default() += 1;
    }

    // 50 characteristics * (1 nominal + 3 stages * 3 values) = 500 ValueText occurrences.
    let expected_total: usize = expected_counts.values().sum();
    assert_eq!(expected_total, 500);
    assert_eq!(
        actual_counts, expected_counts,
        "extracted numeric tokens must match every ValueText.display occurrence exactly once and nothing else"
    );
}

#[test]
fn ac2_unregistered_chart_block_fails_with_report_unsupported_block_naming_block() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();
    let painters = PainterRegistry::default_builtin();
    let store = MemoryImageStore::new();

    let chart_id = "019a9f60-0001-7000-8000-000000000042";
    let json = format!(
        r#"{{
            "digest": "sha256:{}",
            "schema": "resolved_report@1",
            "schema_version": "1",
            "provenance": {{}},
            "warnings": [],
            "pages": [{{
                "id": "p1",
                "blocks": [
                    {{"id":"t1","kind":"text","kind_version":"1","frame":null,"payload":{{"schema":"resolved_text@1","text":"Summary"}}}},
                    {{"id":"{chart_id}","kind":"chart","kind_version":"1","frame":null,"payload":{{"schema":"chart@1"}}}}
                ]
            }}]
        }}"#,
        "aa".repeat(32)
    );

    let doc = ResolvedDoc::from_json_bytes(json.as_bytes()).unwrap();
    let layout = layout_document(&doc, &fonts, &measurers).unwrap();
    let err = paint_document(&doc, &layout, &fonts, &store, &painters).unwrap_err();
    assert_eq!(err.code(), "report.unsupported_block");
    match err {
        PaintError::UnsupportedBlock { block_id, kind } => {
            assert_eq!(block_id, chart_id);
            assert_eq!(kind, "chart");
        }
        other => panic!("expected UnsupportedBlock, got {other:?}"),
    }
}

#[test]
fn ac3_ballooned_sheet_12_vector_circles_match_stored_coordinates_within_1_um() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();
    let painters = PainterRegistry::default_builtin();
    let mut store = MemoryImageStore::new();
    store.insert(BALLOONED_PNG.to_vec());

    let doc = ResolvedDoc::from_json_bytes(BALLOONED_RESOLVED).unwrap();
    let layout = layout_document(&doc, &fonts, &measurers).unwrap();
    let painted = paint_document(&doc, &layout, &fonts, &store, &painters).unwrap();

    assert_eq!(painted.pages.len(), 1);
    assert_eq!(painted.images.len(), 1);
    let stream = String::from_utf8_lossy(&painted.pages[0].content_stream);

    // Frame origin is (1500, 3000) in 0.01 mm = (15_000, 30_000) µm on A4 height 297_000 µm.
    // Balloon radius = 250 * 10 = 2_500 µm.
    // Expected 12 balloon centres in top-down page µm: (15_000 + x*10, 30_000 + y*10).
    let expected_coords_01mm = [
        (2000i64, 1500i64),
        (4500, 1500),
        (7000, 1500),
        (9500, 1500),
        (12000, 1500),
        (14000, 1500),
        (2000, 8500),
        (4500, 8500),
        (7000, 8500),
        (9500, 8500),
        (12000, 8500),
        (14000, 8500),
    ];

    // Parse all 4-segment closed Bezier circles ("<x> <y> m" followed by 4 "... c" lines and "h B")
    // directly from the PDF content stream and verify every circle centre is within 1 µm!
    let lines: Vec<&str> = stream.lines().collect();
    let mut found_centres_pt_milli: Vec<(i64, i64)> = Vec::new();
    for i in 0..lines.len() {
        if lines[i].ends_with(" m") && i + 5 < lines.len() && lines[i + 5] == "h B" {
            // Line i: E = (cx + r, py)
            // Line i+2: W = (cx - r, py) at end of 2nd Bezier curve
            let e_parts: Vec<&str> = lines[i].split_whitespace().collect();
            let w_parts: Vec<&str> = lines[i + 2].split_whitespace().collect();
            let ex = parse_milli(e_parts[0]);
            let ey = parse_milli(e_parts[1]);
            let wx = parse_milli(w_parts[4]);
            let wy = parse_milli(w_parts[5]);
            assert_eq!(ey, wy, "circle E and W cardinal points must share Y");
            found_centres_pt_milli.push(((ex + wx) / 2, ey));
        }
    }

    assert_eq!(found_centres_pt_milli.len(), 12, "must emit 12 vector balloon circles");
    for (idx, &(x_01mm, y_01mm)) in expected_coords_01mm.iter().enumerate() {
        let want_cx_um = 15_000 + x_01mm * 10;
        let want_cy_pdf_um = 297_000 - (30_000 + y_01mm * 10);
        let want_x_mpt = milli_points(want_cx_um);
        let want_y_mpt = milli_points(want_cy_pdf_um);
        let (got_x_mpt, got_y_mpt) = found_centres_pt_milli[idx];
        // 1 µm = 2.835 milli-points; require exact match (<= 1 milli-point = 0.35 µm << 1 µm!).
        assert!(
            (got_x_mpt - want_x_mpt).abs() <= 1 && (got_y_mpt - want_y_mpt).abs() <= 1,
            "balloon {} centre mismatch: got ({got_x_mpt}, {got_y_mpt}) mpt, want ({want_x_mpt}, {want_y_mpt}) mpt",
            idx + 1
        );
    }

    // Verify all 12 balloon numbers ("1".."12") extract cleanly from the painted content stream.
    let extracted = extract_painted_strings(&painted);
    for n in 1..=12 {
        assert!(extracted.iter().any(|s| s == &n.to_string()), "balloon label {n} missing");
    }
}

fn parse_milli(s: &str) -> i64 {
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (whole, frac) = rest.split_once('.').unwrap();
    let v = whole.parse::<i64>().unwrap() * 1000 + frac.parse::<i64>().unwrap();
    if neg {
        -v
    } else {
        v
    }
}

#[test]
fn ac4_same_png_byte_identical_across_reports_and_corrupted_digest_fails_snapshot_missing() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();
    let painters = PainterRegistry::default_builtin();

    let mut store = MemoryImageStore::new();
    let img_digest = store.insert(BALLOONED_PNG.to_vec());

    let make_report = |report_seed: &str, title: &str| {
        format!(
            r#"{{
                "digest": "sha256:{}",
                "schema": "resolved_report@1",
                "schema_version": "1",
                "provenance": {{}},
                "warnings": [],
                "pages": [{{
                    "id": "p1",
                    "blocks": [
                        {{"id":"t1","kind":"text","kind_version":"1","frame":null,"payload":{{"schema":"resolved_text@1","text":"{title}"}}}},
                        {{"id":"img1","kind":"image","kind_version":"1","frame":null,"payload":{{"schema":"resolved_image@1","cas_digest":"{}","media_type":"image/png"}}}}
                    ]
                }}]
            }}"#,
            report_seed.repeat(32),
            img_digest.as_str()
        )
    };

    let d1 = ResolvedDoc::from_json_bytes(make_report("11", "Report One").as_bytes()).unwrap();
    let d2 = ResolvedDoc::from_json_bytes(make_report("22", "Report Two").as_bytes()).unwrap();
    let l1 = layout_document(&d1, &fonts, &measurers).unwrap();
    let l2 = layout_document(&d2, &fonts, &measurers).unwrap();
    let p1 = paint_document(&d1, &l1, &fonts, &store, &painters).unwrap();
    let p2 = paint_document(&d2, &l2, &fonts, &store, &painters).unwrap();

    assert_eq!(
        p1.images.get(&img_digest),
        p2.images.get(&img_digest),
        "same PNG must yield byte-identical image XObject"
    );
    let obj = p1.images.get(&img_digest).unwrap();
    assert_eq!(obj.filter, PdfImageFilter::FlateDecode);
    assert_eq!(obj.color_space, PdfColorSpace::DeviceRgb);
    assert_eq!((obj.width_px, obj.height_px), (160, 100));

    // Corrupted bytes stored under img_digest -> must fail with report.snapshot_missing!
    let mut bad_store = MemoryImageStore::new();
    let mut corrupted = BALLOONED_PNG.to_vec();
    corrupted[0] ^= 0xFF;
    bad_store.insert_under_digest(img_digest, corrupted);
    let err = paint_document(&d1, &l1, &fonts, &bad_store, &painters).unwrap_err();
    assert_eq!(err.code(), "report.snapshot_missing");

    // Missing digest entirely -> report.snapshot_missing!
    let empty_store = MemoryImageStore::new();
    let err2 = paint_document(&d1, &l1, &fonts, &empty_store, &painters).unwrap_err();
    assert_eq!(err2.code(), "report.snapshot_missing");
}

#[test]
fn ac5_tie_and_nonconformance_markers_appear_next_to_value_in_content_stream() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();
    let painters = PainterRegistry::default_builtin();
    let store = MemoryImageStore::new();

    let json = format!(
        r#"{{
            "digest": "sha256:{}",
            "schema": "resolved_report@1",
            "schema_version": "1",
            "provenance": {{}},
            "warnings": [],
            "pages": [{{
                "id": "p1",
                "blocks": [{{
                    "id": "tbl1",
                    "kind": "table",
                    "kind_version": "1",
                    "frame": null,
                    "payload": {{
                        "schema": "resolved_table@1",
                        "columns": [{{"title":"Char"}}, {{"title":"Measured"}}, {{"title":"Decision"}}],
                        "rows": [
                            ["C01", {{"display":"10.050","canonical":"10.050","unit":"mm","markers":["*"]}}, {{"text":"nonconforming"}}],
                            ["C02", {{"display":"25.000","canonical":"25.000","unit":"mm","markers":["[TIE]"]}}, {{"text":"indeterminate"}}]
                        ]
                    }}
                }}]
            }}]
        }}"#,
        "55".repeat(32)
    );

    let doc = ResolvedDoc::from_json_bytes(json.as_bytes()).unwrap();
    let layout = layout_document(&doc, &fonts, &measurers).unwrap();
    let painted = paint_document(&doc, &layout, &fonts, &store, &painters).unwrap();
    let extracted = extract_painted_strings(&painted);

    assert!(
        extracted.iter().any(|s| s.contains("10.050") && s.contains('*')),
        "nonconformance marker must appear next to value in content stream; got {extracted:?}"
    );
    assert!(
        extracted.iter().any(|s| s.contains("25.000") && s.contains("[TIE]")),
        "tie marker must appear next to value in content stream; got {extracted:?}"
    );
}

#[test]
fn jpeg_passthrough_and_milli_formatting_smoke() {
    // Minimal valid baseline JPEG (SOI + SOF0 4x2 RGB + SOS + EOI).
    let jpeg: Vec<u8> = vec![
        0xFF, 0xD8, // SOI
        0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x04, 0x03, // SOF0: 8-bit, h=2, w=4, 3 comps
        0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, // components
        0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, // SOS
        0x00, // dummy scan byte
        0xFF, 0xD9, // EOI
    ];
    let obj = decode_image(&jpeg).unwrap();
    assert_eq!(obj.filter, PdfImageFilter::DctDecode);
    assert_eq!(obj.color_space, PdfColorSpace::DeviceRgb);
    assert_eq!((obj.width_px, obj.height_px), (4, 2));
    assert_eq!(obj.data, jpeg, "JPEG bytes must pass through unchanged");
    assert_eq!(fmt_milli(1000), "1.000");
}
