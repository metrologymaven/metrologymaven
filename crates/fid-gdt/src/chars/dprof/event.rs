//! Journal events of `gdt.publish_defaults_profile@1` and `gdt.repoint_defaults_profile@1`.
//!
//! ```json
//! { "command": "gdt.publish_defaults_profile@1", "file": "root://imports/p.json",
//!   "principal": "human", "branch": "working", "ref": "dp:asme-y14.5-2018@1.0.0#sha256:.." }
//! { "command": "gdt.repoint_defaults_profile@1", "principal": "human", "branch": "working",
//!   "to": "dp:..", "changes": [ { "char": "c1", "old": "dp:..", "new": "dp:.." } ] }
//! ```
//!
//! Serialised as RFC 8785 canonical JSON. The profile content is not in the event: the
//! digest in `ref` addresses it, and replay fetches it by digest ([`super::ProfileContent`]).

use super::ident::{CharId, ProfileRef};
use super::store::{parse_principal, Branch};
use super::{DprofError, MAX_FILE_PARAM_LEN, PUBLISH_COMMAND, REPOINT_COMMAND};
use fid_canon::CValue;
use fid_io_reg::Principal;
use std::collections::BTreeMap;

/// Largest event accepted by [`DprofEvent::from_canonical_bytes`], in bytes.
pub const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_EVENT_DEPTH: usize = 3;

/// One characteristic's change in a re-point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The characteristic.
    pub char: CharId,
    /// Its reference before.
    pub old: ProfileRef,
    /// Its reference after.
    pub new: ProfileRef,
}

impl Change {
    /// `{ "char": .., "old": .., "new": .. }`.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("char".to_owned(), s(self.char.as_str()));
        m.insert("old".to_owned(), s(&self.old.to_string()));
        m.insert("new".to_owned(), s(&self.new.to_string()));
        CValue::Obj(m)
    }
}

/// The event of one publish that stored a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishEvent {
    /// The `file` parameter as given.
    pub file: String,
    /// Who published.
    pub principal: Principal,
    /// The branch written.
    pub branch: Branch,
    /// The stored profile.
    pub reference: ProfileRef,
}

/// The event of one re-point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepointEvent {
    /// Who re-pointed.
    pub principal: Principal,
    /// The branch written.
    pub branch: Branch,
    /// The target profile.
    pub to: ProfileRef,
    /// Every named characteristic, sorted by ID.
    pub changes: Vec<Change>,
}

/// A journal event of this leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DprofEvent {
    /// `gdt.publish_defaults_profile@1`.
    Publish(PublishEvent),
    /// `gdt.repoint_defaults_profile@1`.
    Repoint(RepointEvent),
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn bad(reason: &'static str) -> DprofError {
    DprofError::Event { reason }
}

type Obj = BTreeMap<String, CValue>;

fn keys<'a>(v: &'a CValue, want: &[&str]) -> Result<&'a Obj, DprofError> {
    match v {
        CValue::Obj(o) if o.len() == want.len() && want.iter().all(|k| o.contains_key(*k)) => Ok(o),
        CValue::Obj(_) => Err(bad("bad_keys")),
        _ => Err(bad("wrong_type")),
    }
}

fn text<'a>(o: &'a Obj, k: &str) -> Result<&'a str, DprofError> {
    match o.get(k) {
        Some(CValue::Str(v)) => Ok(v),
        _ => Err(bad("wrong_type")),
    }
}

fn reference(o: &Obj, k: &str) -> Result<ProfileRef, DprofError> {
    ProfileRef::parse(text(o, k)?).ok_or_else(|| bad("bad_ref"))
}

fn who(o: &Obj) -> Result<(Principal, Branch), DprofError> {
    let p = parse_principal(text(o, "principal")?).ok_or_else(|| bad("bad_principal"))?;
    let b = Branch::parse(text(o, "branch")?).ok_or_else(|| bad("bad_branch"))?;
    if Branch::for_principal(p) != b {
        return Err(bad("branch_principal_mismatch"));
    }
    Ok((p, b))
}

impl DprofEvent {
    /// The command ID.
    pub fn command(&self) -> &'static str {
        match self {
            DprofEvent::Publish(_) => PUBLISH_COMMAND,
            DprofEvent::Repoint(_) => REPOINT_COMMAND,
        }
    }

    /// The event as a canonical-JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("command".to_owned(), s(self.command()));
        match self {
            DprofEvent::Publish(e) => {
                m.insert("file".to_owned(), s(&e.file));
                m.insert("principal".to_owned(), s(e.principal.as_str()));
                m.insert("branch".to_owned(), s(e.branch.as_str()));
                m.insert("ref".to_owned(), s(&e.reference.to_string()));
            }
            DprofEvent::Repoint(e) => {
                m.insert("principal".to_owned(), s(e.principal.as_str()));
                m.insert("branch".to_owned(), s(e.branch.as_str()));
                m.insert("to".to_owned(), s(&e.to.to_string()));
                m.insert("changes".to_owned(), CValue::Arr(e.changes.iter().map(Change::to_cvalue).collect()));
            }
        }
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, DprofError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| bad("encode"))
    }

    /// Reads an event back strictly: known command, every key present and no other,
    /// branch consistent with principal, re-point changes sorted by unique characteristic
    /// and all naming `to` as new. Fails `event_invalid{reason}`.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<DprofEvent, DprofError> {
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(bad("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad("json"))?;
        let command = match &v {
            CValue::Obj(o) => text(o, "command")?,
            _ => return Err(bad("wrong_type")),
        };
        if command == PUBLISH_COMMAND {
            let o = keys(&v, &["branch", "command", "file", "principal", "ref"])?;
            let file = text(o, "file")?;
            if file.is_empty() || file.len() > MAX_FILE_PARAM_LEN {
                return Err(bad("bad_file"));
            }
            let (principal, branch) = who(o)?;
            return Ok(DprofEvent::Publish(PublishEvent {
                file: file.to_owned(),
                principal,
                branch,
                reference: reference(o, "ref")?,
            }));
        }
        if command == REPOINT_COMMAND {
            let o = keys(&v, &["branch", "changes", "command", "principal", "to"])?;
            let (principal, branch) = who(o)?;
            let to = reference(o, "to")?;
            let list = match o.get("changes") {
                Some(CValue::Arr(a)) => a,
                _ => return Err(bad("wrong_type")),
            };
            if list.is_empty() || list.len() > super::store::MAX_REPOINT_CHARS {
                return Err(bad("bad_changes"));
            }
            let mut changes: Vec<Change> = Vec::with_capacity(list.len());
            for c in list {
                let c = keys(c, &["char", "new", "old"])?;
                let ch = Change {
                    char: CharId::parse(text(c, "char")?).ok_or_else(|| bad("bad_char_id"))?,
                    old: reference(c, "old")?,
                    new: reference(c, "new")?,
                };
                if ch.new != to {
                    return Err(bad("bad_changes"));
                }
                if changes.last().is_some_and(|prev| prev.char >= ch.char) {
                    return Err(bad("unsorted_changes"));
                }
                changes.push(ch);
            }
            return Ok(DprofEvent::Repoint(RepointEvent { principal, branch, to, changes }));
        }
        Err(bad("unknown_command"))
    }
}
