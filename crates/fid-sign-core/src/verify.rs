//! Envelope verification.

use crate::{Envelope, Fingerprint, PublicKey, SigError};

/// Verifies `envelope` against `key` over PAE(payloadType, payload).
///
/// Signatures whose `keyid` is non-empty and differs from the key's fingerprint are
/// skipped. Returns the key's fingerprint when at least one remaining signature is valid,
/// and `sig.invalid` otherwise (REQ-TRUST.SIGN.DSSE-007).
pub fn verify(envelope: &Envelope, key: &PublicKey) -> Result<Fingerprint, SigError> {
    let fingerprint = key.fingerprint();
    let message = envelope.pae();
    for sig in &envelope.signatures {
        if !sig.keyid.is_empty() && sig.keyid != fingerprint.as_str() {
            continue;
        }
        if key.verify_raw(&message, &sig.sig).is_ok() {
            return Ok(fingerprint);
        }
    }
    Err(SigError::Invalid)
}

/// Parses envelope JSON and a `PUBLIC KEY` PEM, then [`verify`]. Errors in that order:
/// `sig.malformed` (envelope or key), `sig.unsupported_algorithm` (key type),
/// `sig.invalid`.
pub fn verify_json(envelope_json: &[u8], public_key_pem: &str) -> Result<Fingerprint, SigError> {
    let envelope = Envelope::from_json(envelope_json)?;
    let key = PublicKey::from_pem(public_key_pem)?;
    verify(&envelope, &key)
}
