//! Acceptance and performance tests for `RPT.BALLOON.AUTO.PARSE` (`fid_report::balloon::auto_parse`, AC-1..AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Instant;

use fid_canon::{to_canonical_bytes, CValue};
use fid_project::Uuid;
use fid_report::balloon::auto_parse::{
    parse_callout_text, parse_drawing_text_map, CalloutKind, ConfidenceWord, ReasonCode, UnitHint,
};
use fid_report::balloon::auto_text::{extract_pdf_page, DrawingTextMap, PageKind, SheetRect, TextRun};
use fid_report::balloon::sheet::SheetId;
use fid_report::doc::text::Untrusted;

fn auto_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/balloon/auto")
}

fn dummy_sheet() -> SheetId {
    SheetId::new(Uuid::from_bytes([0x01, 0x90, 0x00, 0x00, 0x00, 0x03, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 1]))
        .expect("uuidv7")
}

fn make_run(id: u32, text: &str, x0: i64, y0: i64, x1: i64, y1: i64) -> TextRun {
    TextRun {
        id,
        text: Untrusted::text(text).expect("valid text"),
        bbox: SheetRect::new(x0, y0, x1, y1),
        font_h_cmm: (y1 - y0).max(200),
        angle_mdeg: 0,
    }
}

// ------------------------------------------------------------------ AC-1 & AC-4

#[test]
fn ac1_and_ac4_100_callout_corpus_and_title_block_distractors() {
    let raw = std::fs::read(auto_fixtures_dir().join("callouts_corpus.json")).expect("read corpus");
    let root = fid_canon::parse(&raw).expect("parse corpus json");
    let CValue::Obj(rm) = &root else { panic!("corpus not object") };
    let CValue::Arr(cases) = rm.get("callouts").expect("callouts") else { panic!("callouts array") };
    let CValue::Arr(distractors) = rm.get("distractors").expect("distractors") else { panic!("distractors array") };
    assert_eq!(cases.len(), 100);

    let mut exact_matches = 0usize;
    let mut parsed_count = 0usize;

    for (idx, case) in cases.iter().enumerate() {
        let CValue::Obj(cm) = case else { panic!("case not object") };
        let req_s = |k: &str| match cm.get(k) {
            Some(CValue::Str(s)) => s.as_str(),
            _ => panic!("missing str {k}"),
        };
        let opt_s = |k: &str| match cm.get(k) {
            Some(CValue::Str(s)) => Some(s.as_str()),
            Some(CValue::Null) | None => None,
            _ => panic!("bad opt str {k}"),
        };
        let want_raw = req_s("raw");
        let want_kind = req_s("kind");
        let want_nom = opt_s("nominal");
        let want_up = opt_s("upper");
        let want_lo = opt_s("lower");
        let want_gdt = opt_s("gdt_symbol");
        let want_rep = match cm.get("repeat_count") {
            Some(CValue::Num(f)) => *f as u32,
            _ => 1,
        };
        let want_datums: Vec<&str> = match cm.get("datum_refs") {
            Some(CValue::Arr(a)) => a
                .iter()
                .map(|v| match v {
                    CValue::Str(s) => s.as_str(),
                    _ => panic!("datum str"),
                })
                .collect(),
            _ => Vec::new(),
        };
        let want_mods: Vec<&str> = match cm.get("modifiers") {
            Some(CValue::Arr(a)) => a
                .iter()
                .map(|v| match v {
                    CValue::Str(s) => s.as_str(),
                    _ => panic!("mod str"),
                })
                .collect(),
            _ => Vec::new(),
        };

        let y = ((idx as i64) + 1) * 1000;
        let bbox = SheetRect::new(1000, y, 5000, y + 350);
        if let Some(p) = parse_callout_text(want_raw, bbox, vec![(idx as u32) + 1]) {
            parsed_count += 1;
            let got_datums: Vec<&str> = p.datum_refs.iter().map(|d| d.untrusted_value().as_str()).collect();
            let got_mods: Vec<&str> = p.modifiers.iter().map(|m| m.as_str()).collect();
            let ok = p.kind.as_str() == want_kind
                && p.repeat_count == want_rep
                && p.nominal.as_ref().map(|d| d.as_str()) == want_nom
                && p.upper.as_ref().map(|d| d.as_str()) == want_up
                && p.lower.as_ref().map(|d| d.as_str()) == want_lo
                && p.kind.gdt_symbol().map(|s| s.as_str()) == want_gdt
                && got_datums == want_datums
                && got_mods == want_mods;
            assert!(
                ok,
                "case {} ({want_raw:?}) mismatch:\n  got: kind={} rep={} nom={:?} up={:?} lo={:?} gdt={:?} datums={:?} mods={:?}\n want: kind={want_kind} rep={want_rep} nom={want_nom:?} up={want_up:?} lo={want_lo:?} gdt={want_gdt:?} datums={want_datums:?} mods={want_mods:?}",
                idx + 1,
                p.kind.as_str(),
                p.repeat_count,
                p.nominal,
                p.upper,
                p.lower,
                p.kind.gdt_symbol(),
                got_datums,
                got_mods
            );
            exact_matches += 1;
        } else {
            panic!("case {} ({want_raw:?}) failed to parse", idx + 1);
        }
    }

    assert_eq!(parsed_count, 100);
    assert!(exact_matches >= 95, "precision/recall below 95%: {exact_matches}/100");
    assert_eq!(exact_matches, 100, "all 100 corpus callouts should match verbatim");

    // AC-4: Every title-block distractor is excluded from characteristic candidates, and unit hint is detected.
    let mut dist_runs = Vec::new();
    for (i, d) in distractors.iter().enumerate() {
        let CValue::Str(s) = d else { panic!("distractor str") };
        let y = ((i as i64) + 1) * 1500;
        dist_runs.push(make_run((i as u32) + 1, s, 2000, y, 12000, y + 300));
    }
    let map = DrawingTextMap {
        sheet: dummy_sheet(),
        page_kind: PageKind::Vector,
        runs: dist_runs,
        frame_boxes: Vec::new(),
        warnings: Vec::new(),
    };
    let parsed = parse_drawing_text_map(&map);
    assert!(parsed.callouts.is_empty(), "title-block distractors must not become callouts: {:?}", parsed.callouts);
    assert_eq!(parsed.excluded_run_ids.len(), distractors.len());
    assert_eq!(parsed.title_block_hints.unit_hint, Some(UnitHint::Mm));
}

// ------------------------------------------------------------------ AC-2 (stacked bilateral tolerances)

#[test]
fn ac2_stacked_bilateral_tolerances_merge_with_exact_trailing_zeros_and_union_rect() {
    let map = DrawingTextMap {
        sheet: dummy_sheet(),
        page_kind: PageKind::Vector,
        runs: vec![
            make_run(1, "12.000", 5000, 4000, 6600, 4350),
            make_run(2, "+0.018", 6750, 3850, 7950, 4120),
            make_run(3, "-0.000", 6750, 4200, 7950, 4470),
        ],
        frame_boxes: Vec::new(),
        warnings: Vec::new(),
    };

    let res = parse_drawing_text_map(&map);
    assert_eq!(res.callouts.len(), 1);
    let c = &res.callouts[0];
    assert_eq!(c.kind, CalloutKind::Linear);
    assert_eq!(c.nominal.as_ref().map(|d| d.as_str()), Some("12.000"));
    assert_eq!(c.upper.as_ref().map(|d| d.as_str()), Some("+0.018"));
    assert_eq!(c.lower.as_ref().map(|d| d.as_str()), Some("-0.000"));
    assert_eq!(c.bbox, SheetRect::new(5000, 3850, 7950, 4470));
    assert_eq!(c.run_ids, vec![1, 2, 3]);
    assert!(c.reasons.contains(&ReasonCode::StackedClusterMerged));
}

// ------------------------------------------------------------------ AC-3 (ambiguous/occluded callout)

#[test]
fn ac3_ambiguous_or_occluded_callout_emits_low_confidence_without_fabricating_tolerance() {
    let bbox = SheetRect::new(1000, 1000, 4000, 1350);
    let c = parse_callout_text("12.00 ±???", bbox, vec![7]).expect("should emit low-conf candidate");
    assert_eq!(c.confidence, ConfidenceWord::Low);
    assert!(c.reasons.contains(&ReasonCode::AmbiguousTolerance));
    assert_eq!(c.nominal.as_ref().map(|d| d.as_str()), Some("12.00"));
    assert_eq!(c.upper, None, "must never fabricate upper tolerance");
    assert_eq!(c.lower, None, "must never fabricate lower tolerance");

    let partial_fcf = parse_callout_text("⌖ | ??? | A", bbox, vec![8]).expect("partial FCF candidate");
    assert_eq!(partial_fcf.confidence, ConfidenceWord::Low);
    assert!(partial_fcf.reasons.contains(&ReasonCode::PartialGdtFrame));
    assert_eq!(partial_fcf.nominal, None);
}

// ------------------------------------------------------------------ End-to-end vector_bracket.pdf + D0 + 500-run perf

#[test]
fn end_to_end_vector_bracket_pdf_and_determinism_and_500_run_budget() {
    let pdf_bytes = std::fs::read(auto_fixtures_dir().join("vector_bracket.pdf")).expect("read vector_bracket.pdf");
    let extracted = extract_pdf_page(dummy_sheet(), &pdf_bytes, 0, true).expect("extract pdf");
    let parsed1 = parse_drawing_text_map(&extracted.map);
    let parsed2 = parse_drawing_text_map(&extracted.map);

    assert_eq!(parsed1, parsed2);
    assert_eq!(parsed1.callouts.len(), 15, "all 15 callouts on vector_bracket.pdf should parse");

    let json1 =
        to_canonical_bytes(&CValue::Arr(parsed1.callouts.iter().map(|c| c.to_value()).collect())).expect("cjson1");
    let json2 =
        to_canonical_bytes(&CValue::Arr(parsed2.callouts.iter().map(|c| c.to_value()).collect())).expect("cjson2");
    assert_eq!(json1, json2);

    // 500 text runs clustered and parsed within reasonable budget.
    let mut runs = Vec::with_capacity(500);
    for i in 0..500u32 {
        let col = i64::from(i % 10);
        let row = i64::from(i / 10);
        let x0 = 1000 + col * 4000;
        let y0 = 1000 + row * 1200;
        runs.push(make_run(i + 1, "Ø12.00 ±0.05", x0, y0, x0 + 2500, y0 + 350));
    }
    let big_map = DrawingTextMap {
        sheet: dummy_sheet(),
        page_kind: PageKind::Vector,
        runs,
        frame_boxes: Vec::new(),
        warnings: Vec::new(),
    };
    let t0 = Instant::now();
    let big_res = parse_drawing_text_map(&big_map);
    let elapsed = t0.elapsed();
    assert_eq!(big_res.callouts.len(), 500);
    assert!(elapsed.as_millis() < 250, "500 runs took {} ms", elapsed.as_millis());
}
