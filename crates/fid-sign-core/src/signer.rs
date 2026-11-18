//! Signing through opaque key handles.

use std::fmt;

use crate::{pae, Alg, Envelope, PublicKey, Sig, SigError, MAX_SIGNATURES};

/// Why an envelope could not be signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// `sig.sign_failed`: the key handle refused or failed (detail from the backend).
    Backend(String),
    /// The request or the handle's output is not acceptable: the signer count is outside
    /// 1..=64, the handle's algorithm does not match its public key, or the produced
    /// signature does not verify (for P-256: high S or non-canonical DER). Carries the
    /// verification error and its code.
    Rejected(SigError),
}

impl SignError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            SignError::Backend(_) => "sig.sign_failed",
            SignError::Rejected(e) => e.code(),
        }
    }
}

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignError::Backend(detail) => write!(f, "sig.sign_failed: {detail}"),
            SignError::Rejected(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SignError {}

/// An opaque key handle that signs PAE bytes. Keystore backends implement it
/// (`TRUST.SIGN.KEYSTORE`); this crate never sees private key material outside the
/// `test-signer` feature.
///
/// P-256 handles return a DER `Ecdsa-Sig-Value` with low S; Ed25519 handles return the
/// 64-byte signature.
pub trait Signer {
    fn algorithm(&self) -> Alg;
    fn public_key(&self) -> PublicKey;
    fn sign_pae(&self, pae: &[u8]) -> Result<Vec<u8>, SignError>;
}

/// Signs `payload` with each signer and returns the envelope. Each `keyid` is the
/// signer's fingerprint, and each signature is verified before it is accepted.
pub fn sign_envelope(payload: &[u8], payload_type: &str, signers: &[&dyn Signer]) -> Result<Envelope, SignError> {
    if signers.is_empty() || signers.len() > MAX_SIGNATURES {
        return Err(SignError::Rejected(SigError::Malformed("between 1 and 64 signers are required")));
    }
    let message = pae(payload_type.as_bytes(), payload);
    let mut signatures = Vec::with_capacity(signers.len());
    for signer in signers {
        let key = signer.public_key();
        if key.algorithm() != signer.algorithm() {
            return Err(SignError::Rejected(SigError::Malformed("signer algorithm does not match its public key")));
        }
        let sig = signer.sign_pae(&message)?;
        key.verify_raw(&message, &sig).map_err(SignError::Rejected)?;
        signatures.push(Sig { keyid: key.fingerprint().as_str().to_owned(), sig });
    }
    Ok(Envelope { payload: payload.to_vec(), payload_type: payload_type.to_owned(), signatures })
}

#[cfg(feature = "test-signer")]
pub use software::SoftwareSigner;

#[cfg(feature = "test-signer")]
mod software {
    // ed25519-dalek 2 and p256 0.14 use different `signature` major versions.
    use ed25519_dalek::Signer as _;
    use p256::ecdsa::signature::Signer as _;

    use super::{SignError, Signer};
    use crate::{key::pem_decode, p256_signature_to_der, Alg, PublicKey, SigError};

    /// PKCS#8 (RFC 8410) prefix of an Ed25519 private key; the 32-byte seed follows.
    const PKCS8_ED25519: &[u8] = b"\x30\x2e\x02\x01\x00\x30\x05\x06\x03\x2b\x65\x70\x04\x22\x04\x20";
    /// PKCS#8 (RFC 5208, RFC 5915) prefix of a P-256 private key with its public key;
    /// the 32-byte scalar, `a1 44 03 42 00` and the 65-byte point follow.
    const PKCS8_P256: &[u8] = b"\x30\x81\x87\x02\x01\x00\x30\x13\x06\x07\x2a\x86\x48\xce\x3d\x02\x01\x06\x08\x2a\x86\x48\xce\x3d\x03\x01\x07\x04\x6d\x30\x6b\x02\x01\x01\x04\x20";
    const PKCS8_P256_PUB: &[u8] = b"\xa1\x44\x03\x42\x00";

    enum Key {
        Ed25519(ed25519_dalek::SigningKey),
        P256(p256::ecdsa::SigningKey),
    }

    /// TEST ONLY: a signer holding a private key in memory. Compiled only with the
    /// `test-signer` feature; production signing goes through keystore handles.
    pub struct SoftwareSigner {
        key: Key,
        public: PublicKey,
    }

    impl SoftwareSigner {
        /// Ed25519 from a 32-byte RFC 8032 seed.
        pub fn ed25519_from_seed(seed: &[u8; 32]) -> SoftwareSigner {
            let key = ed25519_dalek::SigningKey::from_bytes(seed);
            let public = PublicKey::Ed25519(key.verifying_key().to_bytes());
            SoftwareSigner { key: Key::Ed25519(key), public }
        }

        /// P-256 from a 32-byte big-endian private scalar in [1, n-1].
        pub fn p256_from_scalar(scalar: &[u8]) -> Result<SoftwareSigner, SigError> {
            let key = p256::ecdsa::SigningKey::from_slice(scalar)
                .map_err(|_| SigError::Malformed("P-256 private scalar out of range"))?;
            let point = key.verifying_key().to_sec1_point(false);
            let public = PublicKey::p256_from_sec1(point.as_bytes())?;
            Ok(SoftwareSigner { key: Key::P256(key), public })
        }

        /// A `-----BEGIN PRIVATE KEY-----` PKCS#8 PEM in the two fixed layouts written by
        /// `fixtures/dsse/make_fixtures.py`: Ed25519 (RFC 8410) and P-256 with the public
        /// key present, which must match the scalar.
        pub fn from_pkcs8_pem(pem: &str) -> Result<SoftwareSigner, SigError> {
            let der = pem_decode(pem, "PRIVATE KEY")?;
            if let Some(seed) = der.strip_prefix(PKCS8_ED25519) {
                let seed: [u8; 32] =
                    seed.try_into().map_err(|_| SigError::Malformed("Ed25519 PKCS#8 seed is not 32 bytes"))?;
                return Ok(SoftwareSigner::ed25519_from_seed(&seed));
            }
            if let Some(rest) = der.strip_prefix(PKCS8_P256) {
                if rest.len() == 32 + PKCS8_P256_PUB.len() + 65 {
                    let (scalar, tail) = rest.split_at(32);
                    if let Some(point) = tail.strip_prefix(PKCS8_P256_PUB) {
                        let signer = SoftwareSigner::p256_from_scalar(scalar)?;
                        if signer.public != PublicKey::p256_from_sec1(point)? {
                            return Err(SigError::Malformed("PKCS#8 public key does not match the scalar"));
                        }
                        return Ok(signer);
                    }
                }
            }
            Err(SigError::UnsupportedAlgorithm)
        }
    }

    impl Signer for SoftwareSigner {
        fn algorithm(&self) -> Alg {
            self.public.algorithm()
        }

        fn public_key(&self) -> PublicKey {
            self.public.clone()
        }

        fn sign_pae(&self, pae: &[u8]) -> Result<Vec<u8>, SignError> {
            match &self.key {
                Key::Ed25519(key) => Ok(key.sign(pae).to_bytes().to_vec()),
                Key::P256(key) => {
                    let sig: p256::ecdsa::Signature =
                        key.try_sign(pae).map_err(|_| SignError::Backend("P-256 signing failed".to_owned()))?;
                    let mut rs = [0u8; 64];
                    let bytes = sig.to_bytes();
                    if bytes.len() != rs.len() {
                        return Err(SignError::Backend("P-256 signature is not 64 bytes".to_owned()));
                    }
                    rs.copy_from_slice(&bytes);
                    Ok(p256_signature_to_der(&rs))
                }
            }
        }
    }
}
