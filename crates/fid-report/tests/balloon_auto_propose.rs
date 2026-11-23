//! Acceptance tests for `RPT.BALLOON.AUTO.PROPOSE` (`fid_report::balloon::auto_propose`, AC-1..AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Instant;

use fid_canon::CValue;
use fid_gdt::chars::dprof::DprofState;
use fid_gdt::chars::spec::{PlanId, SpecState};
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_report::balloon::auto_propose::{
    command_def, replay_propose_exact, AdoptDecision, OcrTextRun, ProposalBranchStore, ProposalOrigin,
    ProposeBalloonsParams, ZoneGridSpec, DIAG_RASTER_NEEDS_OCR,
};
use fid_report::balloon::auto_text::SheetRect;
use fid_report::balloon::char::{
    make_char_input_from_literals, replay_exact as replay_char_exact, AuthoredUnit, CharBalloonStore, Dialect,
};
use fid_report::balloon::register::BalloonRegister;
use fid_report::balloon::sheet::{
    add_drawing_balloon, AddDrawingParams, DeterministicV7Gen, DrawingRevId, DrawingStore, ImportEnv,
    MemoryDrawingBlobs, RevisionId, SheetId,
};

fn auto_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/balloon/auto")
}

fn part_rev_id(seq: u8) -> RevisionId {
    let bytes = [0x01, 0x90, 0x00, 0x00, 0x00, 0x30, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, seq];
    RevisionId::new(Uuid::from_bytes(bytes)).expect("v7")
}

fn import_pdf(
    store: &mut DrawingStore,
    blobs: &mut MemoryDrawingBlobs,
    ids: &mut DeterministicV7Gen,
    filename: &str,
    dn: &str,
) -> (DrawingRevId, SheetId) {
    let mut roots = RootSet::new();
    roots.insert(RootId::new("auto").expect("root"), RootAccess::Import, auto_fixtures_dir());
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
    .expect("import pdf");
    (res.drawing_rev.id, res.sheets[0].id)
}

// ------------------------------------------------------------------ AC-1, AC-2, AC-4

#[test]
fn ac1_ac2_ac4_propose_15_balloons_h2_human_adopt_agent_refused_and_replay() {
    let mut drawing_store = DrawingStore::new();
    let mut blobs = MemoryDrawingBlobs::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_800_000);
    let (drawing_rev, _sheet_id) = import_pdf(&mut drawing_store, &mut blobs, &mut ids, "vector_bracket.pdf", "BRK-P1");

    let plan = PlanId::parse("PLAN-BRK").expect("plan");
    let mut spec = SpecState::new();
    let mut dprof = DprofState::new();
    let mut register = BalloonRegister::new();
    let mut char_store = CharBalloonStore::new();
    let mut proposals = ProposalBranchStore::new();

    let params = ProposeBalloonsParams {
        plan: plan.clone(),
        drawing_rev,
        sheets: None,
        dialect: Some(Dialect::Asme),
        default_unit: Some(AuthoredUnit::Mm),
        zone_grid: ZoneGridSpec::default(),
        ocr_candidates: Vec::new(),
        branch: "agent/balloons-brk".into(),
        dry_run: false,
    };

    // AC-1: W1 proposal creates 15 numbered proposals with non-overlapping labels, working doc unchanged.
    let t0 = Instant::now();
    let batch1 = proposals.propose_balloons(&drawing_store, &blobs, &register, &params).expect("propose_balloons ok");
    assert!(t0.elapsed().as_millis() < 200, "propose took {} ms", t0.elapsed().as_millis());
    assert_eq!(batch1.proposals.len(), 15);
    assert!(batch1.diagnostics.is_empty());
    // Working document untouched (REQ-001)!
    assert!(register.list(drawing_rev).is_empty());
    assert_eq!(char_store.journal().len(), 0);

    // Verify all 15 provisional numbers are 1..=15 and all label positions are distinct / non-overlapping.
    for (i, pr) in batch1.proposals.iter().enumerate() {
        assert_eq!(pr.provisional_number, (i as u32) + 1);
        assert_eq!(pr.origin, ProposalOrigin::VectorParse);
        assert!(pr.zone.is_some());
        assert!(pr.char_input.is_some());
        for other in &batch1.proposals[i + 1..] {
            let dx = i128::from(pr.label.x - other.label.x);
            let dy = i128::from(pr.label.y - other.label.y);
            assert!(dx * dx + dy * dy >= 800 * 800, "labels overlap: {:?} vs {:?}", pr.label, other.label);
        }
    }

    // D0 determinism: running twice yields byte-identical batch and branch digest (REQ-006).
    let mut proposals2 = ProposalBranchStore::new();
    let batch2 = proposals2.propose_balloons(&drawing_store, &blobs, &register, &params).expect("propose 2");
    assert_eq!(batch1, batch2);
    assert_eq!(batch1.branch_digest, batch2.branch_digest);

    // AC-2 (part 1): Agent principal attempting H2 adoption is REFUSED (core.human_only), working doc untouched!
    let mut decisions: Vec<AdoptDecision> = (1..=15u32)
        .map(|n| AdoptDecision {
            provisional_number: n,
            edited_char_input: None,
            edited_label: None,
            edited_zone: None,
        })
        .collect();
    // Edit proposal #1 before adoption (AC-2: "adopts 14 accepted proposals and edits 1").
    decisions[0].edited_char_input = Some(
        make_char_input_from_literals("linear", "120.00mm", "+0.05mm", "-0.05mm", Some("ASME"), &[], Some("Edited"))
            .expect("edited input"),
    );

    let err_agent = proposals
        .adopt_proposal_batch(
            "agent/balloons-brk",
            Principal::Agent,
            &decisions,
            &mut char_store,
            &mut spec,
            &mut dprof,
            &mut register,
            &drawing_store,
            &mut ids,
        )
        .expect_err("agent H2 adoption must be refused");
    assert_eq!(err_agent.code(), "core.human_only");
    assert!(register.list(drawing_rev).is_empty());
    assert_eq!(char_store.journal().len(), 0);

    // AC-2 (part 2): Human principal adopts all 15 (14 accepted + 1 edited) in one H2 batch.
    let adopted = proposals
        .adopt_proposal_batch(
            "agent/balloons-brk",
            Principal::Human,
            &decisions,
            &mut char_store,
            &mut spec,
            &mut dprof,
            &mut register,
            &drawing_store,
            &mut ids,
        )
        .expect("human H2 adoption ok");
    assert_eq!(adopted.adopted.len(), 15);
    assert_eq!(register.list(drawing_rev).len(), 15);
    assert_eq!(char_store.journal().len(), 15);
    // Edited proposal #1 preserved its edited verbatim nominal "120.00 mm"!
    let c1 = char_store.get(&adopted.adopted[0].characteristic_id).expect("char 1");
    assert_eq!(c1.nominal.authored_literal(), "120.00 mm");

    // AC-4: Exact replay of BOTH W1 proposal batch and W2 adopted journal yields IDENTICAL!
    let empty_reg = BalloonRegister::new();
    assert_eq!(replay_propose_exact(&batch1, &params, &drawing_store, &blobs, &empty_reg), ReplayOutcome::Identical);
    let rep_adopt = replay_char_exact(char_store.journal(), &drawing_store);
    assert_eq!(rep_adopt.outcome, ReplayOutcome::Identical);
    assert_eq!(rep_adopt.register.list(drawing_rev).len(), 15);

    // Registry CommandDef validates cleanly.
    let def = command_def().expect("command_def valid");
    assert_eq!(def.id.as_str(), "report.propose_balloons");
}

// ------------------------------------------------------------------ AC-3 (scanned_only + OCR candidates + off-sheet)

#[test]
fn ac3_scanned_only_needs_ocr_and_accepts_validated_ocr_candidates() {
    let mut drawing_store = DrawingStore::new();
    let mut blobs = MemoryDrawingBlobs::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_900_000);
    let (drawing_rev, sheet_id) = import_pdf(&mut drawing_store, &mut blobs, &mut ids, "scanned_only.pdf", "SCAN-P1");

    let plan = PlanId::parse("PLAN-SCAN").expect("plan");
    let register = BalloonRegister::new();
    let mut proposals = ProposalBranchStore::new();

    // 1. Zero OCR candidates on scanned_only.pdf -> proposals: [] + diagnostic report.raster_sheet_needs_ocr.
    let res_empty = proposals
        .propose_balloons(
            &drawing_store,
            &blobs,
            &register,
            &ProposeBalloonsParams {
                plan: plan.clone(),
                drawing_rev,
                sheets: None,
                dialect: Some(Dialect::Asme),
                default_unit: Some(AuthoredUnit::Mm),
                zone_grid: ZoneGridSpec::default(),
                ocr_candidates: Vec::new(),
                branch: "agent/scan-empty".into(),
                dry_run: false,
            },
        )
        .expect("empty raster propose ok");
    assert!(res_empty.proposals.is_empty());
    assert!(res_empty.diagnostics.iter().any(|d| d.code == DIAG_RASTER_NEEDS_OCR));

    // 2. Load fixtures/rpt/balloon/auto/ocr_candidates_5.json -> 5 ocr_candidate proposals staged!
    let raw = std::fs::read(auto_fixtures_dir().join("ocr_candidates_5.json")).expect("read ocr json");
    let val = fid_canon::parse(&raw).expect("parse ocr json");
    let CValue::Obj(m) = &val else { panic!("ocr root") };
    let CValue::Arr(cands_val) = m.get("candidates").expect("candidates") else { panic!("cands") };
    let mut ocrs = Vec::new();
    for cv in cands_val {
        let CValue::Obj(cm) = cv else { panic!("cand obj") };
        let CValue::Str(text) = cm.get("text").expect("text") else { panic!("text") };
        let CValue::Obj(bm) = cm.get("bbox").expect("bbox") else { panic!("bbox") };
        let gn = |k: &str| match bm.get(k) {
            Some(CValue::Num(f)) => *f as i64,
            _ => panic!("missing {k}"),
        };
        ocrs.push(OcrTextRun {
            sheet: sheet_id,
            text: text.clone(),
            bbox: SheetRect::new(gn("x0"), gn("y0"), gn("x1"), gn("y1")),
            font_h_cmm: 350,
        });
    }
    assert_eq!(ocrs.len(), 5);

    let res_ocr = proposals
        .propose_balloons(
            &drawing_store,
            &blobs,
            &register,
            &ProposeBalloonsParams {
                plan: plan.clone(),
                drawing_rev,
                sheets: None,
                dialect: Some(Dialect::Asme),
                default_unit: Some(AuthoredUnit::Mm),
                zone_grid: ZoneGridSpec::default(),
                ocr_candidates: ocrs,
                branch: "agent/scan-ocr".into(),
                dry_run: false,
            },
        )
        .expect("ocr propose ok");
    assert_eq!(res_ocr.proposals.len(), 5);
    assert!(res_ocr.proposals.iter().all(|p| p.origin == ProposalOrigin::OcrCandidate));

    // 3. Out-of-bounds OCR box -> report.balloon_off_sheet (REQ-004).
    let err_off = proposals
        .propose_balloons(
            &drawing_store,
            &blobs,
            &register,
            &ProposeBalloonsParams {
                plan,
                drawing_rev,
                sheets: None,
                dialect: Some(Dialect::Asme),
                default_unit: Some(AuthoredUnit::Mm),
                zone_grid: ZoneGridSpec::default(),
                ocr_candidates: vec![OcrTextRun {
                    sheet: sheet_id,
                    text: "Ø10.00 ±0.05".into(),
                    bbox: SheetRect::new(20_000, 1000, 25_000, 1350), // x1 > 21000 A4 width
                    font_h_cmm: 350,
                }],
                branch: "agent/scan-bad".into(),
                dry_run: false,
            },
        )
        .expect_err("off-sheet OCR must fail");
    assert_eq!(err_off.code(), "report.balloon_off_sheet");
}
