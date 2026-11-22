//! Offline kernel artifact lookup chain (spec node `TRUST.REPLAY.KART.LOOKUP`, D-020 option C).
//!
//! Given a kernel manifest (`kernel/manifest.json`, parsed strictly by its published
//! `KRN.MAN.MANI.SCHEMA` v1 format without linking `fid-kci`), [`OfflineArtifactSource::get`]
//! finds the `wasm32-wasip1` reference WASM artifact whose SHA-256 matches the manifest entry,
//! probing sources strictly in order:
//!
//! 1. **In-package** (`Origin::Package`): package member `kernel/<sha256>.wasm` listed in the
//!    package manifest (read through [`PackageView`], e.g. `fid_pkg::reader::PackageReader`).
//! 2. **Bundled** (`Origin::Bundled`): `<bundled_dir>/<sha256>.wasm` shipped alongside
//!    `fid-verify` (typically `kernels/<sha256>.wasm`).
//! 3. **Local cache** (`Origin::Cache`): `<cache_dir>/<sha256>.wasm`, defaulting to the OS
//!    per-user cache location `<os-cache>/fid/kernels/` and overridable via `--kernel-cache`.
//!
//! Every candidate is read in bounded chunks and hashed before acceptance. A SHA-256 mismatch
//! is skipped, recorded as a `candidate_mismatch` [`Note`] naming its [`Origin`], and probing
//! continues with the next source (REQ-TRUST.REPLAY.KART.LOOKUP-002).
//!
//! If no source holds a matching artifact, or the manifest lists no `wasm32-wasip1` artifact,
//! returns [`Unavailable`] (`KERNEL_UNAVAILABLE`) naming the kernel semver, artifact digest,
//! every recorded note, and both hints (`fid verify fetch-kernel` and the reproducible-rebuild
//! recipe; REQ-TRUST.REPLAY.KART.LOOKUP-003).
//!
//! Zero network dependencies, zero file writes, zero `unsafe`, zero panics (D0, CONSTITUTION §8,
//! `REQ-TRUST.VERIFY.CLI-001`).

use crate::select::{ArtifactSource, PackageView};
use fid_canon::{digest_bytes, parse_with_limits, CValue};
use fid_types::Digest;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ constants

/// Target triple searched in `kernel/manifest.json` `artifacts[]` (REQ-001).
pub const WASM_TARGET: &str = "wasm32-wasip1";
/// Stable error code when no matching artifact is available (REQ-003).
pub const CODE_KERNEL_UNAVAILABLE: &str = "KERNEL_UNAVAILABLE";
/// Stable note code when a candidate's SHA-256 disagrees with the manifest (REQ-002).
pub const NOTE_CANDIDATE_MISMATCH: &str = "candidate_mismatch";
/// Stable note code when a source file or member could not be read.
pub const NOTE_READ_ERROR: &str = "read_error";
/// Stable note code when the kernel manifest lists no `wasm32-wasip1` artifact.
pub const NOTE_NO_WASM_ARTIFACT: &str = "no_wasm_artifact";

/// Maximum kernel manifest size accepted by [`KernelManifest::parse`] (4 MiB).
pub const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
/// Maximum JSON nesting depth for `kernel/manifest.json`.
pub const MAX_MANIFEST_DEPTH: usize = 32;
/// Default maximum WASM kernel artifact size read into memory (128 MiB).
pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;
/// Fixed streaming chunk size when reading files from disk (64 KiB).
const READ_CHUNK_BYTES: usize = 64 * 1024;

// ------------------------------------------------------------------ kernel manifest parser

/// Minimal parsed view of `kernel/manifest.json` (`KRN.MAN.MANI.SCHEMA` v1) needed for
/// offline artifact lookup. Parsed directly without linking `fid-kci`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelManifest {
    /// Kernel semantic version (`core.semver`).
    pub semver: String,
    /// SHA-256 digest of the `wasm32-wasip1` artifact, if listed in `artifacts[]`.
    pub wasm32_wasip1_digest: Option<Digest>,
    /// Digest of the canonical manifest document itself (for `select::ArtifactSource` lookup).
    pub manifest_digest: Digest,
}

/// Typed error returned when `kernel/manifest.json` is malformed or exceeds bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    pub field: &'static str,
    pub detail: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "kernel manifest error ({}): {}", self.field, self.detail)
    }
}

impl std::error::Error for ManifestError {}

impl KernelManifest {
    /// Parses `kernel/manifest.json` bytes according to the published `KRN.MAN.MANI.SCHEMA` v1.
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        let v = parse_with_limits(bytes, MAX_MANIFEST_BYTES, MAX_MANIFEST_DEPTH)
            .map_err(|e| ManifestError { field: "manifest", detail: format!("invalid or oversized JSON: {e}") })?;
        let CValue::Obj(top) = &v else {
            return Err(ManifestError { field: "manifest", detail: "top level must be a JSON object".to_string() });
        };
        let Some(CValue::Obj(core)) = top.get("core") else {
            return Err(ManifestError { field: "core", detail: "missing `core` object".to_string() });
        };
        let semver = match core.get("semver") {
            Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
            _ => {
                return Err(ManifestError {
                    field: "core.semver",
                    detail: "missing non-empty `core.semver` string".to_string(),
                });
            }
        };
        let Some(CValue::Arr(artifacts)) = top.get("artifacts") else {
            return Err(ManifestError { field: "artifacts", detail: "missing `artifacts` array".to_string() });
        };

        let mut wasm_digest: Option<Digest> = None;
        for (idx, item) in artifacts.iter().enumerate() {
            let CValue::Obj(ao) = item else {
                return Err(ManifestError {
                    field: "artifacts",
                    detail: format!("artifacts[{idx}] must be an object"),
                });
            };
            let target = match ao.get("target") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => {
                    return Err(ManifestError {
                        field: "artifacts.target",
                        detail: format!("artifacts[{idx}].target must be a string"),
                    });
                }
            };
            if target != WASM_TARGET {
                continue;
            }
            if let Some(CValue::Str(role)) = ao.get("role") {
                if role != "wasm" {
                    return Err(ManifestError {
                        field: "artifacts.role",
                        detail: format!("artifacts[{idx}] for {WASM_TARGET} has unexpected role {role:?}"),
                    });
                }
            }
            let sha_str = match ao.get("sha256") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => {
                    return Err(ManifestError {
                        field: "artifacts.sha256",
                        detail: format!("artifacts[{idx}].sha256 must be a digest string"),
                    });
                }
            };
            let digest = Digest::parse(sha_str).map_err(|_| ManifestError {
                field: "artifacts.sha256",
                detail: format!("artifacts[{idx}].sha256 is not a valid sha256 digest"),
            })?;
            if wasm_digest.replace(digest).is_some() {
                return Err(ManifestError {
                    field: "artifacts",
                    detail: format!("duplicate `{WASM_TARGET}` artifact entry"),
                });
            }
        }

        let manifest_digest = Digest::from_bytes(digest_bytes(bytes).0);
        Ok(KernelManifest { semver, wasm32_wasip1_digest: wasm_digest, manifest_digest })
    }
}

// ------------------------------------------------------------------ lookup types

/// Where a verified WASM kernel artifact was obtained from (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    Package,
    Bundled,
    Cache,
}

impl Origin {
    /// Wire name (`"package"`, `"bundled"`, or `"cache"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Package => "package",
            Origin::Bundled => "bundled",
            Origin::Cache => "cache",
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Diagnostic note recorded when a candidate source was skipped (e.g. `candidate_mismatch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Source that produced the note (or `None` for manifest-level notes such as `no_wasm_artifact`).
    pub origin: Option<Origin>,
    /// Stable note code (`candidate_mismatch`, `read_error`, `no_wasm_artifact`).
    pub code: &'static str,
    /// Human-readable detail.
    pub detail: String,
}

/// Error returned when no offline source holds a digest-matching `wasm32-wasip1` kernel
/// artifact (REQ-TRUST.REPLAY.KART.LOOKUP-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable {
    /// Stable error code (`"KERNEL_UNAVAILABLE"`).
    pub code: &'static str,
    /// Kernel semantic version from the manifest.
    pub semver: String,
    /// Expected `wasm32-wasip1` artifact digest, if present in the manifest.
    pub digest: Option<Box<Digest>>,
    /// Every skipped-candidate note recorded during probing, in lookup order.
    pub notes: Vec<Note>,
    /// Actionable offline/operator recovery hints (`fid verify fetch-kernel` and rebuild recipe).
    pub hints: Vec<String>,
}

impl Unavailable {
    /// Returns the expected `wasm32-wasip1` artifact digest, if present in the manifest.
    pub fn digest(&self) -> Option<Digest> {
        self.digest.as_deref().copied()
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.digest() {
            Some(d) => write!(
                f,
                "{}: kernel {} ({}) not found in package, bundled dir, or local cache",
                self.code, self.semver, d
            ),
            None => write!(f, "{}: kernel {} lists no {WASM_TARGET} artifact", self.code, self.semver),
        }
    }
}

impl std::error::Error for Unavailable {}

// ------------------------------------------------------------------ cache path resolution

/// Resolves the default OS per-user kernel cache directory (`<os-cache>/fid/kernels`,
/// REQ-TRUST.REPLAY.KART.LOOKUP-005). Read-only; never creates directories.
pub fn default_cache_dir() -> Option<PathBuf> {
    default_cache_dir_from_env(
        std::env::var_os("XDG_CACHE_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
        std::env::var_os("LOCALAPPDATA").as_deref(),
    )
}

/// Pure helper behind [`default_cache_dir`] for deterministic unit testing.
pub fn default_cache_dir_from_env(
    xdg_cache_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
    local_app_data: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let _ = (xdg_cache_home, home);
        local_app_data.filter(|v| !v.is_empty()).map(|base| PathBuf::from(base).join("fid").join("kernels"))
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (xdg_cache_home, local_app_data);
        home.filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join("Library").join("Caches").join("fid").join("kernels"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = local_app_data;
        if let Some(xdg) = xdg_cache_home.filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(xdg).join("fid").join("kernels"));
        }
        home.filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".cache").join("fid").join("kernels"))
    }
}

/// Package member path for a WASM artifact (`kernel/<64-hex>.wasm`).
pub fn package_member_path(digest: &Digest) -> String {
    format!("kernel/{}.wasm", digest.hex())
}

/// Filename inside the bundled or cache directory (`<64-hex>.wasm`).
pub fn artifact_filename(digest: &Digest) -> String {
    format!("{}.wasm", digest.hex())
}

// ------------------------------------------------------------------ OfflineArtifactSource

/// Configuration for the offline kernel artifact lookup chain (D-020 option C).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupConfig {
    /// Directory of bundled kernel artifacts shipped beside `fid-verify` (files `<sha256>.wasm`).
    pub bundled_dir: Option<PathBuf>,
    /// Local kernel cache directory (`--kernel-cache <dir>`, or [`default_cache_dir`]).
    pub cache_dir: Option<PathBuf>,
    /// Maximum accepted artifact byte length (defaults to [`DEFAULT_MAX_ARTIFACT_BYTES`]).
    pub max_artifact_bytes: u64,
}

impl Default for LookupConfig {
    fn default() -> Self {
        Self { bundled_dir: None, cache_dir: default_cache_dir(), max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES }
    }
}

/// Result of a successful offline lookup: verified artifact bytes, origin, and any notes
/// recorded while skipping earlier corrupt candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundArtifact {
    pub bytes: Vec<u8>,
    pub origin: Origin,
    pub notes: Vec<Note>,
}

/// Offline `ArtifactSource` probing `package -> bundled -> cache` without network or writes.
#[derive(Debug, Clone)]
pub struct OfflineArtifactSource {
    config: LookupConfig,
}

impl OfflineArtifactSource {
    /// Creates an offline source with the given directories and byte cap.
    pub fn new(config: LookupConfig) -> Self {
        Self { config }
    }

    /// Creates an offline source with explicit bundled and cache directories (useful for tests
    /// and CLI `--kernel-cache`).
    pub fn with_dirs(bundled_dir: Option<PathBuf>, cache_dir: Option<PathBuf>) -> Self {
        Self { config: LookupConfig { bundled_dir, cache_dir, max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES } }
    }

    /// Looks up the `wasm32-wasip1` artifact named by `manifest`, probing `package` (if
    /// provided), then `bundled`, then `cache`. Returns `(bytes, origin)` on the first
    /// digest-matching candidate, or [`Unavailable`] (`KERNEL_UNAVAILABLE`).
    pub fn get(
        &self,
        manifest: &KernelManifest,
        package: Option<&mut dyn PackageView>,
    ) -> Result<(Vec<u8>, Origin), Unavailable> {
        self.get_with_notes(manifest, package).map(|found| (found.bytes, found.origin))
    }

    /// Same as [`Self::get`], also returning any `candidate_mismatch` notes from skipped
    /// earlier sources when a later source succeeded.
    pub fn get_with_notes(
        &self,
        manifest: &KernelManifest,
        package: Option<&mut dyn PackageView>,
    ) -> Result<FoundArtifact, Unavailable> {
        let mut notes: Vec<Note> = Vec::new();
        let Some(expected_digest) = manifest.wasm32_wasip1_digest else {
            notes.push(Note {
                origin: None,
                code: NOTE_NO_WASM_ARTIFACT,
                detail: format!("kernel manifest {} lists no {WASM_TARGET} artifact", manifest.semver),
            });
            return Err(build_unavailable(&manifest.semver, None, notes));
        };

        // 1. Package member `kernel/<sha256>.wasm` (must be listed in package manifest).
        if let Some(pkg) = package {
            let member = package_member_path(&expected_digest);
            let is_listed = pkg.members().iter().any(|m| m.path == member);
            if is_listed {
                match pkg.read_member(&member, self.config.max_artifact_bytes) {
                    Ok(Some(bytes)) => {
                        let actual = Digest::from_bytes(digest_bytes(&bytes).0);
                        if actual == expected_digest {
                            return Ok(FoundArtifact { bytes, origin: Origin::Package, notes });
                        }
                        notes.push(mismatch_note(Origin::Package, expected_digest, actual));
                    }
                    Ok(None) => {
                        notes.push(Note {
                            origin: Some(Origin::Package),
                            code: NOTE_READ_ERROR,
                            detail: format!("listed package member `{member}` missing in container"),
                        });
                    }
                    Err(e) => {
                        // A corrupted member fails verification inside fid-pkg::reader too:
                        // surface digest/integrity mismatches as `candidate_mismatch` so fallback
                        // continues cleanly per REQ-002.
                        let code = if e.code.contains("digest") || e.code.contains("corrupt") {
                            NOTE_CANDIDATE_MISMATCH
                        } else {
                            NOTE_READ_ERROR
                        };
                        notes.push(Note {
                            origin: Some(Origin::Package),
                            code,
                            detail: format!("{}: {}", e.code, e.detail),
                        });
                    }
                }
            }
        }

        let filename = artifact_filename(&expected_digest);

        // 2. Bundled directory (`<bundled_dir>/<sha256>.wasm`).
        if let Some(bundled_dir) = &self.config.bundled_dir {
            let path = bundled_dir.join(&filename);
            if let Some(found) =
                probe_file(&path, Origin::Bundled, expected_digest, self.config.max_artifact_bytes, &mut notes)
            {
                return Ok(FoundArtifact { bytes: found, origin: Origin::Bundled, notes });
            }
        }

        // 3. Local cache directory (`<cache_dir>/<sha256>.wasm`).
        if let Some(cache_dir) = &self.config.cache_dir {
            let path = cache_dir.join(&filename);
            if let Some(found) =
                probe_file(&path, Origin::Cache, expected_digest, self.config.max_artifact_bytes, &mut notes)
            {
                return Ok(FoundArtifact { bytes: found, origin: Origin::Cache, notes });
            }
        }

        Err(build_unavailable(&manifest.semver, Some(expected_digest), notes))
    }
}

/// Bridges [`OfflineArtifactSource`] to [`crate::select::ArtifactSource`] for a parsed
/// [`KernelManifest`] (checking bundled + cache, or package + bundled + cache when the
/// caller pre-resolves availability).
pub struct ManifestArtifactProbe<'a> {
    source: &'a OfflineArtifactSource,
    manifest: &'a KernelManifest,
}

impl<'a> ManifestArtifactProbe<'a> {
    /// Creates an [`ArtifactSource`] probe for `manifest` backed by `source` (bundled + cache).
    pub const fn new(source: &'a OfflineArtifactSource, manifest: &'a KernelManifest) -> Self {
        Self { source, manifest }
    }
}

impl ArtifactSource for ManifestArtifactProbe<'_> {
    fn is_available(&self, kernel_manifest_digest: &Digest) -> bool {
        if kernel_manifest_digest != &self.manifest.manifest_digest {
            return false;
        }
        self.source.get(self.manifest, None).is_ok()
    }
}

// ------------------------------------------------------------------ helpers

fn probe_file(path: &Path, origin: Origin, expected: Digest, max_bytes: u64, notes: &mut Vec<Note>) -> Option<Vec<u8>> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            notes.push(Note {
                origin: Some(origin),
                code: NOTE_READ_ERROR,
                detail: format!("{}: {e}", path.display()),
            });
            return None;
        }
    };

    if let Ok(meta) = file.metadata() {
        if !meta.is_file() {
            notes.push(Note {
                origin: Some(origin),
                code: NOTE_READ_ERROR,
                detail: format!("{}: not a regular file", path.display()),
            });
            return None;
        }
        if meta.len() > max_bytes {
            notes.push(Note {
                origin: Some(origin),
                code: NOTE_CANDIDATE_MISMATCH,
                detail: format!("{}: exceeds max artifact size ({max_bytes} bytes)", path.display()),
            });
            return None;
        }
    }

    // Stream-read in fixed 64 KiB chunks up to max_bytes.
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let next_len = (buf.len() as u64).saturating_add(n as u64);
                if next_len > max_bytes {
                    notes.push(Note {
                        origin: Some(origin),
                        code: NOTE_CANDIDATE_MISMATCH,
                        detail: format!("{}: exceeds max artifact size ({max_bytes} bytes)", path.display()),
                    });
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) => {
                notes.push(Note {
                    origin: Some(origin),
                    code: NOTE_READ_ERROR,
                    detail: format!("{}: {e}", path.display()),
                });
                return None;
            }
        }
    }

    let actual = Digest::from_bytes(digest_bytes(&buf).0);
    if actual == expected {
        Some(buf)
    } else {
        notes.push(mismatch_note(origin, expected, actual));
        None
    }
}

fn mismatch_note(origin: Origin, expected: Digest, actual: Digest) -> Note {
    Note {
        origin: Some(origin),
        code: NOTE_CANDIDATE_MISMATCH,
        detail: format!("{} candidate SHA-256 {} != manifest {}", origin.as_str(), actual, expected),
    }
}

fn build_unavailable(semver: &str, digest: Option<Digest>, notes: Vec<Note>) -> Unavailable {
    let fetch_hint = match digest {
        Some(d) => format!("Run `fid verify fetch-kernel {d}` to populate the local kernel cache."),
        None => "Run `fid verify fetch-kernel <digest>` once a wasm32-wasip1 artifact digest is published.".to_string(),
    };
    let rebuild_hint = format!(
        "Or rebuild kernel {semver} offline from source using the published reproducible-build recipe (KRN.MAN.REPRO)."
    );
    Unavailable {
        code: CODE_KERNEL_UNAVAILABLE,
        semver: semver.to_string(),
        digest: digest.map(Box::new),
        notes,
        hints: vec![fetch_hint, rebuild_hint],
    }
}
