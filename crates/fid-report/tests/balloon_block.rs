//! Acceptance tests for `RPT.BALLOON.BLOCK` (`AC-1`..`AC-6`, `REQ-001`..`REQ-007`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gdt::chars::dprof::CharId;
use fid_kdec::{NotDecidableReason, Outcome};
use fid_project::{Bits, IdError, ObservationSource, Uuid};
use fid_replay::ReplayOutcome;
use fid_report::balloon::block::{
    drawing_kind_id, register_drawing_kind, resolve, round_half_even_i128, set_drawing_balloon,
    set_drawing_balloon_def, Affine2Int, BalloonShape, BalloonStyle, BlockFrame, BlockPoint, DrawingBlockError,
    DrawingPayload, FitMode, SetDrawingBalloonParams, SheetRect,
};
use fid_report::balloon::register::{
    replay_exact as replay_register_exact, AddBalloonParams, BalloonRegister, RenumberBalloonParams, RenumberMode,
    SheetPoint,
};
use fid_report::balloon::sheet::{digest_bytes, DeterministicV7Gen, DrawingRevId, SheetId, SheetInfo, UuidV7Gen};
use fid_report::doc::{
    add_block, create_report, replay_exact as replay_doc_exact, AddBlock, CreateReport, KindRegistry, MemoryBackend,
    ReportBackend, Untrusted,
};
use fid_report::fmt::Catalog;
use fid_store::cas::BlobRef;
use std::collections::BTreeMap;
use std::time::Instant;

struct Counter(u64);

impl ObservationSource for Counter {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(1_760_000_000_000 + self.0)
    }

    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        let b = Bits::new(n, u128::from(self.0) + 1)?;
        self.0 += 1;
        Ok(b)
    }
}

/// Test `SheetInfo` implementing fixture `fixtures/rpt/balloon/ballooned_a3`:
/// one A3 landscape sheet of `42000 x 29700` units (`0.01 mm`) with a deterministic blank image ref.
struct BalloonedA3Sheets {
    sheet: SheetId,
    rev: DrawingRevId,
    width_cmm: i64,
    height_cmm: i64,
    image: BlobRef,
}

impl SheetInfo for BalloonedA3Sheets {
    fn bounds(&self, s: SheetId) -> Option<(i64, i64)> {
        (s == self.sheet).then_some((self.width_cmm, self.height_cmm))
    }

    fn revision(&self, s: SheetId) -> Option<DrawingRevId> {
        (s == self.sheet).then_some(self.rev)
    }

    fn image(&self, s: SheetId) -> Option<BlobRef> {
        (s == self.sheet).then_some(self.image)
    }

    fn ready(&self, s: SheetId) -> bool {
        s == self.sheet
    }
}

fn char_id(n: u32) -> CharId {
    CharId::parse(&format!("C{n}")).expect("char id")
}

struct A3Fixture {
    ids: DeterministicV7Gen,
    project_id: Uuid,
    sheets: BalloonedA3Sheets,
    register: BalloonRegister,
    outcomes: BTreeMap<CharId, Outcome>,
    kinds: KindRegistry,
    backend: MemoryBackend,
    report_id: fid_report::doc::ReportId,
    block_id: fid_report::doc::BlockId,
}

fn build_ballooned_a3() -> A3Fixture {
    let mut ids = DeterministicV7Gen::new(1_760_000_000_000);
    let project_id = ids.next_v7().unwrap();
    let rev = DrawingRevId::new(ids.next_v7().unwrap()).unwrap();
    let sheet = SheetId::new(ids.next_v7().unwrap()).unwrap();
    let image = BlobRef { digest: digest_bytes(b"synthetic-a3-blank-sheet"), size: 24 };
    let sheets = BalloonedA3Sheets { sheet, rev, width_cmm: 42_000, height_cmm: 29_700, image };

    // 10 balloons: 5 on left half (x <= 21000), 5 on right half (x > 21000).
    // All coordinates are even so dividing by 2 is exact (AC-1).
    let mut register = BalloonRegister::new();
    for i in 1..=10u32 {
        let x = if i <= 5 { i64::from(i) * 4_000 } else { 22_000 + i64::from(i - 6) * 4_000 };
        let y = 4_000 + i64::from(i) * 2_000;
        register
            .add_balloon(
                &sheets,
                &mut ids,
                &AddBalloonParams {
                    drawing_rev: rev,
                    sheet,
                    characteristic: char_id(i),
                    anchor: SheetPoint::new(x, y),
                    label: SheetPoint::new(x + 1_000, y - 1_000),
                    number: Some(i),
                    insert: false,
                    zone: Some(format!("B{i}")),
                },
            )
            .expect("add balloon");
    }

    let mut outcomes = BTreeMap::new();
    outcomes.insert(char_id(1), Outcome::Conforms);
    outcomes.insert(char_id(2), Outcome::Nonconforms);
    // C3 intentionally absent -> must be marked `not evaluated` (AC-3).
    outcomes.insert(char_id(4), Outcome::Indeterminate);
    outcomes.insert(char_id(5), Outcome::NotDecidable(NotDecidableReason::UMissing));

    let mut kinds = KindRegistry::new();
    register_drawing_kind(&mut kinds).expect("register drawing kind");

    let mut backend = MemoryBackend::new(project_id);
    let mut obs = Counter(1);
    let created = create_report(
        &mut backend,
        &kinds,
        &mut obs,
        &CreateReport { project: project_id, title: Untrusted::text("Ballooned A3 FAI").unwrap() },
    )
    .expect("create report");
    let report_id = created.id();
    let page_id = created.pages()[0].id();

    let init_payload =
        DrawingPayload { sheet, crop: None, fit: FitMode::ScaleToFit, outcome_marking: true, show_leaders: true }
            .to_value();

    let block_id = add_block(
        &mut backend,
        &kinds,
        &mut obs,
        &AddBlock {
            report: report_id.uuid(),
            page: page_id.uuid(),
            after: None,
            kind: Some(drawing_kind_id().unwrap()),
            payload: init_payload,
        },
    )
    .expect("add drawing block");

    A3Fixture { ids, project_id, sheets, register, outcomes, kinds, backend, report_id, block_id }
}

#[test]
fn ac1_ac2_ac3_ac6_exact_half_scale_live_renumber_outcomes_and_replay() {
    let mut fx = build_ballooned_a3();
    let catalog = Catalog::english();
    let style = BalloonStyle::default();
    let frame = BlockFrame::new(21_000, 14_850);

    // Read stored payload from the report document.
    let report = fx.backend.load(&fx.report_id).unwrap().unwrap().open(&fx.report_id, &fx.kinds).unwrap();
    let block_before = report.block(&fx.block_id.uuid()).unwrap().clone();
    let payload = DrawingPayload::from_canonical_json(block_before.payload()).unwrap();

    // AC-1: 42000 x 29700 sheet in 21000 x 14850 frame -> exact scale 1/2, zero offset, 10 glyphs in ascending order.
    let content = resolve(&payload, frame, &fx.register, &fx.sheets, &fx.outcomes, &catalog, &style).unwrap();
    assert_eq!(content.transform, Affine2Int { scale_num: 1, scale_den: 2, offset_x: 0, offset_y: 0 });
    assert_eq!(content.sheet_image, Some(fx.sheets.image));
    assert_eq!(content.glyphs.len(), 10);
    assert!(content.excluded.is_empty());

    let orig_balloons = fx.register.list(fx.sheets.rev);
    for (i, (b, g)) in orig_balloons.iter().zip(content.glyphs.iter()).enumerate() {
        assert_eq!(g.number, (i + 1) as u32);
        assert_eq!(g.anchor, BlockPoint::new(b.anchor.x / 2, b.anchor.y / 2));
        assert_eq!(g.label, BlockPoint::new(b.label.x / 2, b.label.y / 2));
        assert_eq!(g.leader, Some((g.anchor, g.label)));
    }

    // AC-3: Outcome marking on C1 CONFORMS, C2 NONCONFORMS, C3 missing -> not evaluated.
    let m1 = content.glyphs[0].outcome.as_ref().unwrap();
    assert_eq!(
        (m1.colour_role.as_str(), m1.symbol.as_str(), m1.label.as_str()),
        ("status.conforms", "state.conforms", "conforms")
    );
    let m2 = content.glyphs[1].outcome.as_ref().unwrap();
    assert_eq!(
        (m2.colour_role.as_str(), m2.symbol.as_str(), m2.label.as_str()),
        ("status.nonconforms", "state.nonconforms", "nonconforms")
    );
    let m3 = content.glyphs[2].outcome.as_ref().unwrap();
    assert_eq!(
        (m3.colour_role.as_str(), m3.symbol.as_str(), m3.label.as_str()),
        ("status.not-evaluated", "state.not-evaluated", "not evaluated")
    );
    let m4 = content.glyphs[3].outcome.as_ref().unwrap();
    assert_eq!(
        (m4.colour_role.as_str(), m4.symbol.as_str(), m4.label.as_str()),
        ("status.indeterminate", "state.indeterminate", "indeterminate")
    );
    let m5 = content.glyphs[4].outcome.as_ref().unwrap();
    assert_eq!(
        (m5.colour_role.as_str(), m5.symbol.as_str(), m5.label.as_str()),
        ("status.not-decidable", "state.not-decidable", "not decidable")
    );

    // AC-2: Renumber balloon 9 -> 11; resolve again WITHOUT editing block payload!
    let balloon_9 = orig_balloons[8].clone();
    fx.register
        .renumber_balloon(&RenumberBalloonParams {
            drawing_rev: fx.sheets.rev,
            mode: RenumberMode::Set { balloon: balloon_9.id, number: 11 },
        })
        .expect("renumber 9 -> 11");

    let report_after = fx.backend.load(&fx.report_id).unwrap().unwrap().open(&fx.report_id, &fx.kinds).unwrap();
    let block_after = report_after.block(&fx.block_id.uuid()).unwrap();
    assert_eq!(block_after.payload(), block_before.payload(), "block payload must stay unchanged");

    let content2 = resolve(&payload, frame, &fx.register, &fx.sheets, &fx.outcomes, &catalog, &style).unwrap();
    let numbers: Vec<u32> = content2.glyphs.iter().map(|g| g.number).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5, 6, 7, 8, 10, 11]);
    let glyph_11 = content2.glyphs.last().unwrap();
    assert_eq!(glyph_11.anchor, BlockPoint::new(balloon_9.anchor.x / 2, balloon_9.anchor.y / 2));

    // Also update block via `report.set_drawing_balloon@1` (W2) and verify AC-6 exact replay.
    let jlen_before = fx.backend.journal().len();
    let dry = set_drawing_balloon(
        &mut fx.backend,
        &fx.kinds,
        &fx.sheets,
        &SetDrawingBalloonParams {
            report: fx.report_id,
            block: fx.block_id,
            sheet: fx.sheets.sheet,
            crop: None,
            fit: FitMode::ScaleToFit,
            outcome_marking: true,
            show_leaders: false,
            dry_run: true,
        },
    )
    .expect("dry run");
    assert!(!DrawingPayload::from_canonical_json(dry.payload()).unwrap().show_leaders);
    assert_eq!(fx.backend.journal().len(), jlen_before, "dry_run must journal nothing");

    let committed = set_drawing_balloon(
        &mut fx.backend,
        &fx.kinds,
        &fx.sheets,
        &SetDrawingBalloonParams {
            report: fx.report_id,
            block: fx.block_id,
            sheet: fx.sheets.sheet,
            crop: None,
            fit: FitMode::ScaleToFit,
            outcome_marking: true,
            show_leaders: false,
            dry_run: false,
        },
    )
    .expect("commit set_drawing_balloon");
    assert_eq!(fx.backend.journal().len(), jlen_before + 1);
    let payload_committed = DrawingPayload::from_canonical_json(committed.payload()).unwrap();
    let expected_content =
        resolve(&payload_committed, frame, &fx.register, &fx.sheets, &fx.outcomes, &catalog, &style).unwrap();
    let expected_digest = expected_content.canonical_digest().unwrap();

    // AC-6: Exact replay of BOTH the report document journal and the balloon register journal -> IDENTICAL + byte-identical digest!
    let doc_run = replay_doc_exact(&fx.backend.journal_values(), &fx.kinds, fx.project_id);
    assert_eq!(doc_run.outcome, ReplayOutcome::Identical);
    let reg_run = replay_register_exact(fx.register.journal(), &fx.sheets);
    assert_eq!(reg_run.outcome, ReplayOutcome::Identical);

    let (replayed_report, _) = doc_run.reports.get(&fx.report_id).unwrap();
    let replayed_payload =
        DrawingPayload::from_canonical_json(replayed_report.block(&fx.block_id.uuid()).unwrap().payload()).unwrap();
    let replayed_content =
        resolve(&replayed_payload, frame, &reg_run.register, &fx.sheets, &fx.outcomes, &catalog, &style).unwrap();
    assert_eq!(replayed_content.canonical_digest().unwrap(), expected_digest);
    assert!(replayed_content.glyphs.iter().all(|g| g.leader.is_none()));
}

#[test]
fn ac4_crop_region_includes_left_half_and_lists_excluded_numbers() {
    let fx = build_ballooned_a3();
    let catalog = Catalog::english();
    let style = BalloonStyle::default();
    // Crop to left half: x in [0..=21000], y in [0..=29700].
    let payload = DrawingPayload {
        sheet: fx.sheets.sheet,
        crop: Some(SheetRect::new(0, 0, 21_000, 29_700)),
        fit: FitMode::ScaleToFit,
        outcome_marking: false,
        show_leaders: true,
    };
    // Frame is wider than cropped aspect (30000 x 14850): height-constrained -> scale = 14850 / 29700 = 1/2,
    // scaled crop width = 10500, centred offset_x = (30000 - 10500) / 2 = 9750, offset_y = 0 (REQ-007)!
    let content =
        resolve(&payload, BlockFrame::new(30_000, 14_850), &fx.register, &fx.sheets, &fx.outcomes, &catalog, &style)
            .unwrap();

    assert_eq!(content.transform, Affine2Int { scale_num: 1, scale_den: 2, offset_x: 9_750, offset_y: 0 });
    let included: Vec<u32> = content.glyphs.iter().map(|g| g.number).collect();
    assert_eq!(included, vec![1, 2, 3, 4, 5]);
    assert_eq!(content.excluded, vec![6, 7, 8, 9, 10]);
    assert!(content.glyphs.iter().all(|g| g.outcome.is_none()));
    // Balloon 1 had anchor (4000, 6000) -> (4000/2 + 9750, 6000/2) = (11750, 3000).
    assert_eq!(content.glyphs[0].anchor, BlockPoint::new(11_750, 3_000));
}

#[test]
fn ac5_unknown_sheet_and_invalid_crop_fail_with_invalid_block_payload_and_journal_nothing() {
    let mut fx = build_ballooned_a3();
    let jlen = fx.backend.journal().len();
    let unknown_sheet = SheetId::new(fx.ids.next_v7().unwrap()).unwrap();

    // AC-5: unknown sheet fails with report.invalid_block_payload and leaves journal unchanged.
    let err = set_drawing_balloon(
        &mut fx.backend,
        &fx.kinds,
        &fx.sheets,
        &SetDrawingBalloonParams {
            report: fx.report_id,
            block: fx.block_id,
            sheet: unknown_sheet,
            crop: None,
            fit: FitMode::ScaleToFit,
            outcome_marking: true,
            show_leaders: true,
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "report.invalid_block_payload");
    assert!(matches!(err, DrawingBlockError::InvalidBlockPayload { field: "sheet", .. }));
    assert_eq!(fx.backend.journal().len(), jlen);

    // Crop outside sheet bounds -> report.invalid_block_payload { field: "crop" }, journal unchanged.
    let err_crop = set_drawing_balloon(
        &mut fx.backend,
        &fx.kinds,
        &fx.sheets,
        &SetDrawingBalloonParams {
            report: fx.report_id,
            block: fx.block_id,
            sheet: fx.sheets.sheet,
            crop: Some(SheetRect::new(40_000, 0, 5_000, 10_000)),
            fit: FitMode::ScaleToFit,
            outcome_marking: false,
            show_leaders: false,
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(err_crop.code(), "report.invalid_block_payload");
    assert!(matches!(err_crop, DrawingBlockError::InvalidBlockPayload { field: "crop", .. }));
    assert_eq!(fx.backend.journal().len(), jlen);

    // Empty crop -> report.invalid_block_payload { field: "crop" }, journal unchanged.
    let err_empty = set_drawing_balloon(
        &mut fx.backend,
        &fx.kinds,
        &fx.sheets,
        &SetDrawingBalloonParams {
            report: fx.report_id,
            block: fx.block_id,
            sheet: fx.sheets.sheet,
            crop: Some(SheetRect::new(100, 100, 0, 100)),
            fit: FitMode::ScaleToFit,
            outcome_marking: false,
            show_leaders: false,
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(err_empty.code(), "report.invalid_block_payload");
    assert_eq!(fx.backend.journal().len(), jlen);

    // Unknown block -> report.not_found.
    let fake_block = fid_report::doc::BlockId::new(Uuid::from_bytes([
        0x01, 0x90, 0x00, 0x00, 0x00, 0x09, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, 99,
    ]))
    .unwrap();
    let err_nf = set_drawing_balloon(
        &mut fx.backend,
        &fx.kinds,
        &fx.sheets,
        &SetDrawingBalloonParams {
            report: fx.report_id,
            block: fake_block,
            sheet: fx.sheets.sheet,
            crop: None,
            fit: FitMode::ScaleToFit,
            outcome_marking: false,
            show_leaders: false,
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(err_nf.code(), "report.not_found");
    assert_eq!(fx.backend.journal().len(), jlen);
}

#[test]
fn half_even_rounding_style_loader_command_def_and_perf_2000_balloons() {
    // IEEE round-half-to-even checks (REQ-001).
    assert_eq!(round_half_even_i128(1, 2).unwrap(), 0);
    assert_eq!(round_half_even_i128(3, 2).unwrap(), 2);
    assert_eq!(round_half_even_i128(5, 2).unwrap(), 2);
    assert_eq!(round_half_even_i128(7, 2).unwrap(), 4);
    assert_eq!(round_half_even_i128(-3, 2).unwrap(), -2);
    assert_eq!(round_half_even_i128(-5, 2).unwrap(), -2);

    // UX.REPORT balloon style JSON loader.
    let custom = BalloonStyle::from_json_bytes(
        br#"{"shape":"pill","radius_cmm":300,"number_size_cmm":240,"leader_width_cmm":30}"#,
    )
    .unwrap();
    assert_eq!(
        custom,
        BalloonStyle { shape: BalloonShape::Pill, radius_cmm: 300, number_size_cmm: 240, leader_width_cmm: 30 }
    );

    // Validated CommandDef.
    let def = set_drawing_balloon_def().expect("valid CommandDef");
    assert_eq!(def.id, "report.set_drawing_balloon");
    assert_eq!(def.major, 1);
    assert_eq!(def.side_effect, fid_types::SideEffect::W2);
    assert_eq!(def.determinism, fid_types::Determinism::D0);
    assert_eq!(def.id.replace('.', "_"), "report_set_drawing_balloon");

    // Performance budget: resolve 2000 balloons cleanly.
    let mut fx = build_ballooned_a3();
    for i in 11..=2000u32 {
        let x = 500 + i64::from(i % 400) * 100;
        let y = 500 + i64::from(i / 400) * 500;
        fx.register
            .add_balloon(
                &fx.sheets,
                &mut fx.ids,
                &AddBalloonParams {
                    drawing_rev: fx.sheets.rev,
                    sheet: fx.sheets.sheet,
                    characteristic: char_id(i),
                    anchor: SheetPoint::new(x, y),
                    label: SheetPoint::new(x + 200, y + 200),
                    number: Some(i),
                    insert: false,
                    zone: None,
                },
            )
            .expect("add perf balloon");
    }
    let payload = DrawingPayload {
        sheet: fx.sheets.sheet,
        crop: None,
        fit: FitMode::ScaleToFit,
        outcome_marking: true,
        show_leaders: true,
    };
    let catalog = Catalog::english();
    let style = BalloonStyle::default();
    let t0 = Instant::now();
    let content =
        resolve(&payload, BlockFrame::new(21_000, 14_850), &fx.register, &fx.sheets, &fx.outcomes, &catalog, &style)
            .unwrap();
    let elapsed = t0.elapsed();
    assert_eq!(content.glyphs.len(), 2000);
    assert!(elapsed.as_millis() < 1_000, "resolving 2000 balloons took {} ms", elapsed.as_millis());
}
