//! Reusable keystore conformance suite (REQ-TRUST.SIGN.KEYSTORE.TRAIT-003).
//!
//! Every keystore backend (`MemKeyStore`, Windows CNG, macOS Keychain/Secure Enclave, Linux
//! PKCS#11/wrapped) runs this suite in CI:
//! 1. Create a P-256 key (`prefer_hardware = true`).
//! 2. Verify `list()` includes it and `public_key()` matches its fingerprint.
//! 3. Open an opaque `KeyHandle`, sign a DSSE PAE via a single-use `UvRelease`, and verify the
//!    envelope and raw signature with `fid-sign-core`.
//! 4. Assert the single-use `ReleasedSigner` refuses a second sign call, and that a UV release
//!    bound to a second, real key (fingerprint mismatch) is refused by both signing paths.
//! 5. Delete the key and assert `list()`, `open()`, `public_key()` and `delete()` report it gone,
//!    and that the handle opened before the delete can no longer sign by either path.

use crate::api::{KeyInfo, KeyStore, KeyStoreError, UvRelease};
use fid_sign_core::{pae, sign_envelope, verify, Alg, Envelope, Fingerprint, Signer, IN_TOTO_PAYLOAD_TYPE};
use std::fmt;

/// Sample DSSE payload signed during conformance verification.
const CONFORMANCE_PAYLOAD: &[u8] = b"{\"suite\":\"TRUST.SIGN.KEYSTORE.TRAIT\",\"step\":\"conformance\"}";
/// Label applied to the conformance P-256 key.
pub const CONFORMANCE_LABEL: &str = "fid-keystore-conformance-p256";
/// Label applied to the second P-256 key used for the mismatched-fingerprint refusal.
pub const CONFORMANCE_OTHER_LABEL: &str = "fid-keystore-conformance-p256-other";

/// Outcome of a passing conformance run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceReport {
    /// Metadata of the P-256 key created (and subsequently deleted) by the suite.
    pub created: KeyInfo,
    /// DSSE envelope signed through the handle and verified with `fid-sign-core`.
    pub envelope: Envelope,
}

/// Failure in a conformance step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceError {
    pub step: &'static str,
    pub detail: String,
}

impl fmt::Display for ConformanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "conformance step `{}` failed: {}", self.step, self.detail)
    }
}

impl std::error::Error for ConformanceError {}

fn fail(step: &'static str, detail: impl Into<String>) -> ConformanceError {
    ConformanceError { step, detail: detail.into() }
}

/// Runs the full conformance suite using a test UV release (requires `test-backend`).
#[cfg(feature = "test-backend")]
pub fn run_conformance<K: KeyStore>(store: &K) -> Result<ConformanceReport, ConformanceError> {
    run_conformance_with_uv(store, |fp| UvRelease::test_release(fp.clone()))
}

/// Runs the full conformance suite with a caller-supplied UV release provider.
pub fn run_conformance_with_uv<K, F>(store: &K, release_for: F) -> Result<ConformanceReport, ConformanceError>
where
    K: KeyStore,
    F: Fn(&Fingerprint) -> UvRelease,
{
    run_conformance_with_hook(store, release_for, |_| Ok(()))
}

/// Runs the full conformance suite, invoking `after_create` right after step 1 (create) and
/// before step 5 (delete) so tests (e.g. AC-4) can inspect the sidecar file on disk.
pub fn run_conformance_with_hook<K, F, H>(
    store: &K,
    release_for: F,
    after_create: H,
) -> Result<ConformanceReport, ConformanceError>
where
    K: KeyStore,
    F: Fn(&Fingerprint) -> UvRelease,
    H: FnOnce(&KeyInfo) -> Result<(), String>,
{
    // 1. Create a P-256 key.
    let info =
        store.create(Alg::EcdsaP256Sha256, CONFORMANCE_LABEL, true).map_err(|e| fail("create", e.to_string()))?;
    if info.alg != Alg::EcdsaP256Sha256 {
        return Err(fail("create", format!("expected P-256, got {}", info.alg.name())));
    }
    if info.label != CONFORMANCE_LABEL {
        return Err(fail("create", format!("expected label {CONFORMANCE_LABEL:?}, got {:?}", info.label)));
    }
    let pubkey = store.public_key(&info.fingerprint).map_err(|e| fail("public_key", e.to_string()))?;
    if pubkey.algorithm() != Alg::EcdsaP256Sha256 {
        return Err(fail("public_key", format!("public key algorithm mismatch: {}", pubkey.algorithm().name())));
    }
    if pubkey.fingerprint() != info.fingerprint {
        return Err(fail("public_key", "public key fingerprint does not match KeyInfo.fingerprint"));
    }

    after_create(&info).map_err(|detail| fail("after_create_hook", detail))?;

    // 2. List keys and confirm the new key is present.
    let listed = store.list().map_err(|e| fail("list", e.to_string()))?;
    if !listed.iter().any(|k| k == &info) {
        return Err(fail("list", format!("created key {} missing from list()", info.fingerprint)));
    }

    // 3. Open opaque handle and sign a DSSE PAE with a single-use UV release.
    let handle = store.open(&info.fingerprint).map_err(|e| fail("open", e.to_string()))?;
    if handle.algorithm() != Alg::EcdsaP256Sha256 || handle.public_key() != pubkey {
        return Err(fail("open", "handle algorithm or public key disagrees with store"));
    }
    let released = handle.release_signer(release_for(&info.fingerprint));
    let envelope = sign_envelope(CONFORMANCE_PAYLOAD, IN_TOTO_PAYLOAD_TYPE, &[&released])
        .map_err(|e| fail("sign_envelope", e.to_string()))?;

    // Single-use UV release must refuse a second signing call.
    if released.sign_pae(b"second-use-must-fail").is_ok() {
        return Err(fail("single_use_uv", "ReleasedSigner permitted a second signing operation"));
    }

    // Direct PAE signing via handle.sign_with_release must also verify.
    let pae_bytes = pae(IN_TOTO_PAYLOAD_TYPE.as_bytes(), CONFORMANCE_PAYLOAD);
    let raw_sig = handle
        .sign_with_release(release_for(&info.fingerprint), &pae_bytes)
        .map_err(|e| fail("sign_with_release", e.to_string()))?;
    pubkey.verify_raw(&pae_bytes, &raw_sig).map_err(|e| fail("verify_raw", e.to_string()))?;

    // A UV release bound to a different (real) key must be refused by both signing paths.
    let other_key = store
        .create(Alg::EcdsaP256Sha256, CONFORMANCE_OTHER_LABEL, true)
        .map_err(|e| fail("create_other", e.to_string()))?;
    if other_key.fingerprint == info.fingerprint {
        return Err(fail("create_other", "second key has the same fingerprint as the first"));
    }
    if handle.sign_with_release(release_for(&other_key.fingerprint), &pae_bytes).is_ok() {
        return Err(fail("mismatched_uv_release", "sign_with_release accepted a release bound to another key"));
    }
    let mismatched = handle.release_signer(release_for(&other_key.fingerprint));
    if mismatched.sign_pae(&pae_bytes).is_ok() {
        return Err(fail("mismatched_uv_release", "ReleasedSigner accepted a release bound to another key"));
    }
    store.delete(&other_key.fingerprint).map_err(|e| fail("delete_other", e.to_string()))?;

    // 4. Verify the DSSE envelope with fid-sign-core::verify.
    let verified_fp = verify(&envelope, &pubkey).map_err(|e| fail("verify", e.to_string()))?;
    if verified_fp != info.fingerprint {
        return Err(fail("verify", format!("verified fingerprint {verified_fp} != {}", info.fingerprint)));
    }
    let mut tampered = envelope.clone();
    tampered.payload.push(b' ');
    if verify(&tampered, &pubkey).is_ok() {
        return Err(fail("verify_tampered", "tampered envelope unexpectedly verified"));
    }

    // 5. Delete the key and assert it is gone.
    store.delete(&info.fingerprint).map_err(|e| fail("delete", e.to_string()))?;
    let after = store.list().map_err(|e| fail("list_after_delete", e.to_string()))?;
    if after.iter().any(|k| k.fingerprint == info.fingerprint || k.fingerprint == other_key.fingerprint) {
        return Err(fail("list_after_delete", "deleted key still appears in list()"));
    }
    match store.open(&info.fingerprint) {
        Err(KeyStoreError::NotFound) => {}
        other => return Err(fail("open_after_delete", format!("expected NotFound, got {other:?}"))),
    }
    match store.public_key(&info.fingerprint) {
        Err(KeyStoreError::NotFound) => {}
        other => return Err(fail("public_key_after_delete", format!("expected NotFound, got {other:?}"))),
    }
    match store.delete(&info.fingerprint) {
        Err(KeyStoreError::NotFound) => {}
        other => return Err(fail("delete_twice", format!("expected NotFound, got {other:?}"))),
    }

    // A handle opened before delete must not sign afterwards, by either signing path.
    if handle.sign_with_release(release_for(&info.fingerprint), &pae_bytes).is_ok() {
        return Err(fail("sign_after_delete", "handle opened before delete still signed via sign_with_release"));
    }
    let stale = handle.release_signer(release_for(&info.fingerprint));
    if stale.sign_pae(&pae_bytes).is_ok() {
        return Err(fail("sign_after_delete", "handle opened before delete still signed via ReleasedSigner"));
    }

    Ok(ConformanceReport { created: info, envelope })
}
