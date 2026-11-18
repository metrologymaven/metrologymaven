//! Public keys, algorithms and fingerprints.

use std::fmt;

use p256::ecdsa::signature::Verifier as _;
use sha2::{Digest as _, Sha256};

use crate::{b64, der, SigError};

/// Largest accepted PEM text, in bytes.
pub(crate) const MAX_PEM_BYTES: usize = 64 * 1024;

/// The supported signature algorithms (REQ-TRUST.SIGN.DSSE-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Alg {
    /// ECDSA over NIST P-256 with SHA-256; signatures are DER `Ecdsa-Sig-Value` with low S.
    EcdsaP256Sha256,
    /// Ed25519 (RFC 8032, pure); signatures are 64 raw bytes.
    Ed25519,
}

impl Alg {
    /// Stable name.
    pub fn name(self) -> &'static str {
        match self {
            Alg::EcdsaP256Sha256 => "ecdsa-p256-sha256",
            Alg::Ed25519 => "ed25519",
        }
    }
}

/// A verification key. Construct it with [`PublicKey::from_spki_der`],
/// [`PublicKey::from_pem`], [`PublicKey::p256_from_sec1`] or
/// [`PublicKey::ed25519_from_bytes`], which validate the point; [`verify`](crate::verify)
/// re-validates keys built from the variants directly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PublicKey {
    /// Uncompressed SEC1 point `04 || X || Y`.
    P256([u8; 65]),
    /// RFC 8032 encoded point.
    Ed25519([u8; 32]),
}

impl PublicKey {
    /// Parses SubjectPublicKeyInfo DER. RSA and any algorithm or curve other than
    /// P-256 and Ed25519 give `sig.unsupported_algorithm`. P-256 keys must use the
    /// uncompressed point form, so the fingerprint of a key is unique.
    pub fn from_spki_der(der: &[u8]) -> Result<PublicKey, SigError> {
        match der::parse_spki(der)? {
            der::SpkiKey::Ed25519(key) => PublicKey::ed25519_from_bytes(key),
            der::SpkiKey::P256(point) => PublicKey::p256_from_sec1(point),
        }
    }

    /// Parses a `-----BEGIN PUBLIC KEY-----` PEM block (SubjectPublicKeyInfo).
    pub fn from_pem(pem: &str) -> Result<PublicKey, SigError> {
        PublicKey::from_spki_der(&pem_decode(pem, "PUBLIC KEY")?)
    }

    /// An uncompressed SEC1 P-256 point; it must lie on the curve.
    pub fn p256_from_sec1(point: &[u8]) -> Result<PublicKey, SigError> {
        let bytes: [u8; 65] =
            point.try_into().map_err(|_| SigError::Malformed("P-256 key is not a 65-byte uncompressed point"))?;
        if bytes[0] != 0x04 {
            return Err(SigError::Malformed("P-256 key is not a 65-byte uncompressed point"));
        }
        p256::ecdsa::VerifyingKey::from_sec1_bytes(&bytes)
            .map_err(|_| SigError::Malformed("P-256 key is not a point on the curve"))?;
        Ok(PublicKey::P256(bytes))
    }

    /// A 32-byte Ed25519 key; it must decode to a curve point. Small-order keys are
    /// accepted here and rejected by every verification.
    pub fn ed25519_from_bytes(key: &[u8]) -> Result<PublicKey, SigError> {
        let bytes: [u8; 32] = key.try_into().map_err(|_| SigError::Malformed("Ed25519 key is not 32 bytes"))?;
        ed25519_dalek::VerifyingKey::from_bytes(&bytes)
            .map_err(|_| SigError::Malformed("Ed25519 key is not a valid point encoding"))?;
        Ok(PublicKey::Ed25519(bytes))
    }

    /// The key's algorithm.
    pub fn algorithm(&self) -> Alg {
        match self {
            PublicKey::P256(_) => Alg::EcdsaP256Sha256,
            PublicKey::Ed25519(_) => Alg::Ed25519,
        }
    }

    /// SubjectPublicKeyInfo DER (RFC 5480 for P-256, RFC 8410 for Ed25519).
    pub fn spki_der(&self) -> Vec<u8> {
        match self {
            PublicKey::P256(point) => der::spki_p256(point),
            PublicKey::Ed25519(key) => der::spki_ed25519(key),
        }
    }

    /// `-----BEGIN PUBLIC KEY-----` PEM with 64-character lines and LF endings.
    pub fn to_pem(&self) -> String {
        let text = b64::encode(&self.spki_der());
        let mut out = String::from("-----BEGIN PUBLIC KEY-----\n");
        for line in text.as_bytes().chunks(64) {
            out.push_str(&String::from_utf8_lossy(line));
            out.push('\n');
        }
        out.push_str("-----END PUBLIC KEY-----\n");
        out
    }

    /// The key fingerprint (REQ-TRUST.SIGN.DSSE-005).
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of_spki_der(&self.spki_der())
    }

    /// Verifies one signature over `message` (for DSSE, the PAE bytes).
    ///
    /// P-256: `signature` must be strict DER with r, s in [1, n-1] and s <= n/2.
    /// Ed25519: 64 bytes, strict verification (canonical S and R, no small-order key or R).
    /// Every failure is `sig.invalid`.
    pub fn verify_raw(&self, message: &[u8], signature: &[u8]) -> Result<(), SigError> {
        match self {
            PublicKey::P256(point) => {
                let rs = der::ecdsa_from_der(signature).ok_or(SigError::Invalid)?;
                if der::is_high_s(&rs[32..]) {
                    return Err(SigError::Invalid);
                }
                let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(point).map_err(|_| SigError::Invalid)?;
                let sig = p256::ecdsa::Signature::from_slice(&rs).map_err(|_| SigError::Invalid)?;
                key.verify(message, &sig).map_err(|_| SigError::Invalid)
            }
            PublicKey::Ed25519(bytes) => {
                let key = ed25519_dalek::VerifyingKey::from_bytes(bytes).map_err(|_| SigError::Invalid)?;
                let raw: [u8; 64] = signature.try_into().map_err(|_| SigError::Invalid)?;
                let sig = ed25519_dalek::Signature::from_bytes(&raw);
                key.verify_strict(message, &sig).map_err(|_| SigError::Invalid)
            }
        }
    }
}

/// Converts a fixed-width P-256 signature `r || s` (32 + 32 bytes, as many keystores
/// return it) into the form DSSE envelopes carry: S replaced by n - S when high, then
/// DER `Ecdsa-Sig-Value`. Keystore [`Signer`](crate::Signer) implementations use it.
pub fn p256_signature_to_der(rs: &[u8; 64]) -> Vec<u8> {
    let mut rs = *rs;
    der::normalize_low_s(&mut rs);
    der::ecdsa_to_der(&rs)
}

/// Lowercase hex SHA-256 of a key's SubjectPublicKeyInfo DER (64 characters).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint(String);

impl Fingerprint {
    /// Fingerprint of the given SubjectPublicKeyInfo DER.
    pub fn of_spki_der(spki: &[u8]) -> Fingerprint {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let digest = Sha256::digest(spki);
        let mut out = String::with_capacity(64);
        for b in digest.iter() {
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 15)]));
        }
        Fingerprint(out)
    }

    /// Parses 64 lowercase hex digits.
    pub fn parse(text: &str) -> Option<Fingerprint> {
        let ok = text.len() == 64 && text.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
        ok.then(|| Fingerprint(text.to_owned()))
    }

    /// The 64 hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Decodes one PEM block with the given label (RFC 7468, strict base64 body).
pub(crate) fn pem_decode(pem: &str, label: &str) -> Result<Vec<u8>, SigError> {
    let bad = || SigError::Malformed("not a single PEM block with the expected label");
    if pem.len() > MAX_PEM_BYTES {
        return Err(SigError::Malformed("PEM text larger than 64 KiB"));
    }
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut lines = pem.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some(begin.as_str()) {
        return Err(bad());
    }
    let mut body = String::new();
    let mut ended = false;
    for line in lines.by_ref() {
        if line == end {
            ended = true;
            break;
        }
        body.push_str(line);
    }
    if !ended || lines.next().is_some() {
        return Err(bad());
    }
    b64::decode(body.as_bytes()).ok_or(SigError::Malformed("PEM body is not canonical base64"))
}
