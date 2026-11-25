//! Acceptance and integration tests for `RPT.BALLOON.SHEET.IMPORT` (`fid_report::balloon::sheet`, AC-1..AC-6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_project::Uuid;
use fid_replay::ReplayOutcome;
use fid_report::balloon::sheet::{
    add_drawing_balloon, command_defs, digest_bytes, execute, list_drawing_balloon, list_drawing_value, replay_exact,
    set_size_drawing_balloon, AddDrawingParams, CasDrawingBlobs, DeterministicV7Gen, DrawingEvent, DrawingStore,
    ImportEnv, ListDrawingParams, MemoryDrawingBlobs, RevisionId, SetSizeParams, SheetError, SheetInfo, SheetStatus,
    SizeSpec, ADD_DRAWING_BALLOON, LIST_DRAWING_BALLOON, SET_SIZE_DRAWING_BALLOON,
};
use fid_store::cas::BlobStore;

fn draw_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/reg/plugin/draw")
}

fn roots_for_drawings() -> RootSet {
    let mut roots = RootSet::new();
    roots.insert(RootId::new("drawings").expect("root id"), RootAccess::Import, draw_fixtures_dir());
    roots
}

fn part_rev_id(seq: u8) -> RevisionId {
    let bytes = [0x01, 0x90, 0x00, 0x00, 0x00, 0x01, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, seq];
    RevisionId::new(Uuid::from_bytes(bytes)).expect("valid uuidv7")
}

struct TmpDir(PathBuf);

impl TmpDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let dir = std::env::temp_dir().join(format!("fid_balloon_sheet_{tag}_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TmpDir(dir)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_import_three_pages_pdf_with_cropbox_and_cas_and_journal() {
    let tmp = TmpDir::new("ac1");
    let cas = BlobStore::open(&tmp.0.join("blobs")).expect("open BlobStore");
    let mut blobs = CasDrawingBlobs(&cas);
    let roots = roots_for_drawings();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_000);
    let mut store = DrawingStore::new();
    let part_rev = part_rev_id(1);

    let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
    let res = add_drawing_balloon(
        &mut store,
        &mut env,
        &AddDrawingParams {
            part_rev,
            file: "root://drawings/three_pages.pdf".into(),
            drawing_number: "D-100".into(),
            revision_label: "B".into(),
            title: Some("Bracket Assembly".into()),
            dry_run: false,
        },
    )
    .expect("import three_pages.pdf");

    assert!(res.committed);
    assert!(res.warnings.is_empty());
    assert_eq!(res.sheets.len(), 3);
    assert_eq!(res.drawing_rev.sheets.len(), 3);
    assert_eq!(res.drawing_rev.part_rev, part_rev);
    assert_eq!(res.drawing_rev.drawing_number.untrusted_value().as_str(), "D-100");
    assert_eq!(res.drawing_rev.revision_label.untrusted_value().as_str(), "B");

    // Page order & CropBox on page 2 (index 1: 150 x 100 mm = 15000 x 10000 cmm).
    assert_eq!((res.sheets[0].index, res.sheets[0].width_cmm, res.sheets[0].height_cmm), (0, 21_000, 29_700));
    assert_eq!((res.sheets[1].index, res.sheets[1].width_cmm, res.sheets[1].height_cmm), (1, 15_000, 10_000));
    assert_eq!((res.sheets[2].index, res.sheets[2].width_cmm, res.sheets[2].height_cmm), (2, 21_590, 27_940));

    // SheetInfo trait reports ready + exact bounds + owning revision.
    for s in &res.sheets {
        assert!(store.ready(s.id));
        assert_eq!(store.bounds(s.id), Some((s.width_cmm, s.height_cmm)));
        assert_eq!(store.revision(s.id), Some(res.drawing_rev.id));
        assert_eq!(store.image(s.id), None);
    }

    // Source bytes stored in on-disk CAS under exact SHA-256 digest.
    let expected_raw = std::fs::read(draw_fixtures_dir().join("three_pages.pdf")).expect("read fixture");
    assert_eq!(res.drawing_rev.source_digest, digest_bytes(&expected_raw));
    let stored = cas.read_object(&res.drawing_rev.source_digest, 10_000_000).expect("read from CAS");
    assert_eq!(stored, expected_raw);

    // Exactly one journal event committed.
    assert_eq!(store.journal().len(), 1);
    assert_eq!(store.journal()[0].kind(), ADD_DRAWING_BALLOON);
}

// ------------------------------------------------------------------ AC-2 & AC-6

#[test]
fn ac2_and_ac6_nodpi_png_needs_size_set_dpi_and_exact_replay() {
    let mut blobs = MemoryDrawingBlobs::new();
    let roots = roots_for_drawings();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_100);
    let mut store = DrawingStore::new();
    let part_rev = part_rev_id(2);

    let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
    let res = add_drawing_balloon(
        &mut store,
        &mut env,
        &AddDrawingParams {
            part_rev,
            file: "root://drawings/nodpi.png".into(),
            drawing_number: "D-200".into(),
            revision_label: "A".into(),
            title: None,
            dry_run: false,
        },
    )
    .expect("import nodpi.png");

    assert_eq!(res.sheets.len(), 1);
    let sheet_id = res.sheets[0].id;
    assert_eq!(res.sheets[0].status, SheetStatus::NeedsSize);
    assert_eq!(res.sheets[0].px, Some((64, 48)));
    assert!(res.sheets[0].image.is_some());
    assert!(!store.ready(sheet_id));
    assert_eq!(store.bounds(sheet_id), None);
    assert_eq!(store.image(sheet_id), res.sheets[0].image);

    // Set DPI to 300: 64 * 2540 / 300 = 541.866.. -> 542; 48 * 2540 / 300 = 406.4 -> 406.
    let sized = set_size_drawing_balloon(
        &mut store,
        &SetSizeParams { sheet: sheet_id, size: SizeSpec::Dpi { x: 300, y: 300 } },
    )
    .expect("set dpi 300");
    assert_eq!(sized.status, SheetStatus::Ready);
    assert_eq!(sized.width_cmm, 542);
    assert_eq!(sized.height_cmm, 406);
    assert!(store.ready(sheet_id));
    assert_eq!(store.bounds(sheet_id), Some((542, 406)));
    assert_eq!(store.journal().len(), 2);

    // Also verify aspect-preserving Width spec: width = 16000 cmm -> height = 16000 * 48 / 64 = 12000 cmm.
    let sized2 =
        set_size_drawing_balloon(&mut store, &SetSizeParams { sheet: sheet_id, size: SizeSpec::Width { cmm: 16_000 } })
            .expect("set width 16000");
    assert_eq!((sized2.width_cmm, sized2.height_cmm), (16_000, 12_000));
    assert_eq!(store.journal().len(), 3);

    // Round-trip journal events through canonical CValue JSON.
    let round_tripped: Vec<DrawingEvent> =
        store.journal().iter().map(|ev| DrawingEvent::from_value(&ev.to_value()).expect("event round-trip")).collect();
    assert_eq!(round_tripped, store.journal());

    // AC-6: Exact replay produces ReplayOutcome::Identical with unchanged sheet IDs.
    let run = replay_exact(&round_tripped, &blobs);
    assert_eq!(run.outcome, ReplayOutcome::Identical);
    assert_eq!(run.first_mismatch, None);
    assert_eq!(run.store.sheet(sheet_id).expect("replayed sheet"), &sized2);
    assert!(run.store.ready(sheet_id));

    // Tampering with stored source blob is caught by replay as Divergent.
    let mut tampered = blobs.clone();
    tampered.tamper(res.drawing_rev.source_digest, b"corrupted bytes".to_vec());
    let bad_run = replay_exact(&round_tripped, &tampered);
    assert_eq!(bad_run.outcome, ReplayOutcome::Divergent);
    assert_eq!(bad_run.first_mismatch, Some(0));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_unreadable_encrypted_and_pages201_fail_without_journaling() {
    let mut blobs = MemoryDrawingBlobs::new();
    let roots = roots_for_drawings();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_200);
    let mut store = DrawingStore::new();
    let part_rev = part_rev_id(3);

    {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        let err = add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: "root://drawings/encrypted.pdf".into(),
                drawing_number: "D-ENC".into(),
                revision_label: "A".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect_err("encrypted.pdf must fail");
        assert_eq!(err.code(), "report.drawing_unreadable");
        match err {
            SheetError::DrawingUnreadable { code, .. } => assert_eq!(code, "pdf_encrypted"),
            other => panic!("expected DrawingUnreadable, got {other:?}"),
        }
    }
    assert_eq!(store.journal().len(), 0);
    assert!(blobs.is_empty());

    {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        let err = add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: "root://drawings/pages201.pdf".into(),
                drawing_number: "D-BIG".into(),
                revision_label: "A".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect_err("pages201.pdf must fail");
        assert_eq!(err.code(), "report.drawing_unreadable");
        match err {
            SheetError::DrawingUnreadable { code, .. } => assert_eq!(code, "limit_exceeded"),
            other => panic!("expected DrawingUnreadable, got {other:?}"),
        }
    }
    assert_eq!(store.journal().len(), 0);
    assert!(blobs.is_empty());
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_raw_path_refused_for_agent_and_duplicate_revision_rejected() {
    let mut blobs = MemoryDrawingBlobs::new();
    let roots = roots_for_drawings();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_300);
    let mut store = DrawingStore::new();
    let part_rev = part_rev_id(4);
    let raw_path = draw_fixtures_dir().join("a3_landscape.pdf").to_string_lossy().into_owned();

    // Agent principal passing a raw filesystem path is rejected before any fs call and journals nothing.
    {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        let err = add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: raw_path.clone(),
                drawing_number: "D-400".into(),
                revision_label: "A".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect_err("agent raw path must be refused");
        assert_eq!(err.code(), "raw_path_forbidden");
    }
    assert_eq!(store.journal().len(), 0);

    // Script principal passing raw path is also refused.
    {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Script, blobs: &mut blobs, ids: &mut ids };
        let err = add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: raw_path,
                drawing_number: "D-400".into(),
                revision_label: "A".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect_err("script raw path must be refused");
        assert_eq!(err.code(), "raw_path_forbidden");
    }
    assert_eq!(store.journal().len(), 0);

    // First valid import succeeds; importing same (part_rev, drawing_number, revision_label) fails.
    {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: "root://drawings/a3_landscape.pdf".into(),
                drawing_number: "D-400".into(),
                revision_label: "A".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect("first import ok");
        let err = add_drawing_balloon(
            &mut store,
            &mut env,
            &AddDrawingParams {
                part_rev,
                file: "root://drawings/a3_landscape.pdf".into(),
                drawing_number: "D-400".into(),
                revision_label: "A".into(),
                title: None,
                dry_run: false,
            },
        )
        .expect_err("duplicate must fail");
        assert_eq!(err.code(), "report.drawing_revision_exists");
    }
    assert_eq!(store.journal().len(), 1);
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_js_action_pdf_succeeds_with_warning_visible_in_list() {
    let mut blobs = MemoryDrawingBlobs::new();
    let roots = roots_for_drawings();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_400);
    let mut store = DrawingStore::new();
    let part_rev = part_rev_id(5);

    let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
    let res = add_drawing_balloon(
        &mut store,
        &mut env,
        &AddDrawingParams {
            part_rev,
            file: "root://drawings/js_action.pdf".into(),
            drawing_number: "D-500".into(),
            revision_label: "A".into(),
            title: None,
            dry_run: false,
        },
    )
    .expect("js_action.pdf imports with warning");
    assert!(res.warnings.iter().any(|w| w.code == "pdf_javascript_ignored"));

    let listed = list_drawing_balloon(&store, &ListDrawingParams { part_rev: Some(part_rev) });
    assert_eq!(listed.len(), 1);
    assert!(listed[0].drawing_rev.warnings.iter().any(|w| w.code == "pdf_javascript_ignored"));
}

// ------------------------------------------------------------------ Registry schemas, dry-run, ordering, text bounds

#[test]
fn registry_command_defs_validate_params_and_results_and_dry_run() {
    let defs = command_defs().expect("command_defs must validate");
    assert_eq!(defs.len(), 3);
    assert_eq!(defs[0].id.as_str(), "report.add_drawing_balloon");
    assert_eq!(defs[1].id.as_str(), "report.list_drawing_balloon");
    assert_eq!(defs[2].id.as_str(), "report.set_size_drawing_balloon");

    let add_params_schema = Compiled::compile(&defs[0].params_schema).expect("compile add params schema");
    let add_result_schema = Compiled::compile(&defs[0].result_schema).expect("compile add result schema");
    let list_params_schema = Compiled::compile(&defs[1].params_schema).expect("compile list params schema");
    let list_result_schema = Compiled::compile(&defs[1].result_schema).expect("compile list result schema");
    let size_params_schema = Compiled::compile(&defs[2].params_schema).expect("compile size params schema");
    let size_result_schema = Compiled::compile(&defs[2].result_schema).expect("compile size result schema");

    let mut blobs = MemoryDrawingBlobs::new();
    let roots = roots_for_drawings();
    let mut ids = DeterministicV7Gen::new(1_760_000_000_500);
    let mut store = DrawingStore::new();
    let part_rev = part_rev_id(6);

    // Dry-run does not mutate store or CAS.
    let dry_json = format!(
        r#"{{"part_rev":"{part_rev}","file":"root://drawings/nodpi.png","drawing_number":"D-600","revision_label":"A","dry_run":true}}"#
    );
    let dry_params = fid_canon::parse(dry_json.as_bytes()).expect("json");
    assert!(add_params_schema.evaluate(&dry_params, &NoHooks).is_ok());
    {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        let dry_res = execute(ADD_DRAWING_BALLOON, &dry_params, &mut store, &mut env).expect("dry-run ok");
        assert!(add_result_schema.evaluate(&dry_res, &NoHooks).is_ok());
    }
    assert_eq!(store.journal().len(), 0);
    assert!(blobs.is_empty());

    // Live execute via JSON command dispatcher.
    let add_json = format!(
        r#"{{"part_rev":"{part_rev}","file":"root://drawings/nodpi.png","drawing_number":"D-600","revision_label":"A"}}"#
    );
    let add_params = fid_canon::parse(add_json.as_bytes()).expect("json");
    assert!(add_params_schema.evaluate(&add_params, &NoHooks).is_ok());
    let add_res = {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        execute(ADD_DRAWING_BALLOON, &add_params, &mut store, &mut env).expect("live add ok")
    };
    assert!(add_result_schema.evaluate(&add_res, &NoHooks).is_ok());

    let sheet_id_str = match &add_res {
        CValue::Obj(m) => match m.get("sheets") {
            Some(CValue::Arr(arr)) => match &arr[0] {
                CValue::Obj(sm) => match sm.get("id") {
                    Some(CValue::Str(s)) => s.clone(),
                    _ => panic!("missing sheet id"),
                },
                _ => panic!("bad sheet"),
            },
            _ => panic!("missing sheets"),
        },
        _ => panic!("bad add_res"),
    };

    let size_json = format!(r#"{{"sheet":"{sheet_id_str}","size":{{"kind":"dpi","x":"300","y":"300"}}}}"#);
    let size_params = fid_canon::parse(size_json.as_bytes()).expect("json");
    assert!(size_params_schema.evaluate(&size_params, &NoHooks).is_ok());
    let size_res = {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        execute(SET_SIZE_DRAWING_BALLOON, &size_params, &mut store, &mut env).expect("set size ok")
    };
    assert!(size_result_schema.evaluate(&size_res, &NoHooks).is_ok());

    let list_params = fid_canon::parse(b"{}").expect("json");
    assert!(list_params_schema.evaluate(&list_params, &NoHooks).is_ok());
    let list_res = {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        execute(LIST_DRAWING_BALLOON, &list_params, &mut store, &mut env).expect("list ok")
    };
    assert!(list_result_schema.evaluate(&list_res, &NoHooks).is_ok());
    let _ = list_drawing_value(&list_drawing_balloon(&store, &ListDrawingParams { part_rev: None }));

    // Text > 256 bytes rejected with report.text_invalid (REQ-004).
    let long_dn = "X".repeat(257);
    let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
    let err = add_drawing_balloon(
        &mut store,
        &mut env,
        &AddDrawingParams {
            part_rev,
            file: "root://drawings/a3_landscape.pdf".into(),
            drawing_number: long_dn,
            revision_label: "A".into(),
            title: None,
            dry_run: false,
        },
    )
    .expect_err("257-byte drawing number must fail");
    assert_eq!(err.code(), "report.text_invalid");

    // Performance check: importing a 2-page CCITT G4 TIFF stays well under budget.
    let t0 = Instant::now();
    let tif_res = add_drawing_balloon(
        &mut store,
        &mut env,
        &AddDrawingParams {
            part_rev,
            file: "root://drawings/g4_two_pages.tif".into(),
            drawing_number: "D-700".into(),
            revision_label: "A".into(),
            title: None,
            dry_run: false,
        },
    )
    .expect("import g4_two_pages.tif");
    assert_eq!(tif_res.sheets.len(), 2);
    assert!(tif_res.sheets.iter().all(|s| s.status == SheetStatus::Ready));
    assert!(t0.elapsed().as_millis() < 2000);
}
