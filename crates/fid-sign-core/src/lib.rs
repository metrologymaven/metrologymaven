//! DSSE v1.0 envelope encoding, signature verification and key fingerprints. Spec node
//! `TRUST.SIGN.DSSE`.
//!
//! The pure cryptographic core shared by the signer and the verifier:
//!
//! - [`pae`]: DSSE v1.0 pre-authentication encoding (REQ-TRUST.SIGN.DSSE-001).
//! - [`Envelope`]: the JSON envelope (`payload` in base64, `payloadType`,
//!   `signatures[{keyid, sig}]`), parsed with bounds (64 MiB, 64 signatures, strict
//!   base64; REQ-TRUST.SIGN.DSSE-006) and written as RFC 8785 canonical JSON.
//! - [`PublicKey`]: ECDSA P-256 / SHA-256 and Ed25519 only; any other
//!   SubjectPublicKeyInfo algorithm is `sig.unsupported_algorithm`
//!   (REQ-TRUST.SIGN.DSSE-003). ECDSA signatures must be canonical DER with a low S
//!   (REQ-TRUST.SIGN.DSSE-004); Ed25519 uses strict verification, which rejects
//!   small-order keys and non-canonical encodings.
//! - [`Fingerprint`]: lowercase hex SHA-256 of the SubjectPublicKeyInfo DER; every
//!   emitted `keyid` equals it (REQ-TRUST.SIGN.DSSE-005).
//! - [`verify`]: checks signatures over PAE(payloadType, decoded payload)
//!   (REQ-TRUST.SIGN.DSSE-007).
//! - [`Signer`]: signing through an opaque key handle. This crate holds no private keys;
//!   a software signer exists only behind the `test-signer` feature.
//!
//! - [`troot`]: offline trust roots, revocation and the `verify.check_signature@1` command
//!   (spec node `TRUST.SIGN.TROOT`).
//!
//! Everything here is D0 (deterministic) and has no network dependencies; only
//! [`troot::command`] opens files, through the caller's opener.

mod b64;
mod der;
mod envelope;
mod key;
mod pae;
mod signer;
pub mod troot;
mod verify;

use std::fmt;

pub use b64::{decode as base64_decode, encode as base64_encode};
pub use envelope::{Envelope, Sig, MAX_ENVELOPE_BYTES, MAX_SIGNATURES};
pub use key::{p256_signature_to_der, Alg, Fingerprint, PublicKey};
pub use pae::pae;
#[cfg(feature = "test-signer")]
pub use signer::SoftwareSigner;
pub use signer::{sign_envelope, SignError, Signer};
pub use verify::{verify, verify_json};

/// `payloadType` for Result Package statements (REQ-TRUST.SIGN.DSSE-002).
pub const IN_TOTO_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";

/// Typed parsing and verification errors. [`SigError::code`] gives the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigError {
    /// `sig.malformed`: the envelope or key is not well formed or exceeds a limit.
    Malformed(&'static str),
    /// `sig.invalid`: no signature verifies, or a signature is not acceptable
    /// (high S, non-canonical DER, small-order key, wrong length).
    Invalid,
    /// `sig.unsupported_algorithm`: a key type other than ECDSA P-256 or Ed25519.
    UnsupportedAlgorithm,
}

impl SigError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            SigError::Malformed(_) => "sig.malformed",
            SigError::Invalid => "sig.invalid",
            SigError::UnsupportedAlgorithm => "sig.unsupported_algorithm",
        }
    }
}

impl fmt::Display for SigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SigError::Malformed(why) => write!(f, "sig.malformed: {why}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for SigError {}
