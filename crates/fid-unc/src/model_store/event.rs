//! The journal event of `unc.create_model@1`, as canonical JSON (RFC 8785):
//!
//! ```text
//! {"author_kind":"human","blob":"sha256:<hex>","blob_size":4252,"branch":"main","by":"alice",
//!  "command":"unc.create_model@1","digest":"sha256:<hex>","kernel_manifest_digest":"sha256:<hex>",
//!  "model":"um:bore-ls@1.0.0"}
//! ```
//!
//! `digest` is the model digest, `blob` and `blob_size` the CORE.STO.CAS address and length of
//! its canonical bytes (for a model of at most 8 MiB the address equals the digest),
//! `kernel_manifest_digest` the digest of the release manifest it was validated against,
//! `author_kind` and `by` the kind and identity of the bus-derived principal, `branch` the
//! branch the bus routed the commit to. The model bytes are in the content-addressed store,
//! not in the event. The reader is strict: unknown, missing or mistyped keys and bytes that are
//! not exactly this canonical form are refused.

use super::{AuthorKind, BlobKey};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_kunc::model::{is_model_name, is_semver, MAX_INPUT_BYTES};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Largest event, in bytes.
pub const MAX_EVENT_BYTES: usize = 4096;
const MAX_EVENT_DEPTH: usize = 2;
/// Longest principal identity or branch name, in bytes (as `fid_bus::principal::MAX_ID_BYTES`).
const MAX_NAME_BYTES: usize = 256;

/// One committed model creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateModelEvent {
    /// The model name (without `um:`).
    pub name: String,
    /// The model semver.
    pub semver: String,
    /// The model digest.
    pub digest: Digest,
    /// Where the canonical bytes are stored.
    pub blob: BlobKey,
    /// The author principal kind.
    pub author_kind: AuthorKind,
    /// The bus-derived principal identity.
    pub by: String,
    /// The branch.
    pub branch: String,
    /// The release manifest the model was validated against.
    pub kernel_manifest_digest: Digest,
}

/// Why event bytes were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadEvent(pub &'static str);

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

impl CreateModelEvent {
    /// `um:<name>@<semver>`.
    pub fn model(&self) -> String {
        format!("um:{}@{}", self.name, self.semver)
    }

    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("author_kind".to_owned(), text(self.author_kind.as_str()));
        o.insert("blob".to_owned(), text(self.blob.address.as_str()));
        // At most MAX_INPUT_BYTES (16 MiB), exact in an f64.
        o.insert("blob_size".to_owned(), CValue::Num(self.blob.size as f64));
        o.insert("branch".to_owned(), text(&self.branch));
        o.insert("by".to_owned(), text(&self.by));
        o.insert("command".to_owned(), text(super::CREATE_MODEL));
        o.insert("digest".to_owned(), text(self.digest.as_str()));
        o.insert("kernel_manifest_digest".to_owned(), text(self.kernel_manifest_digest.as_str()));
        o.insert("model".to_owned(), text(&self.model()));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// Reads recorded bytes; they must be the canonical form of a valid event.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<CreateModelEvent, BadEvent> {
        let v = parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| BadEvent("not JSON"))?;
        let CValue::Obj(o) = v else { return Err(BadEvent("not an object")) };
        let s = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(BadEvent("missing or mistyped key")),
        };
        if s("command")? != super::CREATE_MODEL {
            return Err(BadEvent("unknown command"));
        }
        let digest = |k: &str| Digest::parse(s(k)?).map_err(|_| BadEvent("bad digest"));
        let name_of = |k: &str| {
            let v = s(k)?;
            if v.is_empty() || v.len() > MAX_NAME_BYTES || v.chars().any(char::is_control) {
                Err(BadEvent("bad name"))
            } else {
                Ok(v.to_owned())
            }
        };
        let (name, semver) = s("model")?
            .strip_prefix("um:")
            .and_then(|r| r.split_once('@'))
            .filter(|(n, v)| is_model_name(n) && is_semver(v))
            .ok_or(BadEvent("bad model"))?;
        let size = match o.get("blob_size") {
            Some(CValue::Num(n)) => {
                let v = *n as u64;
                if v as f64 != *n || v == 0 || v > MAX_INPUT_BYTES as u64 {
                    return Err(BadEvent("bad blob_size"));
                }
                v
            }
            _ => return Err(BadEvent("missing blob_size")),
        };
        let e = CreateModelEvent {
            name: name.to_owned(),
            semver: semver.to_owned(),
            digest: digest("digest")?,
            blob: BlobKey { address: digest("blob")?, size },
            author_kind: AuthorKind::parse(s("author_kind")?).ok_or(BadEvent("bad author_kind"))?,
            by: name_of("by")?,
            branch: name_of("branch")?,
            kernel_manifest_digest: digest("kernel_manifest_digest")?,
        };
        // Strict: the bytes must be exactly what this event writes (no extra keys, canonical).
        if e.to_canonical_bytes().as_slice() != bytes {
            return Err(BadEvent("not the canonical form of an event"));
        }
        Ok(e)
    }
}
