//! Acceptance tests for `RPT.BALLOON.SHEET.CARRY` (`AC-1`..`AC-4`, `REQ-001`..`REQ-005`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gdt::chars::dprof::CharId;
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_replay::ReplayOutcome;
use fid_report::balloon::register::{AddBalloonParams, BalloonRegister, RegisterBalloonSink, SheetPoint};
use fid_report::balloon::sheet::{
    add_drawing_balloon, carry_command_def, carry_drawing_balloon, replay_exact_with_sink, AddDrawingParams, BalloonId,
    BalloonSink, BalloonView, CarryDrawingParams, CarryError, CarryPoint, DeterministicV7Gen, DrawingEvent,
    DrawingRevId, DrawingStore, ImportEnv, MemoryDrawingBlobs, NotCarriedReason, RevisionId, SheetId, SinkError,
    UuidV7Gen,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid_balloon_carry_{tag}_{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Builds a minimal valid multi-page blank PDF 1.7 with exact integer MediaBox sizes in points.
fn build_blank_pdf(pages_pt: &[(u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets = Vec::new();
    let n = pages_pt.len();
    let kids: String = (0..n).map(|i| format!("{} 0 R", 3 + i)).collect::<Vec<_>>().join(" ");
    let mut objects: Vec<String> = vec![
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".into(),
        format!("2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {n} >>\nendobj\n"),
    ];
    for (i, &(w, h)) in pages_pt.iter().enumerate() {
        objects.push(format!("{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] >>\nendobj\n", 3 + i));
    }
    for obj in &objects {
        offsets.push(out.len());
        out.extend_from_slice(obj.as_bytes());
    }
    let xref_pos = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n", objects.len() + 1).as_bytes(),
    );
    out
}

/// Builds a minimal valid 4x4 grayscale PNG with no pHYs chunk (`needs_size`).
fn build_nodpi_png() -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if (c & 1) != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let mut crc_in = Vec::with_capacity(4 + data.len());
        crc_in.extend_from_slice(tag);
        crc_in.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
        out
    }
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&4u32.to_be_bytes());
    ihdr.extend_from_slice(&4u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    png.extend_from_slice(&chunk(b"IHDR", &ihdr));
    let mut raw = Vec::new();
    for _ in 0..4 {
        raw.push(0);
        raw.extend_from_slice(&[0xFF; 4]);
    }
    let mut zlib = vec![0x78, 0x01, 0x01];
    let len = raw.len() as u16;
    zlib.extend_from_slice(&len.to_le_bytes());
    zlib.extend_from_slice(&(!len).to_le_bytes());
    zlib.extend_from_slice(&raw);
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in &raw {
        s1 = (s1 + u32::from(b)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    zlib.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    png.extend_from_slice(&chunk(b"IDAT", &zlib));
    png.extend_from_slice(&chunk(b"IEND", &[]));
    png
}

fn setup_roots(dir: &TempDir) -> RootSet {
    let mut roots = RootSet::new();
    roots.insert(RootId::new("import-draw").expect("root id"), RootAccess::Import, dir.0.clone());
    roots
}

fn char_id(n: u32) -> CharId {
    CharId::parse(&format!("char-{n:04}")).expect("char id")
}

/// Fixture `carry_two_revs`: Revision B (2 A3 sheets, 10 balloons: 5 on sheet 1, 5 on sheet 2)
/// and Revision C (2 blank sheets of configurable point sizes, 0 balloons).
struct CarryFixture {
    _dir: TempDir,
    roots: RootSet,
    store: DrawingStore,
    blobs: MemoryDrawingBlobs,
    register: BalloonRegister,
    ids: DeterministicV7Gen,
    part_rev: RevisionId,
    rev_b: DrawingRevId,
    rev_c: DrawingRevId,
    b_sheets: Vec<SheetId>,
    c_sheets: Vec<SheetId>,
}

fn build_carry_two_revs(tag: &str, c_pages_pt: &[(u32, u32)]) -> CarryFixture {
    let dir = TempDir::new(tag);
    // A3 landscape in PostScript points: 1191 x 842 pt (~42016 x 29704 cmm).
    let pdf_b = build_blank_pdf(&[(1191, 842), (1191, 842)]);
    let pdf_c = build_blank_pdf(c_pages_pt);
    std::fs::write(dir.0.join("rev_b.pdf"), &pdf_b).expect("write rev_b.pdf");
    std::fs::write(dir.0.join("rev_c.pdf"), &pdf_c).expect("write rev_c.pdf");

    let roots = setup_roots(&dir);
    let mut store = DrawingStore::new();
    let mut blobs = MemoryDrawingBlobs::new();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_000);
    let part_rev = RevisionId::new(ids.next_v7().unwrap()).expect("part_rev");

    let (res_b, res_c) = {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        let rb = add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: "root://import-draw/rev_b.pdf".into(),
                drawing_number: "DWG-CARRY-001".into(),
                revision_label: "B".into(),
                title: Some("Carry Fixture Rev B".into()),
                dry_run: false,
            },
        )
        .expect("import rev B");

        let rc = add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: "root://import-draw/rev_c.pdf".into(),
                drawing_number: "DWG-CARRY-001".into(),
                revision_label: "C".into(),
                title: Some("Carry Fixture Rev C".into()),
                dry_run: false,
            },
        )
        .expect("import rev C");
        (rb, rc)
    };

    let mut register = BalloonRegister::new();
    // 5 balloons on sheet 1 (x = 4000..20000), 5 balloons on sheet 2 (x = 15000..39000).
    for i in 1..=10u32 {
        let on_sheet_2 = i > 5;
        let sheet = if on_sheet_2 { res_b.sheets[1].id } else { res_b.sheets[0].id };
        let x = if on_sheet_2 { 15_000 + i64::from(i - 6) * 6_000 } else { 4_000 * i64::from(i) };
        let y = 8_000 + i64::from(i) * 1_000;
        register
            .add_balloon(
                &store,
                &mut ids,
                &AddBalloonParams {
                    drawing_rev: res_b.drawing_rev.id,
                    sheet,
                    characteristic: char_id(i),
                    anchor: SheetPoint::new(x, y),
                    label: SheetPoint::new(x + 800, y - 600),
                    number: Some(i),
                    insert: false,
                    zone: Some(format!("A{i}")),
                },
            )
            .expect("add balloon on rev B");
    }

    CarryFixture {
        _dir: dir,
        roots,
        store,
        blobs,
        register,
        ids,
        part_rev,
        rev_b: res_b.drawing_rev.id,
        rev_c: res_c.drawing_rev.id,
        b_sheets: res_b.sheets.iter().map(|s| s.id).collect(),
        c_sheets: res_c.sheets.iter().map(|s| s.id).collect(),
    }
}

#[test]
fn ac1_and_ac4_carry_ten_balloons_and_exact_replay_is_identical() {
    let mut fx = build_carry_two_revs("ac1", &[(1191, 842), (1191, 842)]);
    let b_before = fx.register.list(fx.rev_b);
    assert_eq!(b_before.len(), 10);

    // Dry-run does not mutate sink, store, or journal.
    let journal_before = fx.store.journal().len();
    {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        let dry = carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: fx.rev_c, sheet_map: None, dry_run: true },
        )
        .expect("dry run");
        assert!(!dry.committed);
        assert_eq!(dry.carried.len(), 10);
        assert!(dry.not_carried.is_empty());
    }
    assert_eq!(fx.store.journal().len(), journal_before);
    assert!(fx.register.list(fx.rev_c).is_empty());

    // Real carry: B -> C.
    let res = {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: fx.rev_c, sheet_map: None, dry_run: false },
        )
        .expect("carry B -> C")
    };
    assert!(res.committed);
    assert_eq!(res.carried.len(), 10);
    assert!(res.not_carried.is_empty());
    assert_eq!(fx.store.journal().len(), journal_before + 1);

    // AC-1: C has 10 balloons with B's numbers, char links, positions, new IDs, and 10 predecessor entries.
    let c_balloons = fx.register.list(fx.rev_c);
    assert_eq!(c_balloons.len(), 10);
    let c_rev = fx.store.get_revision(fx.rev_c).expect("rev C");
    assert_eq!(c_rev.carried_from.len(), 10);
    for (b_old, b_new) in b_before.iter().zip(c_balloons.iter()) {
        assert_ne!(b_old.id, b_new.id, "carried balloon must receive a fresh UUIDv7 ID");
        assert_eq!(b_old.number, b_new.number);
        assert_eq!(b_old.characteristic, b_new.characteristic);
        assert_eq!(b_old.anchor, b_new.anchor);
        assert_eq!(b_old.label, b_new.label);
        assert_eq!(b_old.zone, b_new.zone);
        assert_eq!(c_rev.carried_from.get(&b_new.id), Some(&b_old.id));
    }
    // REQ-005: B is completely unchanged.
    assert_eq!(fx.register.list(fx.rev_b), b_before);

    // Canonical JSON event roundtrip.
    let last_ev = fx.store.journal().last().expect("carry event");
    let roundtrip = DrawingEvent::from_value(&last_ev.to_value()).expect("roundtrip carry event");
    assert_eq!(&roundtrip, last_ev);

    // AC-4: exact replay produces IDENTICAL outcome and exact same carried balloon IDs.
    let mut replay_reg = BalloonRegister::new();
    for b in &b_before {
        let mut dummy_ids = DeterministicV7Gen::new(1);
        let mut seed_sink = RegisterBalloonSink::new(&mut replay_reg, &mut dummy_ids);
        seed_sink
            .add_with_number_exact(
                b.id,
                b.drawing_rev,
                b.sheet,
                b.characteristic.as_str(),
                CarryPoint::new(b.anchor.x, b.anchor.y),
                CarryPoint::new(b.label.x, b.label.y),
                b.number,
                b.zone.as_ref().map(|z| z.untrusted_value().as_str()),
            )
            .expect("seed source balloon for replay");
    }
    let mut replay_ids = DeterministicV7Gen::new(9_999_999);
    let run = {
        let mut replay_sink = RegisterBalloonSink::new(&mut replay_reg, &mut replay_ids);
        replay_exact_with_sink(fx.store.journal(), &fx.blobs, &mut replay_sink)
    };
    assert_eq!(run.outcome, ReplayOutcome::Identical, "mismatch at {:?}", run.first_mismatch);
    assert_eq!(replay_reg.list(fx.rev_c), c_balloons);
    assert_eq!(run.store.get_revision(fx.rev_c).expect("replayed C").carried_from, c_rev.carried_from);
}

#[test]
fn ac2_narrower_sheet2_and_unmapped_sheet_report_not_carried() {
    // Sheet 2 narrowed by ~10000 cmm (~283 pt): 1191 - 283 = 908 pt (~32032 cmm).
    // Sheet 2 balloons have x = 15000, 21000, 27000, 33000 (label 33800), 39000 (label 39800).
    // Balloons 9 (x=33000) and 10 (x=39000) lie beyond width ~32032 cmm -> off_sheet!
    let mut fx = build_carry_two_revs("ac2_narrow", &[(1191, 842), (908, 842)]);
    let b_balloons = fx.register.list(fx.rev_b);
    let res = {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: fx.rev_c, sheet_map: None, dry_run: false },
        )
        .expect("carry with narrower sheet 2")
    };
    assert_eq!(res.carried.len(), 8);
    assert_eq!(res.not_carried.len(), 2);
    assert_eq!(res.not_carried[0], (b_balloons[8].id, NotCarriedReason::OffSheet));
    assert_eq!(res.not_carried[1], (b_balloons[9].id, NotCarriedReason::OffSheet));
    assert_eq!(fx.register.list(fx.rev_c).len(), 8);
    assert_eq!(fx.store.get_revision(fx.rev_c).expect("rev C").carried_from.len(), 8);

    // Unmapped sheet test: rev C has only 1 sheet (explicit map omits sheet 2).
    let mut fx2 = build_carry_two_revs("ac2_unmapped", &[(1191, 842)]);
    let res2 = {
        let mut sink = RegisterBalloonSink::new(&mut fx2.register, &mut fx2.ids);
        carry_drawing_balloon(
            &mut fx2.store,
            &mut sink,
            CarryDrawingParams {
                from: fx2.rev_b,
                to: fx2.rev_c,
                sheet_map: Some(vec![(fx2.b_sheets[0], fx2.c_sheets[0])]),
                dry_run: false,
            },
        )
        .expect("carry with explicit 1-sheet map")
    };
    assert_eq!(res2.carried.len(), 5);
    assert_eq!(res2.not_carried.len(), 5);
    assert!(res2.not_carried.iter().all(|(_, r)| *r == NotCarriedReason::NoMappedSheet));
}

#[test]
fn ac3_invalid_targets_and_sink_failure_rollback() {
    let mut fx = build_carry_two_revs("ac3", &[(1191, 842), (1191, 842)]);
    // First carry succeeds.
    {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: fx.rev_c, sheet_map: None, dry_run: false },
        )
        .expect("first carry");
    }
    let jlen = fx.store.journal().len();

    // AC-3: carrying again into C fails with report.carry_target_invalid and journal unchanged.
    {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        let err = carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: fx.rev_c, sheet_map: None, dry_run: false },
        )
        .unwrap_err();
        assert_eq!(err.code(), "report.carry_target_invalid");
        assert_eq!(err, CarryError::CarryTargetInvalid { reason: "target_has_balloons" });
    }
    assert_eq!(fx.store.journal().len(), jlen);

    // Different part_rev -> report.carry_target_invalid.
    let other_part = RevisionId::new(fx.ids.next_v7().unwrap()).expect("other part");
    let rev_other = {
        let mut env =
            ImportEnv { roots: &fx.roots, principal: Principal::Agent, blobs: &mut fx.blobs, ids: &mut fx.ids };
        add_drawing_balloon(
            &mut fx.store,
            &mut env,
            &AddDrawingParams {
                part_rev: other_part,
                file: "root://import-draw/rev_c.pdf".into(),
                drawing_number: "DWG-OTHER-002".into(),
                revision_label: "A".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect("import other part rev")
    };
    let jlen2 = fx.store.journal().len();
    {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        let err = carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: rev_other.drawing_rev.id, sheet_map: None, dry_run: false },
        )
        .unwrap_err();
        assert_eq!(err, CarryError::CarryTargetInvalid { reason: "different_part_revision" });
    }
    assert_eq!(fx.store.journal().len(), jlen2);

    // Target sheet not ready (`needs_size` raster) -> report.carry_target_invalid.
    std::fs::write(fx._dir.0.join("nodpi.png"), build_nodpi_png()).expect("write nodpi.png");
    let rev_nodpi = {
        let mut env =
            ImportEnv { roots: &fx.roots, principal: Principal::Agent, blobs: &mut fx.blobs, ids: &mut fx.ids };
        add_drawing_balloon(
            &mut fx.store,
            &mut env,
            &AddDrawingParams {
                part_rev: fx.part_rev,
                file: "root://import-draw/nodpi.png".into(),
                drawing_number: "DWG-CARRY-001".into(),
                revision_label: "D".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect("import nodpi rev")
    };
    let jlen3 = fx.store.journal().len();
    {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        let err = carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: rev_nodpi.drawing_rev.id, sheet_map: None, dry_run: false },
        )
        .unwrap_err();
        assert_eq!(err, CarryError::CarryTargetInvalid { reason: "sheet_not_ready" });
    }
    assert_eq!(fx.store.journal().len(), jlen3);

    // Mid-carry sink error rolls back every balloon added so far and journals nothing.
    struct FailingSink {
        by_rev: BTreeMap<DrawingRevId, Vec<BalloonView>>,
        fail_after: usize,
        added_count: usize,
        ids: DeterministicV7Gen,
    }
    impl BalloonSink for FailingSink {
        fn list(&self, rev: DrawingRevId) -> Vec<BalloonView> {
            self.by_rev.get(&rev).cloned().unwrap_or_default()
        }
        fn add_with_number(
            &mut self,
            rev: DrawingRevId,
            sheet: SheetId,
            characteristic: &str,
            anchor: CarryPoint,
            label: CarryPoint,
            number: u32,
            zone: Option<&str>,
        ) -> Result<BalloonId, SinkError> {
            if self.added_count >= self.fail_after {
                return Err(SinkError::new("report.sink_injected_failure", "boom"));
            }
            self.added_count += 1;
            let id = BalloonId::new(self.ids.next_v7().unwrap()).unwrap();
            self.by_rev.entry(rev).or_default().push(BalloonView {
                id,
                sheet,
                characteristic: characteristic.to_string(),
                anchor,
                label,
                number,
                zone: zone.map(str::to_string),
            });
            Ok(id)
        }
        fn rollback_added(&mut self, rev: DrawingRevId, ids: &[BalloonId]) {
            if let Some(list) = self.by_rev.get_mut(&rev) {
                list.retain(|b| !ids.contains(&b.id));
            }
        }
    }

    let mut fx_rb = build_carry_two_revs("ac3_rollback", &[(1191, 842), (1191, 842)]);
    let src_views: Vec<BalloonView> = {
        let sink = RegisterBalloonSink::new(&mut fx_rb.register, &mut fx_rb.ids);
        sink.list(fx_rb.rev_b)
    };
    let mut failing = FailingSink {
        by_rev: BTreeMap::from([(fx_rb.rev_b, src_views)]),
        fail_after: 4,
        added_count: 0,
        ids: DeterministicV7Gen::new(2_000_000_000_000),
    };
    let jlen_rb = fx_rb.store.journal().len();
    let err = carry_drawing_balloon(
        &mut fx_rb.store,
        &mut failing,
        CarryDrawingParams { from: fx_rb.rev_b, to: fx_rb.rev_c, sheet_map: None, dry_run: false },
    )
    .unwrap_err();
    assert_eq!(err.code(), "report.sink_injected_failure");
    assert!(failing.list(fx_rb.rev_c).is_empty(), "all 4 added balloons must be rolled back");
    assert_eq!(fx_rb.store.journal().len(), jlen_rb, "nothing journaled on rollback");
    assert!(fx_rb.store.get_revision(fx_rb.rev_c).unwrap().carried_from.is_empty());
}

#[test]
fn command_def_and_perf_budget_2000_balloons() {
    let def = carry_command_def().expect("valid CommandDef");
    assert_eq!(def.id, "report.carry_drawing_balloon");
    assert_eq!(def.major, 1);
    assert_eq!(def.side_effect, fid_types::SideEffect::W2);
    assert_eq!(def.determinism, fid_types::Determinism::D0);
    assert_eq!(def.id.replace('.', "_"), "report_carry_drawing_balloon");

    // Performance budget: carry 2000 balloons cleanly and quickly.
    let mut fx = build_carry_two_revs("perf_2000", &[(1191, 842), (1191, 842)]);
    for i in 11..=2000u32 {
        let sheet = if i % 2 == 0 { fx.b_sheets[1] } else { fx.b_sheets[0] };
        let x = 500 + i64::from(i % 400) * 90;
        let y = 500 + i64::from(i / 400) * 400;
        fx.register
            .add_balloon(
                &fx.store,
                &mut fx.ids,
                &AddBalloonParams {
                    drawing_rev: fx.rev_b,
                    sheet,
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

    let t0 = Instant::now();
    let res = {
        let mut sink = RegisterBalloonSink::new(&mut fx.register, &mut fx.ids);
        carry_drawing_balloon(
            &mut fx.store,
            &mut sink,
            CarryDrawingParams { from: fx.rev_b, to: fx.rev_c, sheet_map: None, dry_run: false },
        )
        .expect("carry 2000")
    };
    let elapsed = t0.elapsed();
    assert_eq!(res.carried.len(), 2000);
    assert!(res.not_carried.is_empty());
    assert!(elapsed.as_millis() < 2_000, "carrying 2000 balloons took {} ms", elapsed.as_millis());
}
