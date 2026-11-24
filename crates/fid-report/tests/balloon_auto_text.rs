//! Acceptance and robustness tests for `RPT.BALLOON.AUTO.TEXT` (`fid_report::balloon::auto_text`, AC-1..AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{to_canonical_bytes, CValue};
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_project::Uuid;
use fid_report::balloon::auto_text::{
    command_def, execute, extract_drawing_text, extract_pdf_page, render_sheet_tile, AutoTextError,
    ExtractDrawingTextParams, PageKind, SheetRect, EXTRACT_DRAWING_TEXT,
};
use fid_report::balloon::sheet::{
    add_drawing_balloon, AddDrawingParams, DeterministicV7Gen, DrawingStore, ImportEnv, MemoryDrawingBlobs, RevisionId,
    SheetId,
};

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn auto_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/balloon/auto")
}

fn part_rev_id(seq: u8) -> RevisionId {
    let bytes = [0x01, 0x90, 0x00, 0x00, 0x00, 0x02, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, seq];
    RevisionId::new(Uuid::from_bytes(bytes)).expect("valid uuidv7")
}

fn import_auto_fixture(
    store: &mut DrawingStore,
    blobs: &mut MemoryDrawingBlobs,
    ids: &mut DeterministicV7Gen,
    filename: &str,
    dn: &str,
) -> SheetId {
    let mut roots = RootSet::new();
    roots.insert(RootId::new("auto").expect("root id"), RootAccess::Import, auto_fixtures_dir());
    let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs, ids };
    let res = add_drawing_balloon(
        store,
        &mut env,
        &AddDrawingParams {
            part_rev: part_rev_id(1),
            file: format!("root://auto/{filename}"),
            drawing_number: dn.into(),
            revision_label: "A".into(),
            title: None,
            dry_run: false,
        },
    )
    .expect("import auto fixture");
    res.sheets[0].id
}

fn build_pdf(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (num, body) in objects {
        offsets.insert(*num, out.len());
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let max_num = offsets.keys().copied().max().unwrap_or(0);
    let xref_pos = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max_num + 1).as_bytes());
    for i in 1..=max_num {
        if let Some(off) = offsets.get(&i) {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        } else {
            out.extend_from_slice(b"0000000000 65535 f \n");
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n", max_num + 1).as_bytes(),
    );
    out
}

// ------------------------------------------------------------------ AC-1 & AC-4

#[test]
fn ac1_and_ac4_vector_bracket_ground_truth_and_determinism() {
    let mut store = DrawingStore::new();
    let mut blobs = MemoryDrawingBlobs::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_100_000);
    let sheet_id = import_auto_fixture(&mut store, &mut blobs, &mut ids, "vector_bracket.pdf", "BRK-01");

    let map1 =
        extract_drawing_text(&store, &blobs, &ExtractDrawingTextParams { sheet: sheet_id, include_frame_boxes: true })
            .expect("extract vector_bracket.pdf");

    assert_eq!(map1.page_kind, PageKind::Vector);
    assert_eq!(map1.runs.len(), 15, "expected 12 dimensions + 3 GD&T frames");
    assert_eq!(map1.frame_boxes.len(), 3, "expected 3 GD&T callout frame boxes");

    // Load committed ground-truth JSON and assert every callout matches within +-5 units (0.05 mm).
    let truth_raw = std::fs::read(auto_fixtures_dir().join("vector_bracket.truth.json")).expect("read truth");
    let truth_val = fid_canon::parse(&truth_raw).expect("parse truth json");
    let CValue::Obj(tm) = &truth_val else { panic!("truth not object") };
    let CValue::Arr(truth_runs) = tm.get("runs").expect("runs") else { panic!("truth runs") };
    assert_eq!(truth_runs.len(), 15);

    for tr in truth_runs {
        let CValue::Obj(rm) = tr else { panic!("run obj") };
        let CValue::Str(want_text) = rm.get("text").expect("text") else { panic!("text") };
        let CValue::Obj(bm) = rm.get("bbox").expect("bbox") else { panic!("bbox") };
        let get_num = |k: &str| -> i64 {
            match bm.get(k) {
                Some(CValue::Num(f)) => *f as i64,
                _ => panic!("missing {k}"),
            }
        };
        let (wx0, wy0, wx1, wy1) = (get_num("x0"), get_num("y0"), get_num("x1"), get_num("y1"));
        let found = map1
            .runs
            .iter()
            .find(|r| r.text.untrusted_value().as_str() == want_text)
            .unwrap_or_else(|| panic!("callout {want_text:?} not found in {:?}", map1.runs));
        assert!(
            (found.bbox.x0 - wx0).abs() <= 5
                && (found.bbox.y0 - wy0).abs() <= 5
                && (found.bbox.x1 - wx1).abs() <= 5
                && (found.bbox.y1 - wy1).abs() <= 5,
            "callout {want_text:?} bbox {:?} deviates > 5 cmm from ({wx0},{wy0},{wx1},{wy1})",
            found.bbox
        );
        assert!((found.font_h_cmm - 350).abs() <= 5, "font height {} != 350", found.font_h_cmm);
        assert_eq!(found.angle_mdeg, 0);
    }

    // AC-4: Running twice produces byte-identical RFC 8785 canonical JSON and identical SheetRects.
    let map2 =
        extract_drawing_text(&store, &blobs, &ExtractDrawingTextParams { sheet: sheet_id, include_frame_boxes: true })
            .expect("second extract");
    assert_eq!(map1, map2);
    let bytes1 = to_canonical_bytes(&map1.to_value()).expect("canonical 1");
    let bytes2 = to_canonical_bytes(&map2.to_value()).expect("canonical 2");
    assert_eq!(bytes1, bytes2);

    // Registry CommandDef schema validation on params and output.
    let def = command_def().expect("command_def valid");
    assert_eq!(def.id.as_str(), "report.extract_drawing_text");
    let p_schema = Compiled::compile(&def.params_schema).expect("compile params");
    let r_schema = Compiled::compile(&def.result_schema).expect("compile result");
    let p_json = format!(r#"{{"sheet":"{sheet_id}","include_frame_boxes":true}}"#);
    let p_val = fid_canon::parse(p_json.as_bytes()).expect("parse params");
    assert!(p_schema.evaluate(&p_val, &NoHooks).is_ok());
    let out_val = execute(EXTRACT_DRAWING_TEXT, &p_val, &store, &blobs).expect("execute ok");
    assert!(r_schema.evaluate(&out_val, &NoHooks).is_ok());

    // REQ-005: Render a bounded RGBA8 cropped snippet around the first GD&T frame at 150 dpi.
    let fb = map1.frame_boxes[0];
    let tile = render_sheet_tile(
        &store,
        &blobs,
        sheet_id,
        SheetRect::new(fb.x0 - 500, fb.y0 - 500, fb.x1 + 500, fb.y1 + 500),
        150,
    )
    .expect("render tile ok");
    assert!(tile.width > 0 && tile.height > 0);
    assert_eq!(tile.rgba.len(), (tile.width as usize) * (tile.height as usize) * 4);
    // Confirm non-white ink pixels were rendered into the snippet.
    assert!(tile.rgba.chunks_exact(4).any(|px| px[0] != 0xFF || px[1] != 0xFF || px[2] != 0xFF));

    // Requesting > 300 dpi fails closed with limit_exceeded.
    let err = render_sheet_tile(&store, &blobs, sheet_id, fb, 600).expect_err("600 dpi must fail");
    assert_eq!(err.code(), "report.limit_exceeded");
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_scanned_only_pdf_classifies_raster_only_with_empty_runs() {
    let mut store = DrawingStore::new();
    let mut blobs = MemoryDrawingBlobs::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_200_000);
    let sheet_id = import_auto_fixture(&mut store, &mut blobs, &mut ids, "scanned_only.pdf", "SCAN-01");

    let map =
        extract_drawing_text(&store, &blobs, &ExtractDrawingTextParams { sheet: sheet_id, include_frame_boxes: true })
            .expect("extract scanned_only.pdf");
    assert_eq!(map.page_kind, PageKind::RasterOnly);
    assert!(map.runs.is_empty());
}

// ------------------------------------------------------------------ AC-3 (hostile run bomb + gstate bomb + IO.CORP)

#[test]
fn ac3_hostile_100k_runs_and_gstate_bomb_fail_closed_no_panic() {
    let fake_sheet =
        SheetId::new(Uuid::from_bytes([0x01, 0x90, 0x00, 0x00, 0x00, 0x09, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 1]))
            .expect("uuidv7");

    // 1. 60,000 synthetic text runs (> 50,000 cap) -> limit_exceeded, zero panics.
    let mut stream = String::from("BT /F1 10 Tf 10 10 Td\n");
    for _ in 0..60_000 {
        stream.push_str("(A) Tj\n");
    }
    stream.push_str("ET\n");
    let stream_obj = format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()).into_bytes();
    let pdf_runs = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] /Contents 4 0 R >>".to_vec()),
        (4, stream_obj),
    ]);
    let err = extract_pdf_page(fake_sheet, &pdf_runs, 0, true).expect_err("60k runs must fail");
    assert_eq!(err.code(), "report.limit_exceeded");
    match err {
        AutoTextError::LimitExceeded { limit, .. } => assert_eq!(limit, "text_runs"),
        other => panic!("expected LimitExceeded(text_runs), got {other:?}"),
    }

    // 2. Deep graphics state nesting (200 x 'q') -> limit_exceeded, zero panics.
    let q_bomb = "q ".repeat(200);
    let q_obj = format!("<< /Length {} >>\nstream\n{q_bomb}\nendstream", q_bomb.len()).into_bytes();
    let pdf_q = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] /Contents 4 0 R >>".to_vec()),
        (4, q_obj),
    ]);
    let err2 = extract_pdf_page(fake_sheet, &pdf_q, 0, true).expect_err("deep q nesting must fail");
    assert_eq!(err2.code(), "report.limit_exceeded");

    // 3. Hostile corpus seeds from IO.CORP never panic.
    let corp_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/corp/hostile");
    if let Ok(entries) = std::fs::read_dir(&corp_dir) {
        for e in entries.flatten() {
            if let Ok(raw) = std::fs::read(e.path()) {
                let _ = extract_pdf_page(fake_sheet, &raw, 0, true);
            }
        }
    }
}

// ------------------------------------------------------------------ 20-page performance check

#[test]
fn twenty_page_vector_drawing_extracts_under_1s() {
    let fake_sheet =
        SheetId::new(Uuid::from_bytes([0x01, 0x90, 0x00, 0x00, 0x00, 0x09, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 2]))
            .expect("uuidv7");

    let mut content = String::new();
    for i in 0..25 {
        content.push_str(&format!("BT /F1 10 Tf {} {} Td (DIM-{i} 12.00) Tj ET\n", 50 + i * 20, 100 + i * 15));
    }
    let stream_obj = format!("<< /Length {} >>\nstream\n{content}endstream", content.len()).into_bytes();

    let mut objs: Vec<(u32, Vec<u8>)> = vec![(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec())];
    let kids: Vec<String> = (4..24).map(|i| format!("{i} 0 R")).collect();
    objs.push((2, format!("<< /Type /Pages /Kids [{}] /Count 20 >>", kids.join(" ")).into_bytes()));
    objs.push((3, stream_obj));
    for i in 4..24 {
        objs.push((i, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1190 842] /Contents 3 0 R >>".to_vec()));
    }
    let pdf = build_pdf(&objs);

    let t0 = Instant::now();
    for p in 0..20 {
        let ex = extract_pdf_page(fake_sheet, &pdf, p, true).expect("page ok");
        assert_eq!(ex.map.runs.len(), 25);
    }
    assert!(t0.elapsed().as_millis() < 1000, "20-page extraction exceeded 1 s budget");
}
