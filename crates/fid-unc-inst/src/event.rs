//! The journal events of the two write commands, as canonical JSON (RFC 8785).
//!
//! `unc.import_instrument_model@1`:
//!
//! ```text
//! {"author_kind","blob","blob_size","branch","by","command":"unc.import_instrument_model@1",
//!  "digest","model"}
//! ```
//!
//! `digest` is the model digest, `blob`/`blob_size` the CORE.STO.CAS address and length of its
//! canonical bytes (for a model up to 8 MiB the address equals the digest), `author_kind` and
//! `by` the kind and identity of the bus-derived principal, `branch` the branch of the commit.
//!
//! `unc.bind_instrument_model@1`:
//!
//! ```text
//! {"author_kind","branch","by","command":"unc.bind_instrument_model@1","model","model_digest",
//!  "target":"project"}
//! {"author_kind","branch","by","command":"unc.bind_instrument_model@1","descriptor_digest",
//!  "device","device_instance_digest","evidence","evidence_size","model","model_digest",
//!  "target":"device"}
//! ```
//!
//! For a device, `descriptor_digest` is the `instrument-profile/v1` digest and
//! `device_instance_digest` the SHA-256 of the canonical `DeviceInstance`; `evidence` and
//! `evidence_size` address the blob holding both documents, so a replay re-derives the digests
//! from recorded bytes instead of from the live device.
//!
//! The reader is strict: unknown, missing or mistyped members and bytes that are not exactly
//! this canonical form are refused. The blobs an event names are permanent GC roots of the
//! content-addressed store (as for `unc.create_model@1`).

use crate::model::split_id;
use crate::store::BindTarget;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_types::Digest;
use fid_unc::model_store::{AuthorKind, BlobKey};
use std::collections::BTreeMap;

/// `unc.import_instrument_model@1`.
pub const IMPORT_INSTRUMENT_MODEL: &str = "unc.import_instrument_model@1";
/// `unc.bind_instrument_model@1`.
pub const BIND_INSTRUMENT_MODEL: &str = "unc.bind_instrument_model@1";

/// Largest event, in bytes.
pub const MAX_EVENT_BYTES: usize = 4096;
const MAX_EVENT_DEPTH: usize = 2;
/// Longest principal identity, branch or device ID, in bytes.
const MAX_NAME_BYTES: usize = 256;
/// Largest blob an event may name: well above the model input limit (1 MiB) and any evidence
/// document (a descriptor of at most 1 MiB plus the device identity).
const MAX_BLOB_BYTES: u64 = 1 << 24;

/// One committed import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEvent {
    /// `im:<name>@<semver>`.
    pub model: String,
    /// The model digest.
    pub digest: Digest,
    /// Where the canonical bytes are.
    pub blob: BlobKey,
    /// The author principal kind.
    pub author_kind: AuthorKind,
    /// The bus-derived principal identity.
    pub by: String,
    /// The branch.
    pub branch: String,
}

/// What a bind event recorded about a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceFacts {
    /// The device ID.
    pub device: String,
    /// SHA-256 of the canonical `DeviceInstance`.
    pub device_instance_digest: Digest,
    /// The `instrument-profile/v1` descriptor digest.
    pub descriptor_digest: Digest,
    /// The evidence blob.
    pub evidence: BlobKey,
}

/// One committed binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindEvent {
    /// `im:<name>@<semver>`.
    pub model: String,
    /// The bound model's digest.
    pub model_digest: Digest,
    /// For a device target, its facts; `None` for the project default.
    pub device: Option<DeviceFacts>,
    /// The author principal kind.
    pub author_kind: AuthorKind,
    /// The bus-derived principal identity.
    pub by: String,
    /// The branch.
    pub branch: String,
}

impl BindEvent {
    /// The target.
    pub fn target(&self) -> BindTarget {
        match &self.device {
            Some(d) => BindTarget::Device(d.device.clone()),
            None => BindTarget::Project,
        }
    }
}

/// A journal event of this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstEvent {
    /// `unc.import_instrument_model@1`.
    Import(ImportEvent),
    /// `unc.bind_instrument_model@1`.
    Bind(BindEvent),
}

/// Why event bytes were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadEvent(pub &'static str);

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn size(n: u64) -> CValue {
    // At most MAX_BLOB_BYTES (16 MiB), exact in an f64.
    CValue::Num(n as f64)
}

impl InstEvent {
    /// The command ID with major.
    pub fn command(&self) -> &'static str {
        match self {
            InstEvent::Import(_) => IMPORT_INSTRUMENT_MODEL,
            InstEvent::Bind(_) => BIND_INSTRUMENT_MODEL,
        }
    }

    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("command".to_owned(), text(self.command()));
        let (kind, by, branch) = match self {
            InstEvent::Import(e) => {
                o.insert("blob".to_owned(), text(e.blob.address.as_str()));
                o.insert("blob_size".to_owned(), size(e.blob.size));
                o.insert("digest".to_owned(), text(e.digest.as_str()));
                o.insert("model".to_owned(), text(&e.model));
                (e.author_kind, &e.by, &e.branch)
            }
            InstEvent::Bind(e) => {
                o.insert("model".to_owned(), text(&e.model));
                o.insert("model_digest".to_owned(), text(e.model_digest.as_str()));
                o.insert("target".to_owned(), text(e.target().kind()));
                if let Some(d) = &e.device {
                    o.insert("descriptor_digest".to_owned(), text(d.descriptor_digest.as_str()));
                    o.insert("device".to_owned(), text(&d.device));
                    o.insert("device_instance_digest".to_owned(), text(d.device_instance_digest.as_str()));
                    o.insert("evidence".to_owned(), text(d.evidence.address.as_str()));
                    o.insert("evidence_size".to_owned(), size(d.evidence.size));
                }
                (e.author_kind, &e.by, &e.branch)
            }
        };
        o.insert("author_kind".to_owned(), text(kind.as_str()));
        o.insert("branch".to_owned(), text(branch));
        o.insert("by".to_owned(), text(by));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline. A writer failure is reported, never turned
    /// into empty bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, BadEvent> {
        to_canonical_bytes(&self.to_value()).map_err(|_| BadEvent("the event has no canonical form"))
    }

    /// Reads recorded bytes; they must be the canonical form of a valid event.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<InstEvent, BadEvent> {
        let v = parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| BadEvent("not JSON"))?;
        let CValue::Obj(o) = v else { return Err(BadEvent("not an object")) };
        let s = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(BadEvent("missing or mistyped key")),
        };
        let digest = |k: &str| Digest::parse(s(k)?).map_err(|_| BadEvent("bad digest"));
        let name_of = |k: &str| {
            let v = s(k)?;
            if v.is_empty() || v.len() > MAX_NAME_BYTES || v.chars().any(char::is_control) {
                Err(BadEvent("bad name"))
            } else {
                Ok(v.to_owned())
            }
        };
        let blob = |k: &str, sk: &str| {
            let n = match o.get(sk) {
                Some(CValue::Num(n)) => *n,
                _ => return Err(BadEvent("missing blob size")),
            };
            let v = n as u64;
            if v as f64 != n || v == 0 || v > MAX_BLOB_BYTES {
                return Err(BadEvent("bad blob size"));
            }
            Ok(BlobKey { address: digest(k)?, size: v })
        };
        let model = s("model")?;
        if split_id(model).is_none() {
            return Err(BadEvent("bad model"));
        }
        let author_kind = AuthorKind::parse(s("author_kind")?).ok_or(BadEvent("bad author_kind"))?;
        let (by, branch) = (name_of("by")?, name_of("branch")?);
        let e = match s("command")? {
            IMPORT_INSTRUMENT_MODEL => InstEvent::Import(ImportEvent {
                model: model.to_owned(),
                digest: digest("digest")?,
                blob: blob("blob", "blob_size")?,
                author_kind,
                by,
                branch,
            }),
            BIND_INSTRUMENT_MODEL => {
                let device = match s("target")? {
                    "project" => None,
                    "device" => Some(DeviceFacts {
                        device: name_of("device")?,
                        device_instance_digest: digest("device_instance_digest")?,
                        descriptor_digest: digest("descriptor_digest")?,
                        evidence: blob("evidence", "evidence_size")?,
                    }),
                    _ => return Err(BadEvent("bad target")),
                };
                InstEvent::Bind(BindEvent {
                    model: model.to_owned(),
                    model_digest: digest("model_digest")?,
                    device,
                    author_kind,
                    by,
                    branch,
                })
            }
            _ => return Err(BadEvent("unknown command")),
        };
        // Strict: the bytes must be exactly what this event writes (no extra keys, canonical).
        if e.to_canonical_bytes()?.as_slice() != bytes {
            return Err(BadEvent("not the canonical form of an event"));
        }
        Ok(e)
    }
}
