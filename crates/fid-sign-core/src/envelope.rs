//! The DSSE JSON envelope.

use fid_trust_canon::Value;

use crate::{b64, pae, SigError};

/// Largest accepted envelope JSON, in bytes (REQ-TRUST.SIGN.DSSE-006).
pub const MAX_ENVELOPE_BYTES: usize = 64 * 1024 * 1024;
/// Most signatures accepted in one envelope (REQ-TRUST.SIGN.DSSE-006).
pub const MAX_SIGNATURES: usize = 64;

/// A DSSE v1.0 envelope with its payload and signatures decoded from base64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub payload: Vec<u8>,
    pub payload_type: String,
    pub signatures: Vec<Sig>,
}

/// One signature. `keyid` is the signer's [`Fingerprint`](crate::Fingerprint) in
/// envelopes this crate emits; DSSE makes it optional, so a parsed envelope may carry an
/// empty or foreign `keyid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sig {
    pub keyid: String,
    pub sig: Vec<u8>,
}

fn string(value: Option<Value>, missing: &'static str) -> Result<String, SigError> {
    match value {
        Some(Value::String(s)) => Ok(s),
        _ => Err(SigError::Malformed(missing)),
    }
}

impl Envelope {
    /// The PAE bytes the signatures are computed over (REQ-TRUST.SIGN.DSSE-007).
    pub fn pae(&self) -> Vec<u8> {
        pae(self.payload_type.as_bytes(), &self.payload)
    }

    /// Parses envelope JSON. Fails with `sig.malformed`, never panics, when the input is
    /// larger than [`MAX_ENVELOPE_BYTES`], is not JSON (duplicate member names included),
    /// lacks `payload`, `payloadType` or `signatures`, has zero or more than
    /// [`MAX_SIGNATURES`] signatures, or holds base64 that is not canonical padded
    /// standard base64. Unknown members are ignored.
    pub fn from_json(json: &[u8]) -> Result<Envelope, SigError> {
        if json.len() > MAX_ENVELOPE_BYTES {
            return Err(SigError::Malformed("envelope larger than 64 MiB"));
        }
        let members = match fid_trust_canon::parse(json) {
            Ok(Value::Object(members)) => members,
            Ok(_) => return Err(SigError::Malformed("envelope is not a JSON object")),
            Err(_) => return Err(SigError::Malformed("envelope is not valid JSON")),
        };
        let (mut payload, mut payload_type, mut signatures) = (None, None, None);
        for (name, value) in members {
            match name.as_str() {
                "payload" => payload = Some(value),
                "payloadType" => payload_type = Some(value),
                "signatures" => signatures = Some(value),
                _ => {}
            }
        }
        let payload = string(payload, "payload missing or not a string")?;
        let payload = b64::decode(payload.as_bytes()).ok_or(SigError::Malformed("payload is not canonical base64"))?;
        let payload_type = string(payload_type, "payloadType missing or not a string")?;
        let list = match signatures {
            Some(Value::Array(list)) => list,
            _ => return Err(SigError::Malformed("signatures missing or not an array")),
        };
        if list.is_empty() {
            return Err(SigError::Malformed("no signatures"));
        }
        if list.len() > MAX_SIGNATURES {
            return Err(SigError::Malformed("more than 64 signatures"));
        }
        let mut sigs = Vec::with_capacity(list.len());
        for item in list {
            let Value::Object(members) = item else {
                return Err(SigError::Malformed("signature is not an object"));
            };
            let (mut keyid, mut sig) = (None, None);
            for (name, value) in members {
                match name.as_str() {
                    "keyid" => keyid = Some(value),
                    "sig" => sig = Some(value),
                    _ => {}
                }
            }
            let keyid = match keyid {
                None => String::new(),
                some => string(some, "keyid is not a string")?,
            };
            let sig = string(sig, "sig missing or not a string")?;
            let sig = b64::decode(sig.as_bytes()).ok_or(SigError::Malformed("sig is not canonical base64"))?;
            sigs.push(Sig { keyid, sig });
        }
        Ok(Envelope { payload, payload_type, signatures: sigs })
    }

    /// Writes the envelope as RFC 8785 canonical JSON (members sorted, no whitespace,
    /// no trailing newline): `{"payload":..,"payloadType":..,"signatures":[{"keyid":..,"sig":..}]}`.
    /// Fails with `sig.malformed` when the result could not be parsed back by
    /// [`Envelope::from_json`] (no signatures, too many, or too large).
    pub fn to_json(&self) -> Result<Vec<u8>, SigError> {
        if self.signatures.is_empty() {
            return Err(SigError::Malformed("no signatures"));
        }
        if self.signatures.len() > MAX_SIGNATURES {
            return Err(SigError::Malformed("more than 64 signatures"));
        }
        let sigs = self
            .signatures
            .iter()
            .map(|s| {
                Value::Object(vec![
                    ("keyid".to_owned(), Value::String(s.keyid.clone())),
                    ("sig".to_owned(), Value::String(b64::encode(&s.sig))),
                ])
            })
            .collect();
        let doc = Value::Object(vec![
            ("payload".to_owned(), Value::String(b64::encode(&self.payload))),
            ("payloadType".to_owned(), Value::String(self.payload_type.clone())),
            ("signatures".to_owned(), Value::Array(sigs)),
        ]);
        let out = fid_trust_canon::serialize(&doc).map_err(|_| SigError::Malformed("envelope cannot be serialised"))?;
        if out.len() > MAX_ENVELOPE_BYTES {
            return Err(SigError::Malformed("envelope larger than 64 MiB"));
        }
        Ok(out)
    }
}
