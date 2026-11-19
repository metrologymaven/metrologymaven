//! Cross-check of a kernel manifest against the WASM reference artifact and the runtime
//! settings (how `KRN.MAN.WASM` relates to `KRN.MAN.MANI.SCHEMA`).
//!
//! In order, failing closed at the first problem:
//!
//! 1. the manifest is read with [`KernelManifest::parse_canonical`] (strict: its bytes must be
//!    its own canonical JSON);
//! 2. the module passes [`crate::module::parse`] and the import check against the allowlist;
//! 3. the manifest lists an artifact with role [`WASM_ROLE`] and target [`WASM_TARGET`]
//!    (unique, because the schema keeps artifacts strictly sorted by `(role, target)`);
//! 4. that artifact's `sha256` equals the SHA-256 of the module bytes;
//! 5. `replay_runtime.config_digest` equals the settings' [`RuntimeConfig::config_digest`];
//! 6. `replay_runtime.version` equals the pinned `wasmtime_version`.

use std::fmt;

use fid_kci::kci::{sha256_digest, Digest};
use fid_kci::manifest::{KernelManifest, ManifestError};

use crate::allow::{self, Allowlist};
use crate::module::{self, WasmError};
use crate::runtime::RuntimeConfig;

/// Artifact role of the WASM reference module.
pub const WASM_ROLE: &str = "wasm";
/// Artifact target of the WASM reference module.
pub const WASM_TARGET: &str = "wasm32-wasip1";

/// Cross-check failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrossCheckError {
    Manifest(ManifestError),
    Module(WasmError),
    /// The module has imports the allowlist does not allow (`module name` each).
    ImportsDenied(Vec<String>),
    ArtifactMissing,
    ArtifactDigestMismatch {
        claimed: Box<Digest>,
        computed: Box<Digest>,
    },
    ConfigDigestMismatch {
        claimed: Box<Digest>,
        computed: Box<Digest>,
    },
    RuntimeVersionMismatch {
        claimed: String,
        pinned: String,
    },
}

impl CrossCheckError {
    /// Stable code; manifest and module errors are prefixed `manifest:` and `module:`.
    pub fn code(&self) -> String {
        match self {
            CrossCheckError::Manifest(e) => format!("manifest:{}", e.code()),
            CrossCheckError::Module(e) => format!("module:{}", e.code()),
            CrossCheckError::ImportsDenied(_) => "imports_denied".to_string(),
            CrossCheckError::ArtifactMissing => "artifact_missing".to_string(),
            CrossCheckError::ArtifactDigestMismatch { .. } => "artifact_digest_mismatch".to_string(),
            CrossCheckError::ConfigDigestMismatch { .. } => "config_digest_mismatch".to_string(),
            CrossCheckError::RuntimeVersionMismatch { .. } => "runtime_version_mismatch".to_string(),
        }
    }
}

impl fmt::Display for CrossCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            CrossCheckError::Manifest(e) => write!(f, "{e}"),
            CrossCheckError::Module(e) => write!(f, "module: {e}"),
            CrossCheckError::ImportsDenied(v) => write!(f, "{code}: {}", v.join(", ")),
            CrossCheckError::ArtifactMissing => write!(f, "{code}: no {WASM_ROLE} artifact for {WASM_TARGET}"),
            CrossCheckError::ArtifactDigestMismatch { claimed, computed }
            | CrossCheckError::ConfigDigestMismatch { claimed, computed } => {
                write!(f, "{code}: claimed {claimed}, computed {computed}")
            }
            CrossCheckError::RuntimeVersionMismatch { claimed, pinned } => {
                write!(f, "{code}: manifest {claimed}, pinned {pinned}")
            }
        }
    }
}

impl std::error::Error for CrossCheckError {}

/// Runs the cross-check (see the module docs) and returns the manifest digest on success.
pub fn cross_check(
    manifest: &[u8],
    module_bytes: &[u8],
    allow: &Allowlist,
    runtime: &RuntimeConfig,
) -> Result<Digest, CrossCheckError> {
    let m = KernelManifest::parse_canonical(manifest).map_err(CrossCheckError::Manifest)?;
    let parsed = module::parse(module_bytes).map_err(CrossCheckError::Module)?;
    let report = allow::check(&parsed, allow);
    if !report.passed() {
        return Err(CrossCheckError::ImportsDenied(
            report.denied().iter().map(|i| format!("{} {}", i.module, i.name)).collect(),
        ));
    }
    let art = m
        .artifacts()
        .iter()
        .find(|a| a.role == WASM_ROLE && a.target == WASM_TARGET)
        .ok_or(CrossCheckError::ArtifactMissing)?;
    let computed = sha256_digest(module_bytes);
    if art.sha256 != computed {
        return Err(CrossCheckError::ArtifactDigestMismatch {
            claimed: Box::new(art.sha256),
            computed: Box::new(computed),
        });
    }
    let rr = &m.core().replay_runtime;
    let config = runtime.config_digest();
    if rr.config_digest != config {
        return Err(CrossCheckError::ConfigDigestMismatch {
            claimed: Box::new(rr.config_digest),
            computed: Box::new(config),
        });
    }
    if rr.version != runtime.wasmtime_version {
        return Err(CrossCheckError::RuntimeVersionMismatch {
            claimed: rr.version.clone(),
            pinned: runtime.wasmtime_version.clone(),
        });
    }
    Ok(m.manifest_digest())
}
