//! The journal event of a lifecycle transition, as canonical JSON (RFC 8785):
//!
//! ```text
//! {"artifact":"<id>","branch":"<branch>","by":"<principal id>","command":"program.register@1",
//!  "content":"<canonical bytes>","digest":"sha256:<hex>","note":"<text>",
//!  "source_digest":"sha256:<hex>","state":"registered","version":1}
//! ```
//!
//! `content` is present exactly on `register` events: the frozen canonical bytes of the
//! version (a program's canonical JSON, or a script bundle's), as a JSON string. That makes
//! the journal the durable record of every version's bytes, and replay rebuilds them from it.
//! `note` and `source_digest` are present only when set. `state` is the state after the
//! transition. The principal kind is not in the event: it is in the journal envelope the bus
//! writes, and replay reads it from there. The reader is strict: unknown, missing or mistyped
//! keys and bytes that are not canonical are refused.

use super::bundle::MAX_BUNDLE_BYTES;
use super::command::Verb;
use super::{ArtifactId, LifeState, MAX_NOTE_BYTES, MAX_VERSIONS};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Largest frozen content, in bytes: the larger of a program and a script bundle.
pub const MAX_CONTENT_BYTES: usize =
    if fid_program::MAX_BYTES > MAX_BUNDLE_BYTES { fid_program::MAX_BYTES } else { MAX_BUNDLE_BYTES };
/// Largest event, in bytes. Content is canonical JSON, so as a JSON string each byte takes at
/// most two (`"` and `\` are escaped; control characters are already escaped in it).
pub const MAX_EVENT_BYTES: usize = 8 * 1024 + 6 * MAX_NOTE_BYTES + 2 * MAX_CONTENT_BYTES;
const MAX_EVENT_DEPTH: usize = 2;
/// Longest principal identity or branch name, in bytes (as `fid_bus::principal::MAX_ID_BYTES`).
const MAX_NAME_BYTES: usize = 256;

/// One lifecycle transition, as journaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeEvent {
    /// The command run.
    pub verb: Verb,
    /// The artifact lineage (its kind gives the command domain).
    pub artifact: ArtifactId,
    /// The branch the bus routed the commit to.
    pub branch: String,
    /// The bus-derived principal identity.
    pub by: String,
    /// The version.
    pub version: u32,
    /// The version digest.
    pub digest: Digest,
    /// The state after the transition.
    pub state: LifeState,
    /// The version's source digest, if any.
    pub source_digest: Option<Digest>,
    /// The caller's note, if any.
    pub note: Option<String>,
    /// The frozen canonical bytes, on `register` events only.
    pub content: Option<String>,
}

/// Why event bytes were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadEvent(pub &'static str);

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

impl LifeEvent {
    /// The command ID with major, e.g. `program.register@1`.
    pub fn command(&self) -> &'static str {
        self.verb.command(self.artifact.kind())
    }

    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("artifact".to_owned(), text(&self.artifact.to_string()));
        o.insert("branch".to_owned(), text(&self.branch));
        o.insert("by".to_owned(), text(&self.by));
        o.insert("command".to_owned(), text(self.command()));
        if let Some(c) = &self.content {
            o.insert("content".to_owned(), text(c));
        }
        o.insert("digest".to_owned(), text(self.digest.as_str()));
        if let Some(n) = &self.note {
            o.insert("note".to_owned(), text(n));
        }
        if let Some(s) = &self.source_digest {
            o.insert("source_digest".to_owned(), text(s.as_str()));
        }
        o.insert("state".to_owned(), text(self.state.as_str()));
        o.insert("version".to_owned(), CValue::Num(f64::from(self.version)));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// Reads recorded bytes; they must be the canonical form of a valid event.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<LifeEvent, BadEvent> {
        let v = parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| BadEvent("not JSON"))?;
        let CValue::Obj(o) = v else { return Err(BadEvent("not an object")) };
        let s = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(BadEvent("missing or mistyped key")),
        };
        let opt = |k: &str| match o.get(k) {
            None => Ok(None),
            Some(CValue::Str(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(BadEvent("mistyped key")),
        };
        let (kind, verb) = Verb::from_command(s("command")?).ok_or(BadEvent("unknown command"))?;
        let artifact = ArtifactId::parse(kind, s("artifact")?).map_err(|_| BadEvent("bad artifact"))?;
        let name = |k: &str| {
            let v = s(k)?;
            if v.is_empty() || v.len() > MAX_NAME_BYTES {
                Err(BadEvent("bad name"))
            } else {
                Ok(v.to_owned())
            }
        };
        let digest = |x: &str| Digest::parse(x).map_err(|_| BadEvent("bad digest"));
        let version = match o.get("version") {
            Some(CValue::Num(n)) => {
                let v = *n as u32;
                if f64::from(v) != *n || v == 0 || v > MAX_VERSIONS {
                    return Err(BadEvent("bad version"));
                }
                v
            }
            _ => return Err(BadEvent("missing version")),
        };
        let content = opt("content")?.map(str::to_owned);
        if content.is_some() != (verb == Verb::Register) {
            return Err(BadEvent("content belongs to register events only"));
        }
        if content.as_ref().is_some_and(|c| c.len() > MAX_CONTENT_BYTES) {
            return Err(BadEvent("content too long"));
        }
        let note = opt("note")?.map(str::to_owned);
        if note.as_ref().is_some_and(|n| n.len() > MAX_NOTE_BYTES) {
            return Err(BadEvent("note too long"));
        }
        let e = LifeEvent {
            verb,
            artifact,
            branch: name("branch")?,
            by: name("by")?,
            version,
            digest: digest(s("digest")?)?,
            state: LifeState::parse(s("state")?).ok_or(BadEvent("bad state"))?,
            source_digest: opt("source_digest")?.map(digest).transpose()?,
            note,
            content,
        };
        if e.state != verb.to_state() {
            return Err(BadEvent("state does not match the command"));
        }
        // Strict: the bytes must be exactly what this event writes (no extra keys, canonical).
        if e.to_canonical_bytes().as_slice() != bytes {
            return Err(BadEvent("not the canonical form of an event"));
        }
        Ok(e)
    }
}
