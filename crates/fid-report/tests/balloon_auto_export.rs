//! End-to-end acceptance tests for `RPT.BALLOON.AUTO.EXPORT` (`fid_report::balloon::auto_export`, AC-1..AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fid_gdt::chars::dprof::DprofState;
use fid_gdt::chars::spec::{PlanId, SpecState};
use fid_io_draw::read_drawing_bytes;
use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_project::Uuid;
use fid_report::balloon::auto_export::{
    command_def, csv_cell_untrusted, export_balloon_pack, BalloonOutcome, EvaluatedRowValue, ExportBalloonPackParams,
};
use fid_report::balloon::auto_propose::{AdoptDecision, ProposalBranchStore, ProposeBalloonsParams, ZoneGridSpec};
use fid_report::balloon::char::{AuthoredUnit, CharBalloonStore, Dialect};
use fid_report::balloon::register::BalloonRegister;
use fid_report::balloon::sheet::{
    add_drawing_balloon, digest_bytes, AddDrawingParams, DeterministicV7Gen, DrawingStore, ImportEnv,
    MemoryDrawingBlobs, RevisionId,
};

struct TmpDir(PathBuf);

impl TmpDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let dir = std::env::temp_dir().join(format!("fid_balloon_export_{tag}_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TmpDir(dir)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn auto_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/balloon/auto")
}

fn part_rev_id(seq: u8) -> RevisionId {
    let bytes = [0x01, 0x90, 0x00, 0x00, 0x00, 0x40, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, seq];
    RevisionId::new(Uuid::from_bytes(bytes)).expect("v7")
}

// ------------------------------------------------------------------ AC-1 .. AC-4 (Full End-to-End 2D PDF Ballooning Pipeline!)

#[test]
fn ac1_to_ac4_full_pipeline_vector_pdf_export_determinism_root_guard_and_csv_injection() {
    let tmp = TmpDir::new("e2e");
    let export_dir = tmp.0.join("exports");
    std::fs::create_dir_all(export_dir.join("pack")).expect("create pack subdir");
    std::fs::create_dir_all(export_dir.join("pack2")).expect("create pack2 subdir");

    let mut roots = RootSet::new();
    roots.insert(RootId::new("auto").expect("root"), RootAccess::Import, auto_fixtures_dir());
    roots.insert(RootId::new("exports").expect("root"), RootAccess::Export, &export_dir);

    let mut drawing_store = DrawingStore::new();
    let mut blobs = MemoryDrawingBlobs::new();
    let mut ids = DeterministicV7Gen::new(1_760_001_000_000);

    // 1. Import vector_bracket.pdf
    let imported = {
        let mut env = ImportEnv { roots: &roots, principal: Principal::Agent, blobs: &mut blobs, ids: &mut ids };
        add_drawing_balloon(
            &mut drawing_store,
            &mut env,
            &AddDrawingParams {
                part_rev: part_rev_id(1),
                file: "root://auto/vector_bracket.pdf".into(),
                drawing_number: "BRK-E2E".into(),
                revision_label: "A".into(),
                title: Some("Bracket Day-1 Ballooning".into()),
                dry_run: false,
            },
        )
        .expect("import ok")
    };
    let drawing_rev = imported.drawing_rev.id;

    // 2. Propose all 15 balloons on W1 draft branch
    let plan = PlanId::parse("PLAN-E2E").expect("plan");
    let mut spec = SpecState::new();
    let mut dprof = DprofState::new();
    let mut register = BalloonRegister::new();
    let mut char_store = CharBalloonStore::new();
    let mut proposals = ProposalBranchStore::new();

    let batch = proposals
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
                ocr_candidates: Vec::new(),
                branch: "agent/e2e".into(),
                dry_run: false,
            },
        )
        .expect("propose ok");
    assert_eq!(batch.proposals.len(), 15);

    // 3. Human H2 adopts all 15 proposals, injecting a formula-injection zone string on balloon #2 (AC-4!)
    //    (Note: register zone max is 16 bytes -> test exact formula payload "=CMD|'calc'!A0" = 14 bytes
    //    AND directly unit-test full "=CMD|'/C calc'!A0" through csv_cell_untrusted!)
    let mut decisions: Vec<AdoptDecision> = (1..=15u32)
        .map(|n| AdoptDecision {
            provisional_number: n,
            edited_char_input: None,
            edited_label: None,
            edited_zone: if n == 2 { Some("=CMD|'calc'!A0".into()) } else { None },
        })
        .collect();
    // Ensure balloon #1 has a clean zone.
    decisions[0].edited_zone = Some("A1".into());

    let adopted = proposals
        .adopt_proposal_batch(
            "agent/e2e",
            Principal::Human,
            &decisions,
            &mut char_store,
            &mut spec,
            &mut dprof,
            &mut register,
            &drawing_store,
            &mut ids,
        )
        .expect("human adopt 15 ok");
    assert_eq!(adopted.adopted.len(), 15);

    // Add one evaluated result (conforms) + leave 14 as not_evaluated to exercise both paths.
    let mut evaluated = BTreeMap::new();
    evaluated.insert(
        adopted.adopted[0].characteristic_id.clone(),
        EvaluatedRowValue { formatted_result: "120.02 mm".into(), outcome: BalloonOutcome::Conforms },
    );

    // AC-1: Export ballooned.pdf + form3-starter.csv into approved export root.
    let mut export_journal = Vec::new();
    let t0 = Instant::now();
    let res1 = export_balloon_pack(
        &drawing_store,
        &blobs,
        &register,
        &char_store,
        &evaluated,
        &roots,
        Principal::Agent,
        &mut export_journal,
        &ExportBalloonPackParams {
            drawing_rev,
            out_dir: "root://exports/pack".into(),
            outcome_marking: true,
            include_unmeasured: true,
            dry_run: false,
        },
    )
    .expect("AC-1 export pack ok");
    assert!(t0.elapsed().as_millis() < 250, "export pack took {} ms", t0.elapsed().as_millis());
    assert!(res1.committed);
    assert_eq!(res1.balloon_count, 15);
    assert_eq!(export_journal.len(), 1);
    assert_eq!(export_journal[0].result, res1);

    let pdf1 = std::fs::read(export_dir.join("pack/ballooned.pdf")).expect("read ballooned.pdf");
    let csv1 = std::fs::read(export_dir.join("pack/form3-starter.csv")).expect("read form3-starter.csv");
    assert_eq!(digest_bytes(&pdf1), res1.pdf_digest);
    assert_eq!(digest_bytes(&csv1), res1.csv_digest);

    // Independent verification: exported ballooned.pdf parses cleanly back through fid-io-draw with exact A3 geometry!
    let reparsed = read_drawing_bytes(None, &pdf1).expect("ballooned.pdf is a valid PDF 1.7");
    assert_eq!(reparsed.pages.len(), 1);
    assert_eq!((reparsed.pages[0].width_cmm, reparsed.pages[0].height_cmm), (42_000, 29_700));
    let pdf_str = String::from_utf8_lossy(&pdf1);
    for n in 1..=15 {
        assert!(pdf_str.contains(&format!("({n}) Tj")), "missing vector balloon ({n}) in ballooned.pdf");
    }

    // Verify Form 3 CSV has header + 15 rows numbered 1..=15 in ascending order + AC-4 formula neutralisation!
    let csv_text = String::from_utf8(csv1.clone()).expect("utf8 csv");
    let lines: Vec<&str> = csv_text.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 16, "expected header + 15 balloon rows, got {}", lines.len());
    assert!(lines[0].starts_with("CharNo,ReferenceLocation,"));
    assert!(lines[1].starts_with("1,A1,"));
    assert!(lines[1].contains("120.02 mm,conforms"));
    assert!(lines[2].starts_with("2,\"'=CMD|'calc'!A0\","), "formula must be neutralised: {}", lines[2]);
    assert!(lines[15].starts_with("15,"));
    assert!(lines[15].ends_with(",not_evaluated"));

    // AC-4 direct formula check on full spec payload `=CMD|'/C calc'!A0`:
    let neutralised = csv_cell_untrusted("=CMD|'/C calc'!A0");
    assert_eq!(neutralised, "\"'=CMD|'/C calc'!A0\"");
    assert!(!neutralised.starts_with('='));

    // AC-2: Running a second time produces byte-identical PDF and CSV files!
    let res2 = export_balloon_pack(
        &drawing_store,
        &blobs,
        &register,
        &char_store,
        &evaluated,
        &roots,
        Principal::Agent,
        &mut export_journal,
        &ExportBalloonPackParams {
            drawing_rev,
            out_dir: "root://exports/pack2".into(),
            outcome_marking: true,
            include_unmeasured: true,
            dry_run: false,
        },
    )
    .expect("AC-2 second export ok");
    let pdf2 = std::fs::read(export_dir.join("pack2/ballooned.pdf")).expect("read pdf2");
    let csv2 = std::fs::read(export_dir.join("pack2/form3-starter.csv")).expect("read csv2");
    assert_eq!(pdf1, pdf2, "ballooned.pdf must be byte-identical across runs");
    assert_eq!(csv1, csv2, "form3-starter.csv must be byte-identical across runs");
    assert_eq!(res1.pdf_digest, res2.pdf_digest);
    assert_eq!(res1.csv_digest, res2.csv_digest);

    // AC-3: Path traversal `../outside` or unapproved raw path fails closed with io.root_violation and writes nothing!
    for bad_out in ["../outside", "root://exports/../outside", "/tmp/outside_fid_escape"] {
        let err = export_balloon_pack(
            &drawing_store,
            &blobs,
            &register,
            &char_store,
            &evaluated,
            &roots,
            Principal::Agent,
            &mut export_journal,
            &ExportBalloonPackParams {
                drawing_rev,
                out_dir: bad_out.into(),
                outcome_marking: false,
                include_unmeasured: true,
                dry_run: false,
            },
        )
        .expect_err("root escape must fail");
        assert_eq!(err.code(), "io.root_violation", "bad_out={bad_out}");
    }
    assert!(!tmp.0.join("outside").exists());
    assert_eq!(export_journal.len(), 2);

    // Registry CommandDef validates cleanly.
    let def = command_def().expect("command_def valid");
    assert_eq!(def.id.as_str(), "report.export_balloon_pack");
}
