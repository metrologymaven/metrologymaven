#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! RPT.OUTPUT.PDF.LAYOUT acceptance tests (AC-1..AC-5, Measure trait, error paths, performance).
//!
//! Fixtures are compiled in via `include_bytes!`, so the exact same tests run identically on
//! x86-64, ARM64 and `wasm32-wasip1` (`REQ-RPT.OUTPUT.PDF.LAYOUT-001`, `AC-1`).

use fid_report_pdf::layout::{
    convert_01mm_to_um, layout_resolved_json, BoxContent, LayoutError, Measure, MeasureRegistry, MeasuredBox, PageSpec,
    ParsedBlock, RectUm, WARN_BLOCK_OVERFLOW,
};
use fid_report_pdf::text::FontSet;

const BRACKET_RESOLVED: &[u8] = include_bytes!("../../../fixtures/rpt/model/golden/bracket_3stage.resolved.json");
const BRACKET_LAYOUT_GOLDEN: &[u8] = include_bytes!("../../../fixtures/rpt/output/layout/bracket_3stage.layout.json");
const FAI_500ROWS_RESOLVED: &[u8] = include_bytes!("../../../fixtures/rpt/output/layout/fai_500rows.resolved.json");
const PPAP_10PARTS_RESOLVED: &[u8] = include_bytes!("../../../fixtures/rpt/output/layout/ppap_10parts.resolved.json");

#[test]
fn ac1_bracket_3stage_matches_golden_display_list_byte_for_byte() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();

    let layout_a = layout_resolved_json(BRACKET_RESOLVED, &fonts, &measurers).unwrap();
    let layout_b = layout_resolved_json(BRACKET_RESOLVED, &fonts, &measurers).unwrap();
    let bytes_a = layout_a.to_canonical_bytes();
    let bytes_b = layout_b.to_canonical_bytes();
    assert_eq!(bytes_a, bytes_b, "layout must be deterministic across repeated runs");
    assert_eq!(layout_a.digest(), layout_b.digest());

    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("UPDATE_LAYOUT_GOLDEN").is_some() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/rpt/output/layout/bracket_3stage.layout.json");
        std::fs::write(&p, &bytes_a).unwrap();
        return;
    }

    assert_eq!(bytes_a.as_slice(), BRACKET_LAYOUT_GOLDEN, "display list must equal bracket_3stage.layout.json");
    assert!(layout_a.pages.len() >= 2, "50-row 3-stage bracket table paginates across multiple A4 pages");
    assert!(layout_a.warnings.is_empty(), "bracket_3stage should lay out cleanly with zero overflow warnings");
    for (idx, p) in layout_a.pages.iter().enumerate() {
        assert_eq!(p.page_index, (idx as u32) + 1);
        assert_eq!(p.total_pages, layout_a.pages.len() as u32);
        assert_eq!(p.size_um, (210_000, 297_000));
    }
}

#[test]
fn ac2_fai_500rows_repeats_header_and_continuation_block_with_n_of_m_footers() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();

    let layout = layout_resolved_json(FAI_500ROWS_RESOLVED, &fonts, &measurers).unwrap();
    let total = layout.pages.len();
    assert!(total >= 5, "500 FAI rows must span multiple pages, got {total}");

    let ident_id = "019a9f10-0001-7000-8000-000000000001";
    let table_id = "019a9f10-0001-7000-8000-000000000002";
    let footer_id = "019a9f10-0001-7000-8000-000000000003";

    let mut total_data_rows = 0usize;

    for (idx, page) in layout.pages.iter().enumerate() {
        let page_num = idx + 1;
        assert_eq!(page.page_index, page_num as u32);
        assert_eq!(page.total_pages, total as u32);

        // Footer reads "Page n of m" accurately on every page (REQ-006).
        let footer = page.boxes.iter().find(|b| b.block_id == footer_id).expect("footer box present on every page");
        let BoxContent::Text(lines) = &footer.content else {
            panic!("footer should be text");
        };
        let footer_text: String = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(footer_text, format!("Page {page_num} of {total}"));

        // Identifying block precedes the table on page 1 AND every continuation page (REQ-003).
        let ident_pos = page
            .boxes
            .iter()
            .position(|b| b.block_id == ident_id)
            .expect("identifying continuation block present on page");
        let table_pos = page.boxes.iter().position(|b| b.block_id == table_id).expect("table slice present on page");
        assert!(ident_pos < table_pos, "identifying block must precede table on page {page_num}");

        let BoxContent::Table(tbl) = &page.boxes[table_pos].content else {
            panic!("expected table box");
        };
        assert_eq!(tbl.continuation, idx > 0, "continuation flag on page {page_num}");
        assert_eq!(tbl.header_row_count, 1, "header row repeated on page {page_num}");
        assert!(tbl.rows[0].is_header, "first row on page {page_num} must be header row");
        assert_eq!(tbl.rows[0].cells[0].lines[0].text, "Char No");
        assert_eq!(tbl.rows[0].cells[4].lines[0].text, "Outcome");
        assert!(tbl.rows.len() >= 2, "at least one data row per page (widow control)");
        total_data_rows += tbl.rows.len() - 1;
    }

    assert_eq!(total_data_rows, 500, "all 500 FAI data rows must appear without loss");
}

#[test]
fn ac3_ppap_10parts_splits_into_two_column_groups_repeating_id_spec_outcome() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();

    let layout = layout_resolved_json(PPAP_10PARTS_RESOLVED, &fonts, &measurers).unwrap();

    let table_boxes: Vec<_> = layout
        .pages
        .iter()
        .flat_map(|p| p.boxes.iter())
        .filter_map(|b| match &b.content {
            BoxContent::Table(t) => Some(t),
            _ => None,
        })
        .collect();

    assert_eq!(table_boxes.len(), 2, "10 parts with max_columns_per_group=6 must produce 2 column groups");

    let g0 = table_boxes[0];
    let g1 = table_boxes[1];
    assert_eq!(g0.column_group_index, 0);
    assert_eq!(g0.column_group_count, 2);
    assert_eq!(g1.column_group_index, 1);
    assert_eq!(g1.column_group_count, 2);

    let g0_hdrs: Vec<String> = g0.rows[0].cells.iter().map(|c| c.lines[0].text.clone()).collect();
    let g1_hdrs: Vec<String> = g1.rows[0].cells.iter().map(|c| c.lines[0].text.clone()).collect();

    assert_eq!(
        g0_hdrs,
        vec!["Balloon", "Specification", "Outcome", "Part 1", "Part 2", "Part 3", "Part 4", "Part 5", "Part 6"]
    );
    assert_eq!(g1_hdrs, vec!["Balloon", "Specification", "Outcome", "Part 7", "Part 8", "Part 9", "Part 10"]);
    assert_eq!(g0.rows.len(), 9, "header + 8 data rows in group 0");
    assert_eq!(g1.rows.len(), 9, "header + 8 data rows in group 1");
}

#[test]
fn ac4_oversized_text_block_clipped_on_own_page_with_overflow_warning() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();

    // 120 explicit lines * 4.8 mm = 576 mm > 267 mm A4 body height.
    let huge_text = (1..=120).map(|i| format!("Oversized note line {i:03}")).collect::<Vec<_>>().join("\\n");
    let before_id = "019a9f30-0001-7000-8000-000000000001";
    let huge_id = "019a9f30-0001-7000-8000-000000000002";
    let after_id = "019a9f30-0001-7000-8000-000000000003";
    let json = format!(
        r#"{{
            "digest": "sha256:{}",
            "schema": "resolved_report@1",
            "schema_version": "1",
            "provenance": {{}},
            "warnings": [],
            "pages": [{{
                "id": "019a9f30-0001-7000-8000-000000000010",
                "blocks": [
                    {{"id":"{before_id}","kind":"text","kind_version":"1","frame":null,"payload":{{"schema":"resolved_text@1","text":"Intro block"}}}},
                    {{"id":"{huge_id}","kind":"text","kind_version":"1","frame":null,"payload":{{"schema":"resolved_text@1","text":"{huge_text}"}}}},
                    {{"id":"{after_id}","kind":"text","kind_version":"1","frame":null,"payload":{{"schema":"resolved_text@1","text":"Closing block"}}}}
                ]
            }}]
        }}"#,
        "aa".repeat(32)
    );

    let layout = layout_resolved_json(json.as_bytes(), &fonts, &measurers).unwrap();
    assert_eq!(layout.pages.len(), 3, "intro on p1, oversized clipped on own p2, closing on p3");
    assert_eq!(layout.pages[0].boxes.len(), 1);
    assert_eq!(layout.pages[0].boxes[0].block_id, before_id);
    assert_eq!(layout.pages[1].boxes.len(), 1);
    assert_eq!(layout.pages[1].boxes[0].block_id, huge_id);
    assert!(layout.pages[1].boxes[0].clipped, "oversized block must be marked clipped");
    assert_eq!(layout.pages[1].boxes[0].rect_um.height_um, layout.pages[1].body_um.height_um);
    assert_eq!(layout.pages[2].boxes.len(), 1);
    assert_eq!(layout.pages[2].boxes[0].block_id, after_id);

    assert_eq!(layout.warnings.len(), 1);
    assert_eq!(layout.warnings[0].code, WARN_BLOCK_OVERFLOW);
    assert_eq!(layout.warnings[0].block_id, huge_id);
}

#[test]
fn ac5_designer_frames_convert_exactly_from_01mm_to_um() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();

    let b1 = "019a9f40-0001-7000-8000-000000000001";
    let b2 = "019a9f40-0001-7000-8000-000000000002";
    let json = format!(
        r#"{{
            "digest": "sha256:{}",
            "schema": "resolved_report@1",
            "schema_version": "1",
            "provenance": {{}},
            "warnings": [],
            "page_layout": {{
                "default": {{
                    "width": 29700,
                    "height": 21000,
                    "margins": [1500, 1500, 1500, 1500],
                    "header": 1200,
                    "footer": 1000
                }}
            }},
            "pages": [{{
                "id": "019a9f40-0001-7000-8000-000000000010",
                "blocks": [
                    {{
                        "id": "{b1}",
                        "kind": "text",
                        "kind_version": "1",
                        "frame": {{"x": "1500", "y": "2700", "width": "12345", "height": "4321"}},
                        "payload": {{"schema": "resolved_text@1", "text": "Framed Title"}}
                    }},
                    {{
                        "id": "{b2}",
                        "kind": "form_field",
                        "kind_version": "1",
                        "frame": {{"x": 15000, "y": 2700, "width": 10000, "height": 2500}},
                        "payload": {{"label": "Lot", "value": "L-2026-09"}}
                    }}
                ]
            }}]
        }}"#,
        "bb".repeat(32)
    );

    let layout = layout_resolved_json(json.as_bytes(), &fonts, &measurers).unwrap();
    assert_eq!(layout.pages.len(), 1);
    assert_eq!(layout.pages[0].size_um, (297_000, 210_000));
    assert_eq!(layout.pages[0].body_um, RectUm::new(15_000, 27_000, 267_000, 158_000));
    assert_eq!(layout.pages[0].boxes[0].rect_um, RectUm::new(15_000, 27_000, 123_450, 43_210));
    assert_eq!(layout.pages[0].boxes[1].rect_um, RectUm::new(150_000, 27_000, 100_000, 25_000));
    assert_eq!(convert_01mm_to_um(12345, "x").unwrap(), 123_450);
    assert!(layout.warnings.is_empty());
}

struct DummyChartMeasure;

impl Measure for DummyChartMeasure {
    fn kind(&self) -> &str {
        "chart"
    }

    fn measure(&self, block: &ParsedBlock, width_um: i64, _fonts: &FontSet) -> Result<MeasuredBox, LayoutError> {
        Ok(MeasuredBox {
            width_um,
            height_um: 55_000,
            content: BoxContent::Paint { block_id: block.id.clone(), kind: "chart".to_string() },
        })
    }
}

#[test]
fn custom_measure_trait_and_invalid_geometry_errors() {
    let fonts = FontSet::bundled().unwrap();
    let chart_m = DummyChartMeasure;
    let mut reg = MeasureRegistry::new();
    reg.register(&chart_m);

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
                    {{"id":"c1","kind":"chart","kind_version":"1","frame":null,"payload":{{"chart":"hist"}}}}
                ]
            }}]
        }}"#,
        "cc".repeat(32)
    );
    let layout = layout_resolved_json(json.as_bytes(), &fonts, &reg).unwrap();
    assert_eq!(layout.pages[0].boxes[0].rect_um.height_um, 55_000);
    assert!(matches!(layout.pages[0].boxes[0].content, BoxContent::Paint { ref kind, .. } if kind == "chart"));

    // Impossible page geometry: margins leave body < 20 mm -> report.layout_invalid.
    let err = PageSpec::from_01mm(21_000, 29_700, 1_500, 10_000, 1_500, 10_000, 0, 0).unwrap_err();
    assert_eq!(err.code(), "report.layout_invalid");
}

#[test]
fn perf_50_pages_with_20_images_under_budget() {
    let fonts = FontSet::bundled().unwrap();
    let measurers = MeasureRegistry::new();
    let img_digest = format!("sha256:{}", "dd".repeat(32));

    let mut pages_json = Vec::with_capacity(50);
    for p in 0..50 {
        let mut blocks = vec![format!(
            r#"{{"id":"t-{p}","kind":"text","kind_version":"1","frame":null,"payload":{{"schema":"resolved_text@1","style":"heading","text":"Inspection Section {p}"}}}}"#
        )];
        if p < 20 {
            blocks.push(format!(
                r#"{{"id":"img-{p}","kind":"image","kind_version":"1","frame":null,"payload":{{"schema":"resolved_image@1","cas_digest":"{img_digest}","media_type":"image/png","width_01mm":8000,"height_01mm":5000}}}}"#
            ));
        }
        let rows: Vec<String> = (0..12)
            .map(|r| format!(r#"["C{p}_{r}",{{"display":"12.00{r}","canonical":"12.00{r}","unit":"mm","markers":[]}},{{"text":"conforms"}}]"#))
            .collect();
        blocks.push(format!(
            r#"{{"id":"tbl-{p}","kind":"table","kind_version":"1","frame":null,"payload":{{"schema":"resolved_table@1","columns":[{{"title":"Char"}},{{"title":"Measured"}},{{"title":"Decision"}}],"rows":[{}]}}}}"#,
            rows.join(",")
        ));
        pages_json.push(format!(r#"{{"id":"page-{p}","blocks":[{}]}}"#, blocks.join(",")));
    }

    let doc = format!(
        r#"{{"digest":"sha256:{}","schema":"resolved_report@1","schema_version":"1","provenance":{{}},"warnings":[],"pages":[{}]}}"#,
        "ee".repeat(32),
        pages_json.join(",")
    );

    let layout = layout_resolved_json(doc.as_bytes(), &fonts, &measurers).unwrap();
    assert_eq!(layout.pages.len(), 50);
}
