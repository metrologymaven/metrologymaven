//! Release-manifest provenance (AC-6 of `UNC.MODEL.GRAPH.STORE`).
//!
//! A model is validated only against the kernel's **release manifest**, bound to the running
//! kernel by [`bind_release_manifest`]. The steps, each fail-closed, in this order:
//!
//! 1. Ask the running kernel for its manifest core digest with the reserved KCI query
//!    `kernel.manifest.core_digest@1.0.0` ([`running_core_digest`]): no inputs, empty
//!    parameters, no seed. The answer is the ASCII `sha256:<hex>` the artifact was built with
//!    (a compiled-in constant; `KRN.MAN.MANI.GEN`). A kernel without that constant (every
//!    build until the first release: `fid-kernel-wasm`'s committed `MANIFEST_CORE_DIGEST` is
//!    `None`) answers `manifest_core_digest_unset`, which is `kernel_manifest_unset` here:
//!    nothing can be validated or stored. Any other kernel error, a result for another
//!    algorithm, with inputs, a seed, an output that does not hash to its digest, or an output
//!    that is not a well-formed digest is `kernel_unavailable`.
//! 2. Read the release manifest bytes from the host's [`ReleaseManifestSource`] (the manifest
//!    file shipped with the running kernel artifact; `kernel_manifest_unavailable` on failure).
//! 3. Parse them with [`KernelManifest::parse_canonical`], the strict reader: the bytes must be
//!    exactly the canonical encoding (`kernel_manifest_invalid` with the manifest error code).
//! 4. The manifest's core digest must equal the digest from step 1
//!    (`kernel_manifest_mismatch`).
//!
//! The result, a [`ReleaseManifest`], has no other constructor, so a command cannot validate
//! against a manifest that skipped any step, and no command parameter reaches this module:
//! the commands' parameter schemas have no manifest member and their readers refuse unknown
//! keys.

use super::{echo, StoreError};
use fid_kci::kci::sha256_digest;
use fid_kci::{
    AlgorithmId, AlgorithmTable, KernelCall, KernelError, KernelManifest, KernelResult, MANIFEST_CORE_DIGEST_QUERY,
};
use fid_types::Digest;

/// The running kernel, as the host reaches it: the assembled [`AlgorithmTable`] of this build
/// (implemented here through [`fid_kci::evaluate`]), or an artifact answering the same calls.
pub trait RunningKernel {
    /// Evaluates one kernel call.
    fn evaluate(&self, call: &KernelCall) -> Result<KernelResult, KernelError>;
}

impl RunningKernel for AlgorithmTable {
    fn evaluate(&self, call: &KernelCall) -> Result<KernelResult, KernelError> {
        fid_kci::evaluate(self, call)
    }
}

/// Where the host reads the release manifest from: the manifest file of the running
/// kernel's release. Never a command parameter.
pub trait ReleaseManifestSource {
    /// The bytes of the release manifest file.
    fn release_manifest(&self) -> Result<Vec<u8>, String>;
}

/// The manifest core digest the running kernel reports (step 1 of the module docs).
pub fn running_core_digest(kernel: &dyn RunningKernel) -> Result<Digest, StoreError> {
    let query = AlgorithmId::parse(MANIFEST_CORE_DIGEST_QUERY)
        .map_err(|_| StoreError::Internal { reason: "the manifest query ID does not parse" })?;
    let call = KernelCall { algorithm: query.clone(), params: Vec::new(), seed: None, inputs: Vec::new() };
    let result = match kernel.evaluate(&call) {
        Ok(r) => r,
        Err(KernelError::ManifestCoreDigestUnset) => return Err(StoreError::KernelManifestUnset),
        Err(e) => return Err(StoreError::KernelUnavailable { code: e.code() }),
    };
    let bad = StoreError::KernelUnavailable { code: "bad_self_report" };
    if *result.algorithm() != query
        || !result.input_digests().is_empty()
        || result.seed().is_some()
        || *result.output_digest() != sha256_digest(result.output())
    {
        return Err(bad);
    }
    let text = std::str::from_utf8(result.output()).map_err(|_| bad.clone())?;
    Digest::parse(text).map_err(|_| bad)
}

/// A release manifest bound to the running kernel. Only [`bind_release_manifest`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseManifest {
    manifest: KernelManifest,
    digest: Digest,
}

impl ReleaseManifest {
    /// The manifest.
    pub fn manifest(&self) -> &KernelManifest {
        &self.manifest
    }

    /// Its digest (`kernel_manifest_digest`): SHA-256 over its canonical bytes, which are the
    /// bytes read.
    pub fn manifest_digest(&self) -> Digest {
        self.digest
    }

    /// Its core digest, equal to the one the running kernel reports.
    pub fn core_digest(&self) -> Digest {
        *self.manifest.core_digest()
    }
}

/// Binds the release manifest to the running kernel (steps 1 to 4 of the module docs).
pub fn bind_release_manifest(
    source: &dyn ReleaseManifestSource,
    kernel: &dyn RunningKernel,
) -> Result<ReleaseManifest, StoreError> {
    let running = running_core_digest(kernel)?;
    let bytes = source.release_manifest().map_err(|d| StoreError::KernelManifestUnavailable { detail: echo(&d) })?;
    let manifest =
        KernelManifest::parse_canonical(&bytes).map_err(|e| StoreError::KernelManifestInvalid { code: e.code() })?;
    let core = *manifest.core_digest();
    if core != running {
        return Err(StoreError::KernelManifestMismatch { running, manifest: core });
    }
    let digest = manifest.manifest_digest();
    Ok(ReleaseManifest { manifest, digest })
}
