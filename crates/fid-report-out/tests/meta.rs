//! Integration tests for `RPT.OUTPUT.META` (AC-1 through AC-7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::roots::{RootAccess, RootId};
use fid_io_reg::Principal;
use fid_report::bind::ResolvedReport;
use fid_report_out::{ExportError, ExportMeta, ExportSink, LocalRootResolver, RendererVersion};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/output")
}

struct TmpDir {
    path: PathBuf,
}

impl TmpDir {
    fn new(tag: &str) -> TmpDir {
        let path = std::env::temp_dir().join(format!("fid-report-out-meta-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TmpDir { path }
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Deterministic seeded PRNG (SplitMix64, seed 7) for AC-2 1 MiB payload — pure integer, no floats.
fn seeded_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let remain = len - out.len();
        let bytes = z.to_le_bytes();
        out.extend_from_slice(&bytes[..remain.min(8)]);
    }
    out
}

#[test]
fn ac1_export_meta_matches_golden_and_is_deterministic() {
    let resolved_bytes = fs::read(fixture_dir().join("resolved_small.json")).unwrap();
    let golden_meta = fs::read(fixture_dir().join("resolved_small.meta.json")).unwrap();
    let resolved = ResolvedReport::from_canonical_bytes(&resolved_bytes).unwrap();
    let renderer = RendererVersion::new("pdf", "1.0.0").unwrap();

    let meta1 = ExportMeta::from_resolved(&resolved, renderer.clone());
    let meta2 = ExportMeta::from_resolved(&resolved, renderer);
    let b1 = meta1.to_canonical_bytes().unwrap();
    let b2 = meta2.to_canonical_bytes().unwrap();

    assert_eq!(b1, golden_meta, "ExportMeta canonical JSON differs from resolved_small.meta.json");
    assert_eq!(b1, b2, "ExportMeta must be byte-identical across runs");
    assert_eq!(ExportMeta::from_canonical_bytes(&b1).unwrap(), meta1);

    // REQ-002: no wall-clock time, host name or user name keys.
    let s = String::from_utf8(b1).unwrap();
    for forbidden in ["timestamp", "created_at", "hostname", "host", "username", "user"] {
        assert!(!s.contains(forbidden), "ExportMeta unexpectedly contains {forbidden}");
    }
}

#[test]
fn ac2_write_one_mib_atomic_and_clean_root() {
    let tmp = TmpDir::new("ac2");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);

    let payload = seeded_bytes(7, 1024 * 1024);
    let expected_sha = Digest::from_bytes(Sha256::digest(&payload).into());

    let receipt = sink.write(Principal::Agent, "root://exports", "report_a.pdf", &payload, false).unwrap();
    assert_eq!(receipt.root.wire(), "root://exports");
    assert_eq!(receipt.name.as_str(), "report_a.pdf");
    assert_eq!(receipt.bytes, 1024 * 1024);
    assert_eq!(receipt.sha256, expected_sha);

    let on_disk = fs::read(tmp.path.join("report_a.pdf")).unwrap();
    assert_eq!(on_disk, payload);

    // No temporary file remains in the root.
    let names: Vec<String> =
        fs::read_dir(&tmp.path).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["report_a.pdf".to_string()]);
}

#[test]
fn ac3_agent_raw_path_forbidden_and_root_unchanged() {
    let tmp = TmpDir::new("ac3");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);

    let raw_path = tmp.path.to_string_lossy().into_owned();
    let err_agent = sink.write(Principal::Agent, &raw_path, "report_a.pdf", b"hello", false).unwrap_err();
    assert_eq!(err_agent.code(), "raw_path_forbidden");
    let err_script = sink.write(Principal::Script, &raw_path, "report_a.pdf", b"hello", false).unwrap_err();
    assert_eq!(err_script.code(), "raw_path_forbidden");

    // Root directory remains empty.
    assert_eq!(fs::read_dir(&tmp.path).unwrap().count(), 0);
}

#[test]
fn ac4_invalid_export_names_rejected() {
    let tmp = TmpDir::new("ac4");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);

    let long_name = "a".repeat(129);
    let bad_names = ["../x.pdf", ".hidden", "a/b.pdf", long_name.as_str(), "", "bad space.pdf", "CON.pdf"];
    for name in bad_names {
        let err = sink.write(Principal::Agent, "root://exports", name, b"data", false).unwrap_err();
        assert_eq!(err.code(), "report.export_name_invalid", "expected export_name_invalid for {name:?}");
    }
    assert_eq!(fs::read_dir(&tmp.path).unwrap().count(), 0);
}

#[test]
fn ac5_existing_file_requires_overwrite() {
    let tmp = TmpDir::new("ac5");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);
    let sink = ExportSink::new(&resolver);

    let r1 = sink.write(Principal::Agent, "root://exports", "report_a.pdf", b"version-1", false).unwrap();
    assert_eq!(r1.bytes, 9);

    // Without overwrite -> report.export_exists, file unchanged.
    let err = sink.write(Principal::Agent, "root://exports", "report_a.pdf", b"version-2-new", false).unwrap_err();
    assert_eq!(err.code(), "report.export_exists");
    assert_eq!(fs::read(tmp.path.join("report_a.pdf")).unwrap(), b"version-1");

    // With overwrite -> replaced atomically, receipt reflects new bytes & digest.
    let r2 = sink.write(Principal::Agent, "root://exports", "report_a.pdf", b"version-2-new", true).unwrap();
    assert_eq!(r2.bytes, 13);
    assert_eq!(r2.sha256, Digest::from_bytes(Sha256::digest(b"version-2-new").into()));
    assert_ne!(r2.sha256, r1.sha256);
    assert_eq!(fs::read(tmp.path.join("report_a.pdf")).unwrap(), b"version-2-new");
}

#[test]
fn ac6_non_local_root_rejected() {
    let tmp = TmpDir::new("ac6");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_with_locality(RootId::new("remote").unwrap(), RootAccess::Export, &tmp.path, false);
    let sink = ExportSink::new(&resolver);

    let err = sink.write(Principal::Agent, "root://remote", "report_a.pdf", b"payload", false).unwrap_err();
    assert_eq!(err.code(), "report.export_root_not_local");
    assert_eq!(fs::read_dir(&tmp.path).unwrap().count(), 0);
}

#[test]
fn ac7_crash_before_rename_leaves_target_unchanged_and_next_call_sweeps_stale_temp() {
    let tmp = TmpDir::new("ac7");
    let mut resolver = LocalRootResolver::new();
    resolver.insert_local(RootId::new("exports").unwrap(), RootAccess::Export, &tmp.path);

    // Write initial committed target file.
    let base_sink = ExportSink::new(&resolver);
    base_sink.write(Principal::Agent, "root://exports", "report_a.pdf", b"original-good", false).unwrap();

    // Inject crash right after temp flush/sync and BEFORE rename.
    let crashed = Arc::new(AtomicBool::new(false));
    let crashed_flag = Arc::clone(&crashed);
    let crashing_sink = ExportSink::new(&resolver).with_before_rename_hook(Box::new(move |_tmp_name| {
        crashed_flag.store(true, Ordering::SeqCst);
        Err(ExportError::Io { kind: "SimulatedCrashBeforeRename".to_string() })
    }));
    let err =
        crashing_sink.write(Principal::Agent, "root://exports", "report_a.pdf", b"new-uncommitted", true).unwrap_err();
    assert_eq!(err.code(), "report.export_io");
    assert!(crashed.load(Ordering::SeqCst));

    // Target file is completely unchanged, and the stale temp file exists on disk.
    assert_eq!(fs::read(tmp.path.join("report_a.pdf")).unwrap(), b"original-good");
    let names_after_crash: Vec<String> =
        fs::read_dir(&tmp.path).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(
        names_after_crash.iter().any(|n| n.starts_with(".report_a.pdf.tmp-")),
        "expected stale temp file after simulated crash, got {names_after_crash:?}"
    );

    // Next sink call on that root removes the stale temp file and writes cleanly!
    let r = base_sink.write(Principal::Agent, "root://exports", "report_b.pdf", b"clean-next", false).unwrap();
    assert_eq!(r.bytes, 10);
    let mut names_after_next: Vec<String> =
        fs::read_dir(&tmp.path).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names_after_next.sort();
    assert_eq!(names_after_next, vec!["report_a.pdf".to_string(), "report_b.pdf".to_string()]);
}
