//! Integration tests for spec node `TRUST.REPLAY.KART.LOOKUP` (AC-1..AC-4,
//! REQ-001..REQ-005, real `fid_pkg::reader::PackageReader` integration, fallback across
//! corrupted candidates, `default_cache_dir_from_env`, and 1,000-mutant AC-H2 check).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_pkg::reader::PackageReader;
use fid_types::Digest;
use fid_vreplay::kart::{
    artifact_filename, default_cache_dir_from_env, KernelManifest, ManifestArtifactProbe, OfflineArtifactSource,
    Origin, CODE_KERNEL_UNAVAILABLE, NOTE_CANDIDATE_MISMATCH, NOTE_NO_WASM_ARTIFACT,
};
use fid_vreplay::select::{ArtifactSource, MemberInfo, PackageError, PackageView};
use std::ffi::OsStr;
use std::io::Cursor;
use std::path::PathBuf;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/replay/kart");

struct PkgAdapter(PackageReader<Cursor<Vec<u8>>>);

impl PackageView for PkgAdapter {
    fn evidence_tier(&self) -> String {
        self.0.manifest().evidence_tier.as_str().to_string()
    }
    fn kernel_manifest_digest(&self) -> Digest {
        self.0.manifest().kernel_manifest_digest
    }
    fn members(&self) -> Vec<MemberInfo> {
        self.0
            .manifest()
            .members
            .iter()
            .map(|m| MemberInfo { path: m.path.as_str().to_string(), sha256: m.sha256 })
            .collect()
    }
    fn read_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, PackageError> {
        self.0
            .read_member(path, max_bytes)
            .map_err(|e| PackageError { code: e.code().to_string(), detail: e.to_string() })
    }
}

fn read_fix(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn open_pkg(name: &str) -> PkgAdapter {
    PkgAdapter(PackageReader::open(Cursor::new(read_fix(name))).unwrap_or_else(|e| panic!("{name}: {e}")))
}

fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid-vreplay-kart-{tag}-{n}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

#[test]
fn ac1_embedded_package_kernel_wins_with_origin_package() {
    let stub = read_fix("stub-kernel.wasm");
    let manifest = KernelManifest::parse(&read_fix("kernel-manifest-ok.json")).expect("parse manifest");
    let mut pkg = open_pkg("points-with-kernel.fidpkg");
    let empty_bundled = temp_dir("ac1-bundled");
    let empty_cache = temp_dir("ac1-cache");

    let source = OfflineArtifactSource::with_dirs(Some(empty_bundled.clone()), Some(empty_cache.clone()));
    let (bytes, origin) = source.get(&manifest, Some(&mut pkg)).expect("ac1 lookup");
    assert_eq!(origin, Origin::Package);
    assert_eq!(bytes, stub);

    // Zero writes to cache or bundled directories (REQ-004)
    assert_eq!(std::fs::read_dir(&empty_cache).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(&empty_bundled).unwrap().count(), 0);

    let _ = std::fs::remove_dir_all(&empty_bundled);
    let _ = std::fs::remove_dir_all(&empty_cache);
}

#[test]
fn ac2_no_kernel_member_falls_back_to_bundled_then_cache() {
    let stub = read_fix("stub-kernel.wasm");
    let manifest = KernelManifest::parse(&read_fix("kernel-manifest-ok.json")).expect("parse manifest");
    let digest = manifest.wasm32_wasip1_digest.expect("wasm digest");
    let fname = artifact_filename(&digest);

    let bundled = temp_dir("ac2-bundled");
    let cache = temp_dir("ac2-cache");
    std::fs::write(bundled.join(&fname), &stub).expect("write bundled");
    std::fs::write(cache.join(&fname), &stub).expect("write cache");

    // Both bundled and cache have it -> bundled wins (package -> bundled -> cache order)
    let mut pkg = open_pkg("points-no-kernel.fidpkg");
    let src_both = OfflineArtifactSource::with_dirs(Some(bundled.clone()), Some(cache.clone()));
    let (bytes, origin) = src_both.get(&manifest, Some(&mut pkg)).expect("bundled lookup");
    assert_eq!(origin, Origin::Bundled);
    assert_eq!(bytes, stub);

    // Bridge to select::ArtifactSource reports available
    let probe = ManifestArtifactProbe::new(&src_both, &manifest);
    assert!(probe.is_available(&manifest.manifest_digest));

    // Only cache has it -> origin is Cache
    let empty_bundled = temp_dir("ac2-empty-bundled");
    let src_cache = OfflineArtifactSource::with_dirs(Some(empty_bundled.clone()), Some(cache.clone()));
    let (bytes2, origin2) = src_cache.get(&manifest, Some(&mut pkg)).expect("cache lookup");
    assert_eq!(origin2, Origin::Cache);
    assert_eq!(bytes2, stub);

    let _ = std::fs::remove_dir_all(&bundled);
    let _ = std::fs::remove_dir_all(&empty_bundled);
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn ac3_corrupted_cache_returns_unavailable_with_candidate_mismatch_and_fallback_skips_corrupt_bundled() {
    let stub = read_fix("stub-kernel.wasm");
    let manifest = KernelManifest::parse(&read_fix("kernel-manifest-ok.json")).expect("parse manifest");
    let digest = manifest.wasm32_wasip1_digest.expect("wasm digest");
    let fname = artifact_filename(&digest);

    let mut flipped = stub.clone();
    flipped[0] ^= 0x01;

    // AC-3: corrupted cache file and no other source -> KERNEL_UNAVAILABLE with candidate_mismatch for cache
    let cache = temp_dir("ac3-cache");
    std::fs::write(cache.join(&fname), &flipped).expect("write corrupt cache");
    let mut pkg = open_pkg("points-no-kernel.fidpkg");
    let src = OfflineArtifactSource::with_dirs(None, Some(cache.clone()));
    let err = src.get(&manifest, Some(&mut pkg)).unwrap_err();
    assert_eq!(err.code, CODE_KERNEL_UNAVAILABLE);
    assert_eq!(err.semver, "0.1.0");
    assert_eq!(err.digest(), Some(digest));
    assert!(
        err.notes.iter().any(|n| n.origin == Some(Origin::Cache) && n.code == NOTE_CANDIDATE_MISMATCH),
        "expected candidate_mismatch note for cache, got {:?}",
        err.notes
    );

    // REQ-002: corrupted bundled candidate is skipped with note and valid cache succeeds
    let bad_bundled = temp_dir("ac3-bad-bundled");
    let good_cache = temp_dir("ac3-good-cache");
    std::fs::write(bad_bundled.join(&fname), &flipped).expect("write bad bundled");
    std::fs::write(good_cache.join(&fname), &stub).expect("write good cache");
    let src_fb = OfflineArtifactSource::with_dirs(Some(bad_bundled.clone()), Some(good_cache.clone()));
    let found = src_fb.get_with_notes(&manifest, Some(&mut pkg)).expect("fallback to cache");
    assert_eq!(found.origin, Origin::Cache);
    assert_eq!(found.bytes, stub);
    assert_eq!(found.notes.len(), 1);
    assert_eq!(found.notes[0].origin, Some(Origin::Bundled));
    assert_eq!(found.notes[0].code, NOTE_CANDIDATE_MISMATCH);

    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_dir_all(&bad_bundled);
    let _ = std::fs::remove_dir_all(&good_cache);
}

#[test]
fn ac4_missing_kernel_9_9_9_and_no_wasm_artifact_return_unavailable_with_both_hints() {
    let m999 = KernelManifest::parse(&read_fix("kernel-manifest-9.9.9.json")).expect("parse 9.9.9");
    let d999 = m999.wasm32_wasip1_digest.expect("9.9.9 digest");
    let empty = temp_dir("ac4-empty");
    let src = OfflineArtifactSource::with_dirs(Some(empty.clone()), Some(empty.clone()));

    let err = src.get(&m999, None).unwrap_err();
    assert_eq!(err.code, CODE_KERNEL_UNAVAILABLE);
    assert_eq!(err.semver, "9.9.9");
    assert_eq!(err.digest(), Some(d999));
    let hints_joined = err.hints.join("\n");
    assert!(hints_joined.contains("fid verify fetch-kernel"), "missing fetch-kernel hint: {hints_joined}");
    assert!(hints_joined.contains(d999.as_str()), "hint must name digest: {hints_joined}");
    assert!(
        hints_joined.contains("reproducible-build") || hints_joined.contains("KRN.MAN.REPRO"),
        "missing rebuild recipe hint: {hints_joined}"
    );

    // Manifest with no wasm32-wasip1 artifact -> KERNEL_UNAVAILABLE naming semver and both hints
    let m_no_wasm = KernelManifest::parse(&read_fix("kernel-manifest-no-wasm.json")).expect("parse no-wasm");
    let err2 = src.get(&m_no_wasm, None).unwrap_err();
    assert_eq!(err2.code, CODE_KERNEL_UNAVAILABLE);
    assert_eq!(err2.semver, "0.2.0");
    assert_eq!(err2.digest(), None);
    assert!(err2.notes.iter().any(|n| n.code == NOTE_NO_WASM_ARTIFACT));
    assert!(err2.hints.iter().any(|h| h.contains("fid verify fetch-kernel")));

    // Default cache dir resolution helper (REQ-005)
    let resolved = default_cache_dir_from_env(
        Some(OsStr::new("/tmp/xdg-cache")),
        Some(OsStr::new("/home/user")),
        Some(OsStr::new("C:\\Users\\user\\AppData\\Local")),
    )
    .expect("cache dir");
    assert!(resolved.ends_with(PathBuf::from("fid").join("kernels")));

    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn ac_h2_thousand_corrupted_candidates_zero_false_accepts() {
    let stub = read_fix("stub-kernel.wasm");
    let manifest = KernelManifest::parse(&read_fix("kernel-manifest-ok.json")).expect("parse manifest");
    let digest = manifest.wasm32_wasip1_digest.expect("wasm digest");
    let fname = artifact_filename(&digest);

    let cache = temp_dir("ach2-faults");
    let src = OfflineArtifactSource::with_dirs(None, Some(cache.clone()));
    let path = cache.join(&fname);

    // 1,000 deterministic mutations (byte flips, truncations, appends): 0 accepted!
    for i in 0..1_000usize {
        let mut mutated = stub.clone();
        match i % 3 {
            0 => {
                let pos = i % mutated.len();
                let bit = 1u8 << ((i / mutated.len()) % 8);
                mutated[pos] ^= bit;
            }
            1 => {
                let keep = i % mutated.len();
                mutated.truncate(keep);
            }
            _ => {
                mutated.push((i & 0xff) as u8);
            }
        }
        std::fs::write(&path, &mutated).expect("write mutant");
        let err = src.get(&manifest, None).unwrap_err();
        assert_eq!(err.code, CODE_KERNEL_UNAVAILABLE, "mutant {i} was falsely accepted!");
        assert!(err.notes.iter().any(|n| n.code == NOTE_CANDIDATE_MISMATCH));
    }

    let _ = std::fs::remove_dir_all(&cache);
}
