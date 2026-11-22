//! In-memory test keystore backend (`MemKeyStore`), compiled only under feature `test-backend`
//! (REQ-TRUST.SIGN.KEYSTORE.TRAIT-005).
//!
//! Uses `fid-sign-core`'s `SoftwareSigner` (behind `test-signer`) and stores sidecar metadata
//! under `<config_dir>/fid/keys/<fingerprint>.json`. Private key material is zeroized on drop,
//! never returned through any API, and implements neither `Serialize`, nor `Display`, nor
//! content-bearing `Debug`.

use crate::api::{
    delete_sidecar, list_sidecars, validate_label, validate_rfc3339_utc, write_sidecar, KeyHandle, KeyHandleOps,
    KeyInfo, KeyStore, KeyStoreError,
};
use fid_sign_core::{base64_encode, Alg, Fingerprint, PublicKey, SignError, Signer, SoftwareSigner};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use zeroize::Zeroize;

/// Maximum attempts to draw a valid P-256 scalar in `[1, n-1]`.
const MAX_KEYGEN_ATTEMPTS: usize = 64;
/// Default deterministic RFC 3339 UTC timestamp used by [`MemKeyStore::new`].
const DEFAULT_CREATED_UTC: &str = "2027-01-01T00:00:00Z";

/// Private key material held strictly in memory for the test backend.
///
/// Implements neither `Serialize`, nor `Display`, nor content-bearing `Debug`. Zeroized on drop.
struct PrivateKeyMaterial {
    alg: Alg,
    bytes: [u8; 32],
}

impl Drop for PrivateKeyMaterial {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl fmt::Debug for PrivateKeyMaterial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateKeyMaterial").field("alg", &self.alg.name()).field("bytes", &"<redacted>").finish()
    }
}

struct MemKeySlot {
    material: PrivateKeyMaterial,
    signer: SoftwareSigner,
    public: PublicKey,
    deleted: AtomicBool,
}

impl fmt::Debug for MemKeySlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemKeySlot")
            .field("alg", &self.public.algorithm().name())
            .field("fingerprint", &self.public.fingerprint().as_str())
            .field("material", &self.material)
            .finish()
    }
}

struct MemHandle {
    slot: Arc<MemKeySlot>,
}

impl KeyHandleOps for MemHandle {
    fn algorithm(&self) -> Alg {
        self.slot.public.algorithm()
    }

    fn public_key(&self) -> PublicKey {
        self.slot.public.clone()
    }

    fn sign_pae_raw(&self, pae: &[u8]) -> Result<Vec<u8>, SignError> {
        if self.slot.deleted.load(Ordering::Acquire) {
            return Err(SignError::Backend("key_deleted".to_string()));
        }
        self.slot.signer.sign_pae(pae)
    }
}

/// In-memory test backend implementing [`KeyStore`].
///
/// Writes canonical JSON sidecars into `<config_dir>/fid/keys/<fingerprint>.json` and keeps
/// non-exportable test key material strictly in memory.
pub struct MemKeyStore {
    config_dir: PathBuf,
    created_utc: String,
    keys: Mutex<BTreeMap<Fingerprint, Arc<MemKeySlot>>>,
}

impl fmt::Debug for MemKeyStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemKeyStore")
            .field("config_dir", &self.config_dir)
            .field("created_utc", &self.created_utc)
            .finish()
    }
}

impl MemKeyStore {
    /// Creates an empty in-memory test keystore writing sidecar metadata under `config_dir`.
    pub fn new(config_dir: impl Into<PathBuf>) -> MemKeyStore {
        MemKeyStore {
            config_dir: config_dir.into(),
            created_utc: DEFAULT_CREATED_UTC.to_string(),
            keys: Mutex::new(BTreeMap::new()),
        }
    }

    /// Creates an empty in-memory test keystore stamping created keys with `created_utc`
    /// (must be valid RFC 3339 UTC `YYYY-MM-DDTHH:MM:SSZ`).
    pub fn with_created_utc(
        config_dir: impl Into<PathBuf>,
        created_utc: impl Into<String>,
    ) -> Result<MemKeyStore, KeyStoreError> {
        let ts = created_utc.into();
        if !validate_rfc3339_utc(&ts) {
            return Err(KeyStoreError::Backend("invalid_created_timestamp".to_string()));
        }
        Ok(MemKeyStore { config_dir: config_dir.into(), created_utc: ts, keys: Mutex::new(BTreeMap::new()) })
    }

    /// Root configuration directory where `<config_dir>/fid/keys/<fingerprint>.json` sidecars live.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Debug- and test-only verification helper for AC-4: returns `Ok(true)` when `haystack`
    /// (such as a sidecar file's bytes) contains none of the key's private bytes in raw,
    /// lowercase hex, uppercase hex or base64 form (including 16-byte halves), and `Ok(false)`
    /// if any match is found. Never returns private key bytes to the caller.
    #[cfg(all(feature = "test-backend", debug_assertions))]
    pub fn debug_assert_absent_from_bytes(
        &self,
        fingerprint: &Fingerprint,
        haystack: &[u8],
    ) -> Result<bool, KeyStoreError> {
        let guard = self.keys.lock().map_err(|_| KeyStoreError::Backend("mutex_poisoned".to_string()))?;
        let slot = guard.get(fingerprint).ok_or(KeyStoreError::NotFound)?;
        Ok(!bytes_contain_secret(haystack, &slot.material.bytes))
    }
}

#[cfg(all(feature = "test-backend", debug_assertions))]
fn bytes_contain_secret(haystack: &[u8], secret: &[u8; 32]) -> bool {
    let hex_lower = encode_hex(secret, false);
    let hex_upper = encode_hex(secret, true);
    let b64 = base64_encode(secret);
    let half_lo = &secret[..16];
    let half_hi = &secret[16..];
    let half_lo_hex = encode_hex(half_lo, false);
    let half_hi_hex = encode_hex(half_hi, false);
    let needles: [&[u8]; 7] = [
        secret.as_slice(),
        hex_lower.as_bytes(),
        hex_upper.as_bytes(),
        b64.as_bytes(),
        half_lo,
        half_hi,
        half_lo_hex.as_bytes(),
    ];
    needles.iter().any(|n| contains_subslice(haystack, n)) || contains_subslice(haystack, half_hi_hex.as_bytes())
}

#[cfg(all(feature = "test-backend", debug_assertions))]
fn encode_hex(bytes: &[u8], upper: bool) -> String {
    const LOWER: &[u8; 16] = b"0123456789abcdef";
    const UPPER: &[u8; 16] = b"0123456789ABCDEF";
    let table = if upper { UPPER } else { LOWER };
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(table[usize::from(b >> 4)]));
        out.push(char::from(table[usize::from(b & 0x0f)]));
    }
    out
}

#[cfg(all(feature = "test-backend", debug_assertions))]
fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn generate_slot(alg: Alg) -> Result<MemKeySlot, KeyStoreError> {
    match alg {
        Alg::Ed25519 => {
            let mut seed = [0u8; 32];
            getrandom::getrandom(&mut seed).map_err(|e| KeyStoreError::Backend(format!("getrandom:{e}")))?;
            let signer = SoftwareSigner::ed25519_from_seed(&seed);
            let public = signer.public_key();
            let material = PrivateKeyMaterial { alg, bytes: seed };
            seed.zeroize();
            Ok(MemKeySlot { material, signer, public, deleted: AtomicBool::new(false) })
        }
        Alg::EcdsaP256Sha256 => {
            for _ in 0..MAX_KEYGEN_ATTEMPTS {
                let mut scalar = [0u8; 32];
                getrandom::getrandom(&mut scalar).map_err(|e| KeyStoreError::Backend(format!("getrandom:{e}")))?;
                if let Ok(signer) = SoftwareSigner::p256_from_scalar(&scalar) {
                    let public = signer.public_key();
                    let material = PrivateKeyMaterial { alg, bytes: scalar };
                    scalar.zeroize();
                    return Ok(MemKeySlot { material, signer, public, deleted: AtomicBool::new(false) });
                }
                scalar.zeroize();
            }
            Err(KeyStoreError::Backend("p256_keygen_exhausted".to_string()))
        }
    }
}

impl KeyStore for MemKeyStore {
    fn create(&self, alg: Alg, label: &str, _prefer_hardware: bool) -> Result<KeyInfo, KeyStoreError> {
        validate_label(label)?;
        let slot = Arc::new(generate_slot(alg)?);
        let fingerprint = slot.public.fingerprint();
        let info = KeyInfo {
            fingerprint: fingerprint.clone(),
            alg,
            label: label.to_string(),
            created: self.created_utc.clone(),
            // In-memory test backend is never hardware-backed.
            hardware: false,
        };
        let mut guard = self.keys.lock().map_err(|_| KeyStoreError::Backend("mutex_poisoned".to_string()))?;
        write_sidecar(&self.config_dir, &info)?;
        guard.insert(fingerprint, slot);
        Ok(info)
    }

    fn list(&self) -> Result<Vec<KeyInfo>, KeyStoreError> {
        let guard = self.keys.lock().map_err(|_| KeyStoreError::Backend("mutex_poisoned".to_string()))?;
        let sidecars = list_sidecars(&self.config_dir)?;
        let mut out: Vec<KeyInfo> = sidecars.into_iter().filter(|k| guard.contains_key(&k.fingerprint)).collect();
        out.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        Ok(out)
    }

    fn open(&self, fingerprint: &Fingerprint) -> Result<KeyHandle, KeyStoreError> {
        let guard = self.keys.lock().map_err(|_| KeyStoreError::Backend("mutex_poisoned".to_string()))?;
        let slot = guard.get(fingerprint).ok_or(KeyStoreError::NotFound)?;
        Ok(KeyHandle::from_ops(Box::new(MemHandle { slot: Arc::clone(slot) })))
    }

    fn delete(&self, fingerprint: &Fingerprint) -> Result<(), KeyStoreError> {
        let mut guard = self.keys.lock().map_err(|_| KeyStoreError::Backend("mutex_poisoned".to_string()))?;
        let slot = guard.remove(fingerprint).ok_or(KeyStoreError::NotFound)?;
        slot.deleted.store(true, Ordering::Release);
        match delete_sidecar(&self.config_dir, fingerprint) {
            Ok(()) | Err(KeyStoreError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn public_key(&self, fingerprint: &Fingerprint) -> Result<PublicKey, KeyStoreError> {
        let guard = self.keys.lock().map_err(|_| KeyStoreError::Backend("mutex_poisoned".to_string()))?;
        let slot = guard.get(fingerprint).ok_or(KeyStoreError::NotFound)?;
        Ok(slot.public.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_format_is_redacted() {
        let secret = [0xab; 32];
        let mat = PrivateKeyMaterial { alg: Alg::Ed25519, bytes: secret };
        let rendered = format!("{mat:?}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
        assert!(!rendered.contains("171"), "must not leak decimal secret bytes: {rendered}");
        assert!(!rendered.contains("ab"), "must not leak hex secret bytes: {rendered}");
    }

    #[test]
    fn secret_absence_detector_is_non_vacuous() {
        let secret = [0x42u8; 32];
        assert!(!bytes_contain_secret(b"{\"alg\":\"ed25519\",\"label\":\"qa\"}", &secret));
        assert!(bytes_contain_secret(&secret, &secret));
        assert!(bytes_contain_secret(encode_hex(&secret, false).as_bytes(), &secret));
        assert!(bytes_contain_secret(encode_hex(&secret, true).as_bytes(), &secret));
        assert!(bytes_contain_secret(base64_encode(&secret).as_bytes(), &secret));
    }
}
