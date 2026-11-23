//! Integration tests for spec node `IO.RES.REPORT` (`export.results_report@1`, AC-1..AC-6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::{Principal, RootAccess, RootId, RootSet};
use fid_io_res::report::{
    command_def, export_results_report, is_report_media_type, replay_results_report, MemoryReportStore,
    ReportExportError, ReportExportEvent, ReportExportRequest, COMMAND,
};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as Sha2Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn fixture_pdf() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/res/report/sample.pdf");
    fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

fn sha256_of(bytes: &[u8]) -> Digest {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&out);
    Digest::from_bytes(raw)
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("fid-io-res-report-{}-{}-{id}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp root dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn export_roots(dir: &Path) -> RootSet {
    let mut roots = RootSet::new();
    roots.insert(RootId::new("exp").expect("valid root id"), RootAccess::Export, dir.to_path_buf());
    roots
}

#[test]
fn command_definition_validates_in_registry() {
    let def = command_def().expect("export.results_report@1 CommandDef must be valid");
    assert_eq!(format!("{}@{}", def.id, def.major), COMMAND);
    assert_eq!(def.side_effect, fid_types::SideEffect::W1);
    assert_eq!(def.fs_effect, fid_registry::FsEffect::WriteRoot);
    assert_eq!(def.determinism, fid_types::Determinism::D0);
    assert_eq!(def.mcp_projection, fid_registry::Projection::Exposed);
}

#[test]
fn ac1_exports_sample_pdf_unchanged_and_journals_matching_digests() {
    let pdf = fixture_pdf();
    let mut store = MemoryReportStore::new();
    let digest = store.insert("application/pdf", pdf.clone());

    let root_dir = TempDir::new("ac1");
    let roots = export_roots(&root_dir.path);

    let req = ReportExportRequest { report: digest, target: "root://exp/r.pdf", overwrite: false, dry_run: false };
    let exported = export_results_report(&store, &roots, Principal::Human, &req).expect("AC-1 export succeeds");

    let out_path = root_dir.path.join("r.pdf");
    assert!(out_path.is_file(), "r.pdf must exist in export root");
    let written_bytes = fs::read(&out_path).expect("read r.pdf");
    assert_eq!(written_bytes, pdf, "REQ-001/REQ-004: bytes must be written unchanged");
    assert_eq!(sha256_of(&written_bytes), digest);

    assert_eq!(exported.result.target, "root://exp/r.pdf");
    assert_eq!(exported.result.sha256, digest);
    assert_eq!(exported.result.bytes, pdf.len() as u64);
    assert_eq!(exported.result.media_type, "application/pdf");

    // REQ-003: journal event records source blob SHA-256, media type, and written SHA-256 equal to blob digest.
    assert_eq!(exported.event.report, digest);
    assert_eq!(exported.event.written_sha256, digest);
    assert_eq!(exported.event.media_type, "application/pdf");
    assert_eq!(exported.event.bytes, pdf.len() as u64);

    // Event canonical JSON round-trips cleanly.
    let event_json = exported.event.canonical_bytes().expect("event canonical bytes");
    let parsed = ReportExportEvent::from_json(&event_json).expect("event round-trip");
    assert_eq!(parsed, exported.event);
}

#[test]
fn ac2_unknown_digest_fails_report_not_found_and_writes_nothing() {
    let store = MemoryReportStore::new();
    let missing = sha256_of(b"not-present-report");
    let root_dir = TempDir::new("ac2");
    let roots = export_roots(&root_dir.path);

    let req = ReportExportRequest { report: missing, target: "root://exp/r.pdf", overwrite: false, dry_run: false };
    let err = export_results_report(&store, &roots, Principal::Human, &req).expect_err("must fail report_not_found");
    assert_eq!(err.code(), "report_not_found");
    assert!(matches!(err, ReportExportError::ReportNotFound { digest } if digest == missing));
    assert!(!root_dir.path.join("r.pdf").exists(), "nothing may be written");
}

#[test]
fn ac3_non_report_media_type_fails_media_type_not_report() {
    let mut store = MemoryReportStore::new();
    let stl_digest = store.insert("model/stl", b"solid empty\nendsolid empty\n".to_vec());
    let root_dir = TempDir::new("ac3");
    let roots = export_roots(&root_dir.path);

    let req = ReportExportRequest { report: stl_digest, target: "root://exp/r.pdf", overwrite: false, dry_run: false };
    let err =
        export_results_report(&store, &roots, Principal::Human, &req).expect_err("must fail media_type_not_report");
    assert_eq!(err.code(), "media_type_not_report");
    assert!(!root_dir.path.join("r.pdf").exists(), "nothing may be written");
}

#[test]
fn ac4_existing_target_without_overwrite_fails_target_exists() {
    let pdf = fixture_pdf();
    let mut store = MemoryReportStore::new();
    let digest = store.insert("application/pdf", pdf.clone());
    let root_dir = TempDir::new("ac4");
    let roots = export_roots(&root_dir.path);

    let existing_bytes = b"pre-existing-report-content".to_vec();
    fs::write(root_dir.path.join("r.pdf"), &existing_bytes).expect("seed existing file");

    let req = ReportExportRequest { report: digest, target: "root://exp/r.pdf", overwrite: false, dry_run: false };
    let err = export_results_report(&store, &roots, Principal::Human, &req).expect_err("must fail target_exists");
    assert_eq!(err.code(), "target_exists");
    assert_eq!(fs::read(root_dir.path.join("r.pdf")).expect("existing untouched"), existing_bytes);

    // With overwrite: true it replaces atomically.
    let req_overwrite =
        ReportExportRequest { report: digest, target: "root://exp/r.pdf", overwrite: true, dry_run: false };
    let exported = export_results_report(&store, &roots, Principal::Human, &req_overwrite).expect("overwrite succeeds");
    assert_eq!(exported.event.written_sha256, digest);
    assert_eq!(fs::read(root_dir.path.join("r.pdf")).expect("overwritten file"), pdf);
}

#[test]
fn ac5_agent_raw_path_fails_raw_path_forbidden() {
    let pdf = fixture_pdf();
    let mut store = MemoryReportStore::new();
    let digest = store.insert("application/pdf", pdf);
    let root_dir = TempDir::new("ac5");
    let roots = export_roots(&root_dir.path);
    let raw_target = root_dir.path.join("r.pdf");
    let raw_str = raw_target.to_string_lossy().into_owned();

    let req = ReportExportRequest { report: digest, target: &raw_str, overwrite: false, dry_run: false };
    let err =
        export_results_report(&store, &roots, Principal::Agent, &req).expect_err("agent raw path must be forbidden");
    assert_eq!(err.code(), "raw_path_forbidden");
    assert!(!raw_target.exists(), "nothing may be written");
}

#[test]
fn ac6_exact_replay_into_empty_root_is_identical() {
    let pdf = fixture_pdf();
    let mut store = MemoryReportStore::new();
    let digest = store.insert("application/pdf", pdf.clone());

    let dir1 = TempDir::new("ac6-orig");
    let roots1 = export_roots(&dir1.path);
    let req = ReportExportRequest { report: digest, target: "root://exp/r.pdf", overwrite: false, dry_run: false };
    let exported = export_results_report(&store, &roots1, Principal::Human, &req).expect("initial export");

    // Replay into a brand new empty root.
    let dir2 = TempDir::new("ac6-replay");
    let roots2 = export_roots(&dir2.path);
    let outcome = replay_results_report(&exported.event, &store, &roots2, Principal::Human).expect("replay succeeds");
    assert_eq!(outcome, ReplayOutcome::Identical);
    assert_eq!(fs::read(dir2.path.join("r.pdf")).expect("replayed file"), pdf);

    // Replay when blob is missing returns NotReplayable.
    let empty_store = MemoryReportStore::new();
    let dir3 = TempDir::new("ac6-missing");
    let roots3 = export_roots(&dir3.path);
    let missing_outcome =
        replay_results_report(&exported.event, &empty_store, &roots3, Principal::Human).expect("missing replay");
    assert_eq!(missing_outcome, ReplayOutcome::NotReplayable);
}

#[test]
fn path_outside_root_corrupt_blob_dry_run_and_media_type_allowlist() {
    // Allowlist checks.
    assert!(is_report_media_type("application/pdf"));
    assert!(is_report_media_type("APPLICATION/PDF"));
    assert!(is_report_media_type("text/html"));
    assert!(is_report_media_type("application/vnd.openxmlformats-officedocument.wordprocessingml.document"));
    assert!(is_report_media_type("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"));
    assert!(!is_report_media_type("model/stl"));
    assert!(!is_report_media_type("application/pdf; charset=utf-8"));
    assert!(!is_report_media_type("application/vnd.openxmlformats-officedocument."));
    assert!(!is_report_media_type("application/vnd.openxmlformats-officedocument.foo..bar"));
    assert!(!is_report_media_type(""));

    let pdf = fixture_pdf();
    let mut store = MemoryReportStore::new();
    let digest = store.insert("text/html", pdf.clone());
    let root_dir = TempDir::new("extra");
    let roots = export_roots(&root_dir.path);

    // Traversal fails path_outside_root.
    let traversal =
        ReportExportRequest { report: digest, target: "root://exp/../escape.html", overwrite: false, dry_run: false };
    let err = export_results_report(&store, &roots, Principal::Human, &traversal).expect_err("traversal rejected");
    assert_eq!(err.code(), "path_outside_root");

    // Dry run validates and returns matching digests without writing to disk.
    let dry = ReportExportRequest { report: digest, target: "root://exp/dry.html", overwrite: false, dry_run: true };
    let dry_res = export_results_report(&store, &roots, Principal::Human, &dry).expect("dry run succeeds");
    assert_eq!(dry_res.event.written_sha256, digest);
    assert!(dry_res.event.dry_run);
    assert!(!root_dir.path.join("dry.html").exists(), "dry run must not write file");

    // Corrupt blob (bytes don't match requested digest) cancels atomic write and leaves no target.
    let fake_digest = sha256_of(b"claimed-content");
    store.insert_under_digest(fake_digest, "application/pdf", b"tampered-content".to_vec());
    let bad =
        ReportExportRequest { report: fake_digest, target: "root://exp/corrupt.pdf", overwrite: false, dry_run: false };
    let err = export_results_report(&store, &roots, Principal::Human, &bad).expect_err("corrupt blob rejected");
    assert_eq!(err.code(), "digest_mismatch");
    assert!(!root_dir.path.join("corrupt.pdf").exists(), "corrupt blob must not be committed");
}
