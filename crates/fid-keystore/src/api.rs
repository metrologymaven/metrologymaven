//! Keystore trait, opaque handle, UV release token, errors and sidecar metadata.

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_sign_core::{Alg, Fingerprint, PublicKey, SignError, Signer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Maximum UTF-8 byte length of a key label (REQ-TRUST.SIGN.KEYSTORE.TRAIT-002).
pub const MAX_LABEL_BYTES: usize = 128;
/// Maximum accepted sidecar metadata JSON size, in bytes.
pub const MAX_SIDECAR_BYTES: usize = 16 * 1024;
/// Maximum JSON nesting depth accepted in a sidecar file.
const MAX_SIDECAR_DEPTH: usize = 8;

const SIDECAR_KEYS: [&str; 5] = ["alg", "created", "fingerprint", "hardware", "label"];

// ------------------------------------------------------------------ errors

/// Typed errors returned by [`KeyStore`] operations and sidecar metadata helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStoreError {
    /// No key with the requested fingerprint exists (`keystore.not_found`).
    NotFound,
    /// Algorithm, label, metadata or operation is not supported (`keystore.unsupported`).
    Unsupported,
    /// Underlying platform or storage backend failure with a stable code (`keystore.backend`).
    Backend(String),
    /// Keystore or hardware token is locked and requires unlock / UV (`keystore.locked`).
    Locked,
}

impl KeyStoreError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            KeyStoreError::NotFound => "keystore.not_found",
            KeyStoreError::Unsupported => "keystore.unsupported",
            KeyStoreError::Backend(_) => "keystore.backend",
            KeyStoreError::Locked => "keystore.locked",
        }
    }
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyStoreError::NotFound => f.write_str("keystore.not_found"),
            KeyStoreError::Unsupported => f.write_str("keystore.unsupported"),
            KeyStoreError::Backend(code) => write!(f, "keystore.backend: {code}"),
            KeyStoreError::Locked => f.write_str("keystore.locked"),
        }
    }
}

impl std::error::Error for KeyStoreError {}

// ------------------------------------------------------------------ key metadata

/// Public metadata describing a key held in a [`KeyStore`] (REQ-TRUST.SIGN.KEYSTORE.TRAIT-002).
///
/// Stored as RFC 8785 canonical JSON at `<config>/fid/keys/<fingerprint>.json`. Never holds
/// private key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// Lowercase 64-hex SHA-256 of the SubjectPublicKeyInfo DER (per `fid-sign-core`).
    pub fingerprint: Fingerprint,
    /// Signature algorithm (`Alg::EcdsaP256Sha256` or `Alg::Ed25519`).
    pub alg: Alg,
    /// Human-readable label, non-empty and at most [`MAX_LABEL_BYTES`] (128) UTF-8 bytes.
    pub label: String,
    /// Creation timestamp in RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
    pub created: String,
    /// True when the private key resides in hardware (TPM, Secure Enclave, PKCS#11 token).
    pub hardware: bool,
}

impl KeyInfo {
    /// Deterministic sort key matching `TRUST.SIGN.KEYSTORE.CMDS` (created date, then fingerprint).
    pub fn sort_key(&self) -> (&str, &str) {
        (self.created.as_str(), self.fingerprint.as_str())
    }
}

/// Validates a key label: non-empty, at most [`MAX_LABEL_BYTES`] UTF-8 bytes, no control chars.
pub fn validate_label(label: &str) -> Result<(), KeyStoreError> {
    if label.trim().is_empty() || label.len() > MAX_LABEL_BYTES || label.chars().any(char::is_control) {
        return Err(KeyStoreError::Unsupported);
    }
    Ok(())
}

/// Validates an RFC 3339 UTC timestamp `YYYY-MM-DDTHH:MM:SSZ` strictly.
pub fn validate_rfc3339_utc(ts: &str) -> bool {
    let b = ts.as_bytes();
    if b.len() != 20
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
        || b.get(19) != Some(&b'Z')
    {
        return false;
    }
    let Some(year) = parse_digits(b, 0, 4) else { return false };
    let Some(month) = parse_digits(b, 5, 2) else { return false };
    let Some(day) = parse_digits(b, 8, 2) else { return false };
    let Some(hour) = parse_digits(b, 11, 2) else { return false };
    let Some(minute) = parse_digits(b, 14, 2) else { return false };
    let Some(second) = parse_digits(b, 17, 2) else { return false };
    year > 0 && day > 0 && day <= days_in_month(year, month) && hour <= 23 && minute <= 59 && second <= 59
}

fn days_in_month(year: u32, month: u32) -> u32 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

fn parse_digits(b: &[u8], from: usize, len: usize) -> Option<u32> {
    let part = b.get(from..from + len)?;
    if !part.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(part.iter().fold(0u32, |acc, &c| acc * 10 + u32::from(c - b'0')))
}

fn alg_from_name(name: &str) -> Option<Alg> {
    match name {
        "ecdsa-p256-sha256" => Some(Alg::EcdsaP256Sha256),
        "ed25519" => Some(Alg::Ed25519),
        _ => None,
    }
}

// ------------------------------------------------------------------ sidecar metadata files

/// `<config>/fid/keys` directory storing per-key sidecar JSON metadata.
pub fn sidecar_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("fid").join("keys")
}

/// `<config>/fid/keys/<fingerprint>.json` path for a single key's sidecar metadata.
pub fn sidecar_path(config_dir: &Path, fingerprint: &Fingerprint) -> PathBuf {
    sidecar_dir(config_dir).join(format!("{}.json", fingerprint.as_str()))
}

/// Encodes [`KeyInfo`] as RFC 8785 canonical JSON holding only the five metadata fields and
/// zero private key material.
pub fn encode_sidecar(info: &KeyInfo) -> Result<Vec<u8>, KeyStoreError> {
    validate_label(&info.label)?;
    if !validate_rfc3339_utc(&info.created) {
        return Err(KeyStoreError::Backend("invalid_created_timestamp".to_string()));
    }
    let mut map = BTreeMap::new();
    map.insert("alg".to_string(), CValue::Str(info.alg.name().to_string()));
    map.insert("created".to_string(), CValue::Str(info.created.clone()));
    map.insert("fingerprint".to_string(), CValue::Str(info.fingerprint.as_str().to_string()));
    map.insert("hardware".to_string(), CValue::Bool(info.hardware));
    map.insert("label".to_string(), CValue::Str(info.label.clone()));
    to_canonical_bytes(&CValue::Obj(map)).map_err(|e| KeyStoreError::Backend(format!("sidecar_encode:{e}")))
}

/// Decodes and strictly validates a sidecar metadata JSON document (exact 5 keys, no extras).
pub fn decode_sidecar(bytes: &[u8]) -> Result<KeyInfo, KeyStoreError> {
    let parsed = parse_with_limits(bytes, MAX_SIDECAR_BYTES, MAX_SIDECAR_DEPTH)
        .map_err(|e| KeyStoreError::Backend(format!("sidecar_parse:{e}")))?;
    let CValue::Obj(map) = parsed else {
        return Err(KeyStoreError::Backend("sidecar_parse:expected_object".to_string()));
    };
    let allowed: BTreeSet<&str> = SIDECAR_KEYS.into_iter().collect();
    for key in map.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(KeyStoreError::Backend(format!("sidecar_parse:unexpected_field:{key}")));
        }
    }
    let get_str = |k: &str| -> Result<&str, KeyStoreError> {
        match map.get(k) {
            Some(CValue::Str(v)) => Ok(v.as_str()),
            _ => Err(KeyStoreError::Backend(format!("sidecar_parse:invalid_{k}"))),
        }
    };
    let fp_raw = get_str("fingerprint")?;
    let fingerprint = Fingerprint::parse(fp_raw)
        .ok_or_else(|| KeyStoreError::Backend("sidecar_parse:bad_fingerprint".to_string()))?;
    let alg_raw = get_str("alg")?;
    let alg = alg_from_name(alg_raw).ok_or(KeyStoreError::Unsupported)?;
    let label = get_str("label")?.to_string();
    validate_label(&label)?;
    let created = get_str("created")?.to_string();
    if !validate_rfc3339_utc(&created) {
        return Err(KeyStoreError::Backend("sidecar_parse:bad_created".to_string()));
    }
    let hardware = match map.get("hardware") {
        Some(CValue::Bool(b)) => *b,
        _ => return Err(KeyStoreError::Backend("sidecar_parse:invalid_hardware".to_string())),
    };
    Ok(KeyInfo { fingerprint, alg, label, created, hardware })
}

/// Writes `<config>/fid/keys/<fingerprint>.json` atomically via temp file + flush + rename.
pub fn write_sidecar(config_dir: &Path, info: &KeyInfo) -> Result<PathBuf, KeyStoreError> {
    let bytes = encode_sidecar(info)?;
    let dir = sidecar_dir(config_dir);
    fs::create_dir_all(&dir).map_err(|e| KeyStoreError::Backend(format!("sidecar_mkdir:{e}")))?;
    let final_path = sidecar_path(config_dir, &info.fingerprint);
    let tmp_path = dir.join(format!(".{}.json.tmp", info.fingerprint.as_str()));
    let mut f = fs::File::create(&tmp_path).map_err(|e| KeyStoreError::Backend(format!("sidecar_create:{e}")))?;
    if let Err(e) = f.write_all(&bytes).and_then(|()| f.flush()) {
        let _ = fs::remove_file(&tmp_path);
        return Err(KeyStoreError::Backend(format!("sidecar_write:{e}")));
    }
    drop(f);
    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(KeyStoreError::Backend(format!("sidecar_rename:{e}")));
    }
    Ok(final_path)
}

/// Reads and validates `<config>/fid/keys/<fingerprint>.json`.
pub fn read_sidecar(config_dir: &Path, fingerprint: &Fingerprint) -> Result<KeyInfo, KeyStoreError> {
    let path = sidecar_path(config_dir, fingerprint);
    let mut bytes = Vec::new();
    match fs::File::open(&path) {
        Ok(f) => {
            f.take(MAX_SIDECAR_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| KeyStoreError::Backend(format!("sidecar_read:{e}")))?;
        }
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(KeyStoreError::NotFound),
        Err(e) => return Err(KeyStoreError::Backend(format!("sidecar_open:{e}"))),
    }
    if bytes.len() > MAX_SIDECAR_BYTES {
        return Err(KeyStoreError::Backend("sidecar_too_large".to_string()));
    }
    let info = decode_sidecar(&bytes)?;
    if info.fingerprint != *fingerprint {
        return Err(KeyStoreError::Backend("sidecar_fingerprint_mismatch".to_string()));
    }
    Ok(info)
}

/// Lists all valid sidecar metadata files under `<config>/fid/keys/`, sorted by `(created, fingerprint)`.
pub fn list_sidecars(config_dir: &Path) -> Result<Vec<KeyInfo>, KeyStoreError> {
    let dir = sidecar_dir(config_dir);
    let entries = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(KeyStoreError::Backend(format!("sidecar_readdir:{e}"))),
    };
    let mut names = Vec::new();
    for item in entries {
        let entry = item.map_err(|e| KeyStoreError::Backend(format!("sidecar_dirent:{e}")))?;
        let name = entry.file_name();
        let Some(s) = name.to_str() else { continue };
        if let Some(stem) = s.strip_suffix(".json") {
            if let Some(fp) = Fingerprint::parse(stem) {
                names.push(fp);
            }
        }
    }
    names.sort();
    let mut infos = Vec::with_capacity(names.len());
    for fp in &names {
        infos.push(read_sidecar(config_dir, fp)?);
    }
    infos.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    Ok(infos)
}

/// Deletes `<config>/fid/keys/<fingerprint>.json`.
pub fn delete_sidecar(config_dir: &Path, fingerprint: &Fingerprint) -> Result<(), KeyStoreError> {
    let path = sidecar_path(config_dir, fingerprint);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(KeyStoreError::NotFound),
        Err(e) => Err(KeyStoreError::Backend(format!("sidecar_delete:{e}"))),
    }
}

// ------------------------------------------------------------------ opaque handle & UV release

/// User-verification methods a production [`UvRelease`] may record (closed set; maintainer
/// decision on PR #444, following REQ-TRUST.SIGN.UV-004 and D-011).
pub const UV_METHODS: [&str; 5] = ["windows_hello", "macos_local_auth", "token_pin", "token_pin_touch", "passphrase"];

/// UV method recorded by test releases; accepted only when feature `test-backend` is enabled.
const TEST_UV_METHOD: &str = "test_uv";

/// True when `uv_method` is in [`UV_METHODS`], or is `test_uv` and feature `test-backend` is on.
pub fn is_valid_uv_method(uv_method: &str) -> bool {
    UV_METHODS.contains(&uv_method) || (cfg!(feature = "test-backend") && uv_method == TEST_UV_METHOD)
}

/// Single-use authorization recording that OS user verification succeeded for one signing
/// operation on one key (`TRUST.SIGN.UV`). Intentionally neither `Clone` nor `Copy`.
///
/// **This is a flow marker, not an in-process security boundary.** Any code linked into the
/// process can call [`UvRelease::issue`], so holding a `UvRelease` proves nothing by itself.
/// The guarantees are: (1) [`UvRelease::issue`] accepts only the closed [`UV_METHODS`] set
/// (plus `test_uv` under feature `test-backend`), so a forged release still records a real
/// method name; (2) the CI rule `python ci/keystore/key-bytes-scan --uv-release-issue` fails
/// if `UvRelease::issue` is reached anywhere outside crate `fid-uv` and fid-keystore's own
/// tests; (3) the OS keystore backends enforce user verification themselves where the
/// platform supports it. A sealed capability owned by `TRUST.SIGN.UV` is an open question
/// recorded in `spec/trust/sign/keystore/trait/NODE.md`.
#[derive(Debug, PartialEq, Eq)]
pub struct UvRelease {
    fingerprint: Fingerprint,
    uv_method: String,
}

impl UvRelease {
    /// Issues a single-use release bound to `fingerprint` and `uv_method`.
    ///
    /// Only `TRUST.SIGN.UV` (crate `fid-uv`) may call this, after verifying principal, channel
    /// and per-operation OS user verification; CI enforces that with
    /// `ci/keystore/key-bytes-scan --uv-release-issue`. Returns
    /// [`KeyStoreError::Unsupported`] unless [`is_valid_uv_method`] accepts `uv_method`.
    pub fn issue(fingerprint: Fingerprint, uv_method: &str) -> Result<UvRelease, KeyStoreError> {
        if !is_valid_uv_method(uv_method) {
            return Err(KeyStoreError::Unsupported);
        }
        Ok(UvRelease { fingerprint, uv_method: uv_method.to_string() })
    }

    /// Test-only UV release minted under feature `test-backend` for conformance and unit tests.
    #[cfg(feature = "test-backend")]
    pub fn test_release(fingerprint: Fingerprint) -> UvRelease {
        UvRelease { fingerprint, uv_method: TEST_UV_METHOD.to_string() }
    }

    /// Fingerprint of the key this release authorizes.
    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    /// User-verification method recorded for the signature statement (e.g. `windows_hello`).
    pub fn uv_method(&self) -> &str {
        &self.uv_method
    }
}

/// Backend operations wrapped inside an opaque [`KeyHandle`].
///
/// Keystore backends implement this trait so signing is reachable only through [`KeyHandle`]
/// with a [`UvRelease`], never directly as an ungated `fid_sign_core::Signer`.
pub trait KeyHandleOps: Send + Sync {
    fn algorithm(&self) -> Alg;
    fn public_key(&self) -> PublicKey;
    fn sign_pae_raw(&self, pae: &[u8]) -> Result<Vec<u8>, SignError>;
}

/// Opaque handle to a signing key held inside a [`KeyStore`].
///
/// Exposes only the key's algorithm and public key directly; signing requires a single-use
/// [`UvRelease`] token. `Debug` is manually redacted and never prints key material.
pub struct KeyHandle {
    inner: Box<dyn KeyHandleOps>,
}

impl KeyHandle {
    /// Wraps a backend key handle implementation into an opaque [`KeyHandle`].
    pub fn from_ops(ops: Box<dyn KeyHandleOps>) -> KeyHandle {
        KeyHandle { inner: ops }
    }

    /// Signature algorithm of this key.
    pub fn algorithm(&self) -> Alg {
        self.inner.algorithm()
    }

    /// Public verification key corresponding to this handle.
    pub fn public_key(&self) -> PublicKey {
        self.inner.public_key()
    }

    /// Key fingerprint (`fid_sign_core::Fingerprint`).
    pub fn fingerprint(&self) -> Fingerprint {
        self.inner.public_key().fingerprint()
    }

    /// Signs DSSE PAE bytes once, consuming the [`UvRelease`] authorization token.
    pub fn sign_with_release(&self, release: UvRelease, pae: &[u8]) -> Result<Vec<u8>, SignError> {
        let expected = self.fingerprint();
        if release.fingerprint != expected {
            return Err(SignError::Backend("uv_release_fingerprint_mismatch".to_string()));
        }
        self.inner.sign_pae_raw(pae)
    }

    /// Produces a one-shot [`ReleasedSigner`] implementing [`fid_sign_core::Signer`] for a single
    /// `sign_envelope` call, consuming `release`. Any second sign attempt on the same
    /// [`ReleasedSigner`] is refused.
    pub fn release_signer(&self, release: UvRelease) -> ReleasedSigner<'_> {
        ReleasedSigner { handle: self, release, used: AtomicBool::new(false) }
    }
}

impl fmt::Debug for KeyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyHandle")
            .field("fingerprint", &self.fingerprint().as_str())
            .field("alg", &self.algorithm().name())
            .field("key_material", &"<redacted>")
            .finish()
    }
}

/// One-shot [`fid_sign_core::Signer`] adapter bound to a single [`UvRelease`].
pub struct ReleasedSigner<'a> {
    handle: &'a KeyHandle,
    release: UvRelease,
    used: AtomicBool,
}

impl ReleasedSigner<'_> {
    /// User-verification method bound to this signing release.
    pub fn uv_method(&self) -> &str {
        self.release.uv_method()
    }
}

impl fmt::Debug for ReleasedSigner<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReleasedSigner")
            .field("fingerprint", &self.handle.fingerprint().as_str())
            .field("uv_method", &self.release.uv_method())
            .field("used", &self.used.load(Ordering::Acquire))
            .field("key_material", &"<redacted>")
            .finish()
    }
}

impl Signer for ReleasedSigner<'_> {
    fn algorithm(&self) -> Alg {
        self.handle.algorithm()
    }

    fn public_key(&self) -> PublicKey {
        self.handle.public_key()
    }

    fn sign_pae(&self, pae: &[u8]) -> Result<Vec<u8>, SignError> {
        if self.release.fingerprint != self.handle.fingerprint() {
            return Err(SignError::Backend("uv_release_fingerprint_mismatch".to_string()));
        }
        if self.used.swap(true, Ordering::AcqRel) {
            return Err(SignError::Backend("uv_release_already_used".to_string()));
        }
        self.handle.inner.sign_pae_raw(pae)
    }
}

// ------------------------------------------------------------------ KeyStore trait

/// Contract implemented by every OS keystore backend and the in-memory test backend
/// (REQ-TRUST.SIGN.KEYSTORE.TRAIT-001).
///
/// No method returns or exposes private key bytes.
pub trait KeyStore {
    /// Creates a non-exportable signing key for `alg` with `label` (<= 128 UTF-8 bytes) and
    /// writes its sidecar metadata. Uses hardware backing when `prefer_hardware` is true and
    /// available for `alg`.
    fn create(&self, alg: Alg, label: &str, prefer_hardware: bool) -> Result<KeyInfo, KeyStoreError>;

    /// Lists all keys known to this store, sorted by creation timestamp then fingerprint.
    fn list(&self) -> Result<Vec<KeyInfo>, KeyStoreError>;

    /// Opens an opaque [`KeyHandle`] for the key with `fingerprint`.
    fn open(&self, fingerprint: &Fingerprint) -> Result<KeyHandle, KeyStoreError>;

    /// Deletes the key and its sidecar metadata from the store.
    fn delete(&self, fingerprint: &Fingerprint) -> Result<(), KeyStoreError>;

    /// Returns the public verification key for `fingerprint`.
    fn public_key(&self, fingerprint: &Fingerprint) -> Result<PublicKey, KeyStoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_and_timestamp_validation() {
        assert!(validate_label("qa-signing-key").is_ok());
        assert!(validate_label(&"a".repeat(MAX_LABEL_BYTES)).is_ok());
        assert!(validate_label(&"a".repeat(MAX_LABEL_BYTES + 1)).is_err());
        assert!(validate_label("").is_err());
        assert!(validate_label("   ").is_err());
        assert!(validate_label("bad\nlabel").is_err());

        assert!(validate_rfc3339_utc("2027-01-15T12:00:00Z"));
        assert!(validate_rfc3339_utc("2024-02-29T23:59:59Z"));
        assert!(!validate_rfc3339_utc("2027-02-29T00:00:00Z"));
        assert!(!validate_rfc3339_utc("2027-01-15T12:00:00+00:00"));
        assert!(!validate_rfc3339_utc("not-a-date"));
    }

    #[test]
    fn sidecar_canonical_round_trip_and_extra_field_rejected() {
        let fp = Fingerprint::parse(&"a".repeat(64)).unwrap_or_else(|| Fingerprint::of_spki_der(b"fallback"));
        let info = KeyInfo {
            fingerprint: fp,
            alg: Alg::EcdsaP256Sha256,
            label: "qa".to_string(),
            created: "2027-01-15T12:00:00Z".to_string(),
            hardware: false,
        };
        let bytes = encode_sidecar(&info);
        assert!(bytes.is_ok());
        if let Ok(b) = bytes {
            assert_eq!(decode_sidecar(&b), Ok(info));
        }
        // Extra field (e.g. smuggled private_key) must be rejected.
        let bad = br#"{"alg":"ecdsa-p256-sha256","created":"2027-01-15T12:00:00Z","fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","hardware":false,"label":"qa","private_key":"00"}"#;
        assert!(decode_sidecar(bad).is_err());
    }

    #[test]
    fn uv_release_issue_accepts_only_the_closed_method_set() {
        let fp = Fingerprint::of_spki_der(b"uv-method-unit-test");
        for method in UV_METHODS {
            let release = UvRelease::issue(fp.clone(), method);
            assert_eq!(release.as_ref().map(UvRelease::uv_method), Ok(method), "{method}");
        }
        for bad in ["", "none", "WINDOWS_HELLO", "windows_hello ", "touch_id", "pin", "test_uv\0"] {
            assert_eq!(UvRelease::issue(fp.clone(), bad), Err(KeyStoreError::Unsupported), "{bad:?}");
        }
        // `test_uv` is accepted exactly when feature `test-backend` is compiled in.
        assert_eq!(UvRelease::issue(fp, "test_uv").is_ok(), cfg!(feature = "test-backend"));
        assert_eq!(is_valid_uv_method("test_uv"), cfg!(feature = "test-backend"));
    }
}
