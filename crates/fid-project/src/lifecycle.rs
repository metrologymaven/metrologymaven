//! Project identity: the genesis records and their verifier. Spec node `CORE.PRJ.CONT.LIFE`
//! (the store-free half; the five lifecycle commands and their store calls belong in
//! `fid-project-cmd`, layer engine).
//!
//! # Records (REQ-008)
//!
//! - [`IdentityRecord`]: the RFC 8785 object
//!   `{"format": "fid-project-identity/1", "namespace": <uuid>, "project": <uuidv7>,
//!   "source"?: {"head": <sha256 digest>, "project": <uuidv7>}}`. Only a genesis carries one:
//!   the `project.create_project@1` entry of a new project (no `source`) or the
//!   `project.save_project_as@1` entry that starts a copy's own journal (with `source`).
//! - [`SavedAsRecord`]: `{"copy": <uuidv7>, "format": "fid-project-saved-as/1",
//!   "head": <sha256 digest>, "project": <uuidv7>}`, the source-side save-as entry.
//!
//! Each record is a CAS blob attached to its entry. The entry's header outputs hold exactly
//! one output, whose `entity` is a project ID and whose `digest` is the SHA-256 of the record's
//! canonical bytes (the header-level marker). The record is read only after its bytes re-hash
//! to that digest. [`read_record`] tells a record from any other blob by its `format` member:
//! a blob that is not a small JSON object with one of the two formats is not a record, and a
//! blob that claims one of them but is not exactly its canonical form is `record_malformed`.
//!
//! # The identity read (REQ-009, REQ-CORE.RPL.RECON.FOLD-008)
//!
//! [`IdentityReader`] (and [`read_identity`]) walks the entries of stream `record` in sequence
//! order from sequence 1: each verified entry header (it is checked again here with
//! `fid_journal::verify_header_value`, and its `seq` and `prev` must continue the chain), the
//! entry's payload when it is stored, and `promoted_from` from the commit stage's record.
//! Identity is taken from verified headers and the records they commit to, never from an event
//! body. Every refusal is fatal (FOLD-008 (f)): the read fails closed with an
//! [`IdentityError`] naming the entry's sequence and hash.
//!
//! An entry is an *identity entry* if its command is `project.create_project@1` or
//! `project.save_project_as@1`, if its stored payload holds one of the identity effects
//! (`ProjectIdentified`, `ProjectCopiedFrom`, `ProjectSavedAs`), or if one of its header outputs
//! (with an entity, not the tombstone) names a stored blob that is a record. Every other entry
//! is passed over; its payload is not read beyond the events commitment check. Per identity
//! entry, in this order (the first failing rule is the refusal):
//!
//! 1. the command is one of the two owners (`foreign_command`; FOLD-008 (a), (b));
//! 2. the entry is on branch `main` (`off_main`) and is not a promoted copy (`promoted`) (e);
//! 3. its payload is stored (`payload_missing`; without it (c) cannot be checked);
//! 4. no event is a `Request*` event (`request`) (c);
//! 5. exactly one output, one record and one event (`bad_shape`); the class is the command's
//!    (W2 for create, W1 for save-as), the determinism D0, and the output's entity the record's
//!    `project` (`bad_shape`);
//! 6. command, record and event belong together (`record_kind`): create with an identity
//!    record without `source` and `ProjectIdentified`; save-as with an identity record with
//!    `source` and `ProjectCopiedFrom` (the copy's genesis); save-as with a saved-as record and
//!    `ProjectSavedAs` (the source side). So a source-side entry claiming an identity record
//!    (re-identification of the source) is refused here;
//! 7. the event body equals the record (`body_mismatch`): bodies are checked, never read;
//! 8. a genesis (create or copy): its previous entry is a `session_header` on `main`
//!    (`not_after_session`); no earlier genesis followed a session header with the same
//!    `session_id` (`second_genesis`); its principal kind and commitment equal the session
//!    header's (`principal_mismatch`) (a);
//! 9. a create genesis: the session header is at sequence 1 and the genesis at sequence 2
//!    (`create_position`), decided against the empty journal (d);
//! 10. a copy genesis: an identity exists; `source.project` is it, `source.head` is the
//!     session header's `prev`, the namespace is unchanged and the new project ID was never
//!     this journal's (`source_mismatch`) (d);
//! 11. a source-side save-as: an identity exists and is the record's `project`
//!     (`wrong_project`); the record's `head` is the entry's own `prev`, the head it was decided
//!     against under the commit lock (`stale_head`, which also refuses a replayed
//!     `ProjectSavedAs`) (d); its `copy` is new to this journal (`duplicate_copy`).
//!
//! The identity is the latest valid genesis; every earlier one is the identity the next names
//! as its source. Only append order on `main` counts: head moves (undo) and other branches never
//! change it. A journal without a genesis is `no_genesis`.
//!
//! The check of an entry's events against its header's `events_commit` (salt and value from the
//! payload, `CORE.JRN.ENTRY` REQ-003) is done here for every command entry with a stored
//! payload; a mismatch or an event that is not a `{"body", "type"}` envelope is
//! `payload_mismatch`. An entry whose payload was removed hides only its events; its header and
//! record marker are still checked, and an identity entry without payload is refused.

use crate::id::Uuid;
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, Digest};
use fid_journal::entry::{commitment, parse_digest, verify_header_value, Salt, GENESIS_PREV, TOMBSTONE};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// `project.create_project` (major 1): W2, the genesis of a new project.
pub const CREATE_PROJECT: &str = "project.create_project";
/// `project.open_project` (major 1): R0.
pub const OPEN_PROJECT: &str = "project.open_project";
/// `project.close_project` (major 1): R0.
pub const CLOSE_PROJECT: &str = "project.close_project";
/// `project.save_project_as` (major 1): W1, the copy's genesis and the source-side entry.
pub const SAVE_PROJECT_AS: &str = "project.save_project_as";
/// `project.list_projects` (major 1): R0.
pub const LIST_PROJECTS: &str = "project.list_projects";
/// The create genesis event; reserved to `project.create_project@1`.
pub const PROJECT_IDENTIFIED: &str = "ProjectIdentified";
/// The copy's genesis event; reserved to `project.save_project_as@1`.
pub const PROJECT_COPIED_FROM: &str = "ProjectCopiedFrom";
/// The source-side save-as event; reserved to `project.save_project_as@1`.
pub const PROJECT_SAVED_AS: &str = "ProjectSavedAs";
/// The three identity effects with their owning command (REQ-010).
pub const IDENTITY_EFFECTS: [(&str, &str); 3] =
    [(PROJECT_COPIED_FROM, SAVE_PROJECT_AS), (PROJECT_IDENTIFIED, CREATE_PROJECT), (PROJECT_SAVED_AS, SAVE_PROJECT_AS)];
/// `format` of an [`IdentityRecord`].
pub const IDENTITY_FORMAT: &str = "fid-project-identity/1";
/// `format` of a [`SavedAsRecord`].
pub const SAVED_AS_FORMAT: &str = "fid-project-saved-as/1";
/// The stream the identity is read from.
pub const RECORD_STREAM: &str = "record";
/// The only branch whose identity entries count.
pub const MAIN: &str = "main";
/// Largest blob [`read_record`] looks at; a larger blob is not a record.
pub const MAX_RECORD_BYTES: usize = 1024;
/// Prefix of the pending-request event types (`CORE.BUS.POL.PROJ`).
pub const REQUEST_PREFIX: &str = "Request";

// ------------------------------------------------------------------ records

/// Why a record could not be built or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError(pub String);

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "record_malformed: {}", self.0)
    }
}

impl std::error::Error for RecordError {}

fn bad(what: impl Into<String>) -> RecordError {
    RecordError(what.into())
}

/// The project a copy was made from: its project ID and the head it was copied at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRef {
    pub project: Uuid,
    pub head: Digest,
}

/// A project ID: an RFC 9562 UUIDv7.
pub fn is_project_id(id: &Uuid) -> bool {
    id.version() == 7 && id.variant_bits() == 0b10
}

/// A namespace UUID: an RFC 9562 UUID that is not nil.
pub fn is_namespace(id: &Uuid) -> bool {
    *id != Uuid::NIL && id.variant_bits() == 0b10
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn canonical(v: &CValue) -> Vec<u8> {
    // Records hold strings only, so canonical encoding cannot fail.
    to_canonical_bytes(v).unwrap_or_default()
}

/// The identity record of a genesis (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityRecord {
    pub project: Uuid,
    pub namespace: Uuid,
    pub source: Option<SourceRef>,
}

impl IdentityRecord {
    /// The record of a new project. `project` must be a UUIDv7 and `namespace` a non-nil UUID.
    pub fn create(project: Uuid, namespace: Uuid) -> Result<IdentityRecord, RecordError> {
        IdentityRecord { project, namespace, source: None }.checked()
    }

    /// The record of a copy of `source`: a new project ID, the source's namespace.
    pub fn copy(project: Uuid, namespace: Uuid, source: SourceRef) -> Result<IdentityRecord, RecordError> {
        if project == source.project {
            return Err(bad("a copy's project ID must differ from its source's"));
        }
        IdentityRecord { project, namespace, source: Some(source) }.checked()
    }

    fn checked(self) -> Result<IdentityRecord, RecordError> {
        if !is_project_id(&self.project) {
            return Err(bad("project is not a UUIDv7"));
        }
        if !is_namespace(&self.namespace) {
            return Err(bad("namespace is not a non-nil RFC 9562 UUID"));
        }
        if let Some(src) = &self.source {
            if !is_project_id(&src.project) {
                return Err(bad("source.project is not a UUIDv7"));
            }
        }
        Ok(self)
    }

    /// The record as its JSON object.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::from([
            ("format".to_string(), s(IDENTITY_FORMAT)),
            ("namespace".to_string(), s(&self.namespace.to_string())),
            ("project".to_string(), s(&self.project.to_string())),
        ]);
        if let Some(src) = &self.source {
            o.insert(
                "source".to_string(),
                obj([("head", s(&src.head.to_string())), ("project", s(&src.project.to_string()))]),
            );
        }
        CValue::Obj(o)
    }

    /// The RFC 8785 bytes stored as the record's blob.
    pub fn to_bytes(&self) -> Vec<u8> {
        canonical(&self.to_value())
    }

    /// The SHA-256 of [`IdentityRecord::to_bytes`]: the header output digest.
    pub fn digest(&self) -> Digest {
        digest_bytes(&self.to_bytes())
    }
}

/// The record of a source-side save-as entry (module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedAsRecord {
    /// The source project, whose journal holds the entry.
    pub project: Uuid,
    /// The copy's new project ID.
    pub copy: Uuid,
    /// The source head the copy was made at, and the entry's own `prev`.
    pub head: Digest,
}

impl SavedAsRecord {
    /// A checked record: both IDs UUIDv7 and different.
    pub fn new(project: Uuid, copy: Uuid, head: Digest) -> Result<SavedAsRecord, RecordError> {
        if !is_project_id(&project) || !is_project_id(&copy) {
            return Err(bad("project and copy must be UUIDv7"));
        }
        if project == copy {
            return Err(bad("copy must differ from project"));
        }
        Ok(SavedAsRecord { project, copy, head })
    }

    /// The record as its JSON object.
    pub fn to_value(&self) -> CValue {
        obj([
            ("copy", s(&self.copy.to_string())),
            ("format", s(SAVED_AS_FORMAT)),
            ("head", s(&self.head.to_string())),
            ("project", s(&self.project.to_string())),
        ])
    }

    /// The RFC 8785 bytes stored as the record's blob.
    pub fn to_bytes(&self) -> Vec<u8> {
        canonical(&self.to_value())
    }

    /// The SHA-256 of [`SavedAsRecord::to_bytes`].
    pub fn digest(&self) -> Digest {
        digest_bytes(&self.to_bytes())
    }
}

/// A record read from a blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record {
    Identity(IdentityRecord),
    SavedAs(SavedAsRecord),
}

impl Record {
    /// The record as its JSON object.
    pub fn to_value(&self) -> CValue {
        match self {
            Record::Identity(r) => r.to_value(),
            Record::SavedAs(r) => r.to_value(),
        }
    }

    /// The project ID the header output must name.
    pub fn project(&self) -> Uuid {
        match self {
            Record::Identity(r) => r.project,
            Record::SavedAs(r) => r.project,
        }
    }
}

fn members(o: &BTreeMap<String, CValue>, required: &[&str], optional: &[&str]) -> Result<(), RecordError> {
    for k in o.keys() {
        if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
            return Err(bad(format!("unknown member {k:?}")));
        }
    }
    for k in required {
        if !o.contains_key(*k) {
            return Err(bad(format!("missing member {k:?}")));
        }
    }
    Ok(())
}

fn uuid_member(o: &BTreeMap<String, CValue>, key: &str) -> Result<Uuid, RecordError> {
    match o.get(key) {
        Some(CValue::Str(t)) => match Uuid::parse(t) {
            Ok(u) if u.to_string() == *t => Ok(u),
            _ => Err(bad(format!("{key} is not a lowercase UUID"))),
        },
        _ => Err(bad(format!("{key} is not a string"))),
    }
}

fn digest_member(o: &BTreeMap<String, CValue>, key: &str) -> Result<Digest, RecordError> {
    match o.get(key) {
        Some(CValue::Str(t)) => parse_digest(t).ok_or_else(|| bad(format!("{key} is not a sha256: digest"))),
        _ => Err(bad(format!("{key} is not a string"))),
    }
}

/// Reads a blob as a record (module docs). `Ok(None)`: not a record (not a small JSON object
/// with a record `format`). `Err`: it claims a record format but is not exactly a valid record
/// in canonical form.
pub fn read_record(bytes: &[u8]) -> Result<Option<Record>, RecordError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Ok(None);
    }
    let Ok(value) = parse_with_limits(bytes, MAX_RECORD_BYTES, 4) else { return Ok(None) };
    let CValue::Obj(o) = &value else { return Ok(None) };
    let Some(CValue::Str(format)) = o.get("format") else { return Ok(None) };
    let record = if format == IDENTITY_FORMAT {
        members(o, &["format", "namespace", "project"], &["source"])?;
        let source = match o.get("source") {
            None => None,
            Some(CValue::Obj(src)) => {
                members(src, &["head", "project"], &[])?;
                Some(SourceRef { project: uuid_member(src, "project")?, head: digest_member(src, "head")? })
            }
            Some(_) => return Err(bad("source is not an object")),
        };
        let r = IdentityRecord { project: uuid_member(o, "project")?, namespace: uuid_member(o, "namespace")?, source };
        let r = r.checked()?;
        if r.source.is_some_and(|src| src.project == r.project) {
            return Err(bad("a copy's project ID must differ from its source's"));
        }
        Record::Identity(r)
    } else if format == SAVED_AS_FORMAT {
        members(o, &["copy", "format", "head", "project"], &[])?;
        Record::SavedAs(SavedAsRecord::new(
            uuid_member(o, "project")?,
            uuid_member(o, "copy")?,
            digest_member(o, "head")?,
        )?)
    } else if format.starts_with("fid-project-identity/") || format.starts_with("fid-project-saved-as/") {
        return Err(bad(format!("unknown record format {format:?}")));
    } else {
        return Ok(None);
    };
    if canonical(&value) != bytes {
        return Err(bad("the record is not in its RFC 8785 form"));
    }
    Ok(Some(record))
}

// ------------------------------------------------------------------ inputs

/// One entry of stream `record` as the journal stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    /// The entry header (JSON value, `hash` included). Verified again by the reader.
    pub header: CValue,
    /// The payload (`{"salts": {...}, <field>: <value>, ...}`), `None` if it was removed.
    pub payload: Option<CValue>,
    /// The entry this one is a promoted copy of, from the commit stage's record
    /// (`CORE.JRN.BR.PROMO`); `None` for an entry committed directly.
    pub promoted_from: Option<Digest>,
}

/// The blob store as the identity read sees it. Implementations read with the safe CAS
/// object read (never a chunk manifest as the object).
pub trait RecordBlobs {
    /// The bytes of the stored object `digest`, or `None` if it is not stored or is larger
    /// than `max_bytes`. The reader re-hashes what it gets.
    fn object(&self, digest: &Digest, max_bytes: usize) -> Result<Option<Vec<u8>>, String>;
}

impl<T: RecordBlobs + ?Sized> RecordBlobs for &T {
    fn object(&self, digest: &Digest, max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
        (**self).object(digest, max_bytes)
    }
}

/// Blobs in memory, keyed by digest (tests, and callers that already hold the records).
impl RecordBlobs for BTreeMap<[u8; 32], Vec<u8>> {
    fn object(&self, digest: &Digest, max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
        Ok(self.get(&digest.0).filter(|b| b.len() <= max_bytes).cloned())
    }
}

// ------------------------------------------------------------------ errors

/// Why the identity read refused (module docs, rule list). Stable codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refusal {
    /// The header fails its schema or hash, or does not continue the chain.
    Chain,
    /// The blob store could not be read.
    Storage,
    /// A record blob does not hash to its header output digest.
    RecordCorrupt,
    /// A blob claims a record format but is not a valid canonical record.
    RecordMalformed,
    /// The events of a command entry do not reproduce its `events_commit`.
    PayloadMismatch,
    /// An identity effect or record under a command that does not own it.
    ForeignCommand,
    /// An identity entry on a branch other than `main`.
    OffMain,
    /// An identity entry that is a promoted copy.
    Promoted,
    /// An identity entry whose payload was removed.
    PayloadMissing,
    /// An identity entry that also carries a `Request*` event.
    Request,
    /// Not exactly one output, record and event, or the wrong class, determinism or entity.
    BadShape,
    /// Command, record and event do not belong together.
    RecordKind,
    /// The event body is not the committed record.
    BodyMismatch,
    /// A genesis whose previous entry is not a session header on `main`.
    NotAfterSession,
    /// A second genesis in a session.
    SecondGenesis,
    /// A genesis whose principal is not its session header's.
    PrincipalMismatch,
    /// A create genesis that is not at sequence 2 after a session header at sequence 1.
    CreatePosition,
    /// A copy genesis that does not name the current identity and head as its source.
    SourceMismatch,
    /// A source-side save-as for another project than the current identity.
    WrongProject,
    /// A source-side save-as not decided against its own `prev` (also a replay).
    StaleHead,
    /// A source-side save-as naming a copy this journal already named.
    DuplicateCopy,
    /// The journal holds no genesis.
    NoGenesis,
}

impl Refusal {
    /// The stable code.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Chain => "chain",
            Refusal::Storage => "storage",
            Refusal::RecordCorrupt => "record_corrupt",
            Refusal::RecordMalformed => "record_malformed",
            Refusal::PayloadMismatch => "payload_mismatch",
            Refusal::ForeignCommand => "foreign_command",
            Refusal::OffMain => "off_main",
            Refusal::Promoted => "promoted",
            Refusal::PayloadMissing => "payload_missing",
            Refusal::Request => "request",
            Refusal::BadShape => "bad_shape",
            Refusal::RecordKind => "record_kind",
            Refusal::BodyMismatch => "body_mismatch",
            Refusal::NotAfterSession => "not_after_session",
            Refusal::SecondGenesis => "second_genesis",
            Refusal::PrincipalMismatch => "principal_mismatch",
            Refusal::CreatePosition => "create_position",
            Refusal::SourceMismatch => "source_mismatch",
            Refusal::WrongProject => "wrong_project",
            Refusal::StaleHead => "stale_head",
            Refusal::DuplicateCopy => "duplicate_copy",
            Refusal::NoGenesis => "no_genesis",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// A refused identity read, naming the entry (FOLD-008 (f)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityError {
    pub refusal: Refusal,
    /// The refused entry's sequence (`None` only for an empty journal's `no_genesis`).
    pub seq: Option<u64>,
    /// The refused entry's hash, when its header got that far.
    pub entry: Option<Digest>,
    pub detail: String,
}

impl IdentityError {
    /// The stable code of [`IdentityError::refusal`].
    pub fn code(&self) -> &'static str {
        self.refusal.code()
    }
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.refusal)?;
        if let Some(seq) = self.seq {
            write!(f, " at entry {seq}")?;
        }
        if let Some(h) = &self.entry {
            write!(f, " ({h})")?;
        }
        write!(f, ": {}", self.detail)
    }
}

impl std::error::Error for IdentityError {}

// ------------------------------------------------------------------ result

/// A valid genesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Genesis {
    pub seq: u64,
    /// The genesis entry's hash.
    pub entry: Digest,
    /// The hash of the session header it follows.
    pub session: Digest,
    pub record: IdentityRecord,
}

/// A valid source-side save-as entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedAs {
    pub seq: u64,
    pub entry: Digest,
    pub record: SavedAsRecord,
}

/// The identity of a project, read from its journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The latest valid genesis: the project's identity.
    pub genesis: Genesis,
    /// Earlier geneses, oldest first: the sources this project was copied from.
    pub lineage: Vec<Genesis>,
    /// Source-side save-as entries, in journal order (also those inherited from a source).
    pub saved_as: Vec<SavedAs>,
    /// The last entry read.
    pub head: Digest,
    /// Its sequence.
    pub head_seq: u64,
}

impl Identity {
    pub fn project(&self) -> Uuid {
        self.genesis.record.project
    }

    pub fn namespace(&self) -> Uuid {
        self.genesis.record.namespace
    }

    /// The project this one was copied from, and the head it was copied at.
    pub fn source(&self) -> Option<SourceRef> {
        self.genesis.record.source
    }
}

// ------------------------------------------------------------------ the reader

/// The facts of the previous entry a genesis needs.
#[derive(Debug, Clone)]
struct Prev {
    hash: Digest,
    seq: u64,
    prev: Digest,
    session: Option<String>,
    branch: String,
    principal: (String, String),
}

/// What one header says, as far as the identity read needs it.
struct Header {
    hash: Digest,
    seq: u64,
    prev: Digest,
    kind: String,
    branch: String,
    principal: (String, String),
    session: Option<String>,
    command: Option<Command>,
}

struct Command {
    id: String,
    major: f64,
    class: String,
    determinism: String,
    events_commit: Digest,
    /// `(entity, digest)` of every output.
    outputs: Vec<(Option<String>, Digest)>,
}

fn field<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Option<&'a CValue> {
    o.get(key)
}

fn text<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Option<&'a str> {
    match field(o, key) {
        Some(CValue::Str(t)) => Some(t),
        _ => None,
    }
}

fn u64_of(v: Option<&CValue>) -> Option<u64> {
    match v {
        Some(CValue::U64(n)) => Some(*n),
        Some(CValue::Str(t)) => t.parse().ok().filter(|n: &u64| n.to_string() == *t),
        _ => None,
    }
}

fn digest_of(v: Option<&CValue>) -> Option<Digest> {
    match v {
        Some(CValue::Str(t)) => parse_digest(t),
        _ => None,
    }
}

fn parse_header(v: &CValue) -> Result<Header, String> {
    let hash = verify_header_value(v).map_err(|e| format!("header: {e}"))?;
    let CValue::Obj(o) = v else { return Err("header is not an object".into()) };
    let seq = u64_of(field(o, "seq")).ok_or("header seq")?;
    let prev = digest_of(field(o, "prev")).ok_or("header prev")?;
    let kind = text(o, "kind").ok_or("header kind")?.to_string();
    let branch = text(o, "branch").ok_or("header branch")?.to_string();
    let stream = text(o, "stream").ok_or("header stream")?;
    if stream != RECORD_STREAM {
        return Err(format!("entry of stream {stream:?}, not {RECORD_STREAM:?}"));
    }
    let Some(CValue::Obj(p)) = field(o, "principal") else { return Err("header principal".into()) };
    let principal = (
        text(p, "kind").ok_or("principal kind")?.to_string(),
        text(p, "commit").ok_or("principal commit")?.to_string(),
    );
    let session = match field(o, "session") {
        Some(CValue::Obj(si)) => Some(text(si, "session_id").ok_or("session_id")?.to_string()),
        _ => None,
    };
    let command = match field(o, "command") {
        Some(CValue::Obj(c)) => {
            let major = match field(c, "major") {
                Some(CValue::Num(n)) => *n,
                _ => return Err("command major".into()),
            };
            let Some(CValue::Arr(outs)) = field(c, "outputs") else { return Err("command outputs".into()) };
            let mut outputs = Vec::with_capacity(outs.len());
            for out in outs {
                let CValue::Obj(oo) = out else { return Err("command output".into()) };
                let digest = digest_of(field(oo, "digest")).ok_or("output digest")?;
                outputs.push((text(oo, "entity").map(str::to_string), digest));
            }
            Some(Command {
                id: text(c, "id").ok_or("command id")?.to_string(),
                major,
                class: text(c, "class").ok_or("command class")?.to_string(),
                determinism: text(c, "determinism").ok_or("command determinism")?.to_string(),
                events_commit: digest_of(field(c, "events_commit")).ok_or("events_commit")?,
                outputs,
            })
        }
        _ => None,
    };
    Ok(Header { hash, seq, prev, kind, branch, principal, session, command })
}

/// The events of a stored payload, checked against the header's `events_commit`:
/// `(type, body)` per event.
fn payload_events(payload: &CValue, events_commit: &Digest) -> Result<Vec<(String, CValue)>, String> {
    let CValue::Obj(p) = payload else { return Err("payload is not an object".into()) };
    let Some(CValue::Obj(salts)) = field(p, "salts") else { return Err("payload has no salts".into()) };
    let salt = text(salts, "events").ok_or("payload has no events salt")?;
    let salt = Salt::from_hex(salt).map_err(|e| e.to_string())?;
    let value = field(p, "events").ok_or("payload has no events")?;
    if commitment(&salt, value).map_err(|e| e.to_string())? != *events_commit {
        return Err("the payload's events do not reproduce events_commit".into());
    }
    let CValue::Arr(events) = value else { return Err("events is not an array".into()) };
    let mut out = Vec::with_capacity(events.len());
    for e in events {
        let CValue::Obj(env) = e else { return Err("an event is not an object".into()) };
        if env.len() != 2 {
            return Err("an event is not a {body, type} envelope".into());
        }
        let ty = text(env, "type").ok_or("an event has no type")?;
        let body = field(env, "body").ok_or("an event has no body")?;
        out.push((ty.to_string(), body.clone()));
    }
    Ok(out)
}

/// The current identity while reading.
#[derive(Debug, Clone)]
struct Current {
    genesis: Genesis,
}

/// Reads a project's identity entry by entry (module docs). Feed every entry of stream
/// `record` in sequence order from sequence 1 with [`IdentityReader::push`], then call
/// [`IdentityReader::finish`].
pub struct IdentityReader<B: RecordBlobs> {
    blobs: B,
    next_seq: u64,
    prev_hash: Digest,
    last: Option<Prev>,
    genesis_sessions: BTreeSet<String>,
    current: Option<Current>,
    lineage: Vec<Genesis>,
    projects: BTreeSet<[u8; 16]>,
    saved_as: Vec<SavedAs>,
    copies: BTreeSet<[u8; 16]>,
}

impl<B: RecordBlobs> fmt::Debug for IdentityReader<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdentityReader").field("next_seq", &self.next_seq).finish_non_exhaustive()
    }
}

/// Reads the identity of the journal `entries` (module docs).
pub fn read_identity<I, B>(entries: I, blobs: B) -> Result<Identity, IdentityError>
where
    I: IntoIterator<Item = JournalEntry>,
    B: RecordBlobs,
{
    let mut r = IdentityReader::new(blobs);
    for e in entries {
        r.push(&e)?;
    }
    r.finish()
}

impl<B: RecordBlobs> IdentityReader<B> {
    pub fn new(blobs: B) -> IdentityReader<B> {
        IdentityReader {
            blobs,
            next_seq: 1,
            prev_hash: GENESIS_PREV,
            last: None,
            genesis_sessions: BTreeSet::new(),
            current: None,
            lineage: Vec::new(),
            projects: BTreeSet::new(),
            saved_as: Vec::new(),
            copies: BTreeSet::new(),
        }
    }

    /// The identity read so far, if a genesis was read.
    pub fn current(&self) -> Option<&Genesis> {
        self.current.as_ref().map(|c| &c.genesis)
    }

    /// The hash of the last entry read ([`GENESIS_PREV`] before the first).
    pub fn head(&self) -> Digest {
        self.prev_hash
    }

    /// Reads one entry.
    pub fn push(&mut self, entry: &JournalEntry) -> Result<(), IdentityError> {
        let seq_hint = self.next_seq;
        let h = parse_header(&entry.header).map_err(|d| IdentityError {
            refusal: Refusal::Chain,
            seq: Some(seq_hint),
            entry: None,
            detail: d,
        })?;
        let refuse = |refusal: Refusal, detail: &str| IdentityError {
            refusal,
            seq: Some(h.seq),
            entry: Some(h.hash),
            detail: detail.to_string(),
        };
        if h.seq != self.next_seq {
            return Err(refuse(Refusal::Chain, &format!("expected sequence {}", self.next_seq)));
        }
        if h.prev != self.prev_hash {
            return Err(refuse(Refusal::Chain, "prev is not the hash of the entry before"));
        }
        if h.kind == "command" {
            self.command_entry(&h, entry, &refuse)?;
        }
        self.last = Some(Prev {
            hash: h.hash,
            seq: h.seq,
            prev: h.prev,
            session: if h.kind == "session_header" { h.session.clone() } else { None },
            branch: h.branch.clone(),
            principal: h.principal.clone(),
        });
        self.prev_hash = h.hash;
        self.next_seq = h.seq.checked_add(1).ok_or_else(|| refuse(Refusal::Chain, "sequence overflow"))?;
        Ok(())
    }

    fn command_entry(
        &mut self,
        h: &Header,
        entry: &JournalEntry,
        refuse: &dyn Fn(Refusal, &str) -> IdentityError,
    ) -> Result<(), IdentityError> {
        let Some(c) = &h.command else { return Err(refuse(Refusal::Chain, "command entry without command")) };
        let events = match &entry.payload {
            Some(p) => Some(payload_events(p, &c.events_commit).map_err(|d| refuse(Refusal::PayloadMismatch, &d))?),
            None => None,
        };
        let mut records = Vec::new();
        for (entity, digest) in &c.outputs {
            if entity.is_none() || *digest == TOMBSTONE {
                continue;
            }
            let bytes = self
                .blobs
                .object(digest, MAX_RECORD_BYTES)
                .map_err(|d| refuse(Refusal::Storage, &format!("reading {digest}: {d}")))?;
            let Some(bytes) = bytes else { continue };
            if digest_bytes(&bytes) != *digest {
                return Err(refuse(Refusal::RecordCorrupt, &format!("blob {digest} does not hash to its digest")));
            }
            match read_record(&bytes) {
                Ok(Some(r)) => records.push((entity.clone(), r)),
                Ok(None) => {}
                Err(e) => return Err(refuse(Refusal::RecordMalformed, &e.0)),
            }
        }
        let has_effect = events
            .as_ref()
            .is_some_and(|ev| ev.iter().any(|(t, _)| IDENTITY_EFFECTS.iter().any(|(name, _)| name == t)));
        let owner = c.major == 1.0 && (c.id == CREATE_PROJECT || c.id == SAVE_PROJECT_AS);
        if !owner && !has_effect && records.is_empty() {
            return Ok(());
        }
        // 1. Owner.
        if !owner {
            return Err(refuse(Refusal::ForeignCommand, "an identity effect or record under another command"));
        }
        // 2. Branch and promotion.
        if h.branch != MAIN {
            return Err(refuse(Refusal::OffMain, "an identity entry on another branch than main"));
        }
        if entry.promoted_from.is_some() {
            return Err(refuse(Refusal::Promoted, "an identity entry in a promoted copy"));
        }
        // 3. Payload.
        let Some(events) = events else {
            return Err(refuse(Refusal::PayloadMissing, "an identity entry whose payload was removed"));
        };
        // 4. Requests.
        if events.iter().any(|(t, _)| t.starts_with(REQUEST_PREFIX)) {
            return Err(refuse(Refusal::Request, "an identity entry that also carries a Request* event"));
        }
        // 5. Shape.
        let create = c.id == CREATE_PROJECT;
        if c.outputs.len() != 1 || records.len() != 1 || events.len() != 1 {
            return Err(refuse(Refusal::BadShape, "an identity entry holds exactly one output, record and event"));
        }
        let want_class = if create { "W2" } else { "W1" };
        if c.class != want_class || c.determinism != "D0" {
            return Err(refuse(
                Refusal::BadShape,
                &format!("class {} {}, not {want_class} D0", c.class, c.determinism),
            ));
        }
        let (Some((entity, record)), Some((ty, body))) = (records.first(), events.first()) else {
            return Err(refuse(Refusal::BadShape, "no record or event"));
        };
        if entity.as_deref() != Some(record.project().to_string().as_str()) {
            return Err(refuse(Refusal::BadShape, "the output's entity is not the record's project"));
        }
        // 6. Kind.
        let kind_ok = match record {
            Record::Identity(r) if create => r.source.is_none() && ty == PROJECT_IDENTIFIED,
            Record::Identity(r) => r.source.is_some() && ty == PROJECT_COPIED_FROM,
            Record::SavedAs(_) => !create && ty == PROJECT_SAVED_AS,
        };
        if !kind_ok {
            return Err(refuse(Refusal::RecordKind, &format!("{} with this record and {ty}", c.id)));
        }
        // 7. Body.
        if *body != record.to_value() {
            return Err(refuse(Refusal::BodyMismatch, &format!("the {ty} body is not the committed record")));
        }
        match record {
            Record::Identity(r) => self.genesis(h, *r, refuse),
            Record::SavedAs(r) => self.saved_as(h, *r, refuse),
        }
    }

    fn genesis(
        &mut self,
        h: &Header,
        r: IdentityRecord,
        refuse: &dyn Fn(Refusal, &str) -> IdentityError,
    ) -> Result<(), IdentityError> {
        // 8. After a session header, once per session, same principal.
        let s = match &self.last {
            Some(p) if p.session.is_some() && p.branch == MAIN => p.clone(),
            _ => {
                return Err(refuse(Refusal::NotAfterSession, "a genesis must directly follow a session header on main"))
            }
        };
        let sid = s.session.clone().unwrap_or_default();
        if !self.genesis_sessions.insert(sid.clone()) {
            return Err(refuse(Refusal::SecondGenesis, &format!("session {sid:?} already has a genesis")));
        }
        if s.principal != h.principal {
            return Err(refuse(Refusal::PrincipalMismatch, "the genesis principal is not its session header's"));
        }
        match (&r.source, &self.current) {
            // 9. Create.
            (None, None) if s.seq == 1 && h.seq == 2 => {}
            (None, _) => {
                return Err(refuse(
                    Refusal::CreatePosition,
                    "a create genesis is the entry at sequence 2 after the session header at sequence 1",
                ))
            }
            // 10. Copy.
            (Some(_), None) => return Err(refuse(Refusal::SourceMismatch, "a copy genesis without a source identity")),
            (Some(src), Some(cur)) => {
                let cur = &cur.genesis.record;
                if src.project != cur.project {
                    return Err(refuse(Refusal::SourceMismatch, "source.project is not the current identity"));
                }
                if src.head != s.prev {
                    return Err(refuse(Refusal::SourceMismatch, "source.head is not the session header's prev"));
                }
                if r.namespace != cur.namespace {
                    return Err(refuse(Refusal::SourceMismatch, "a copy keeps its source's namespace"));
                }
                if self.projects.contains(r.project.as_bytes()) {
                    return Err(refuse(Refusal::SourceMismatch, "the copy's project ID is not new"));
                }
            }
        }
        let g = Genesis { seq: h.seq, entry: h.hash, session: s.hash, record: r };
        if let Some(old) = self.current.take() {
            self.lineage.push(old.genesis);
        }
        self.projects.insert(*r.project.as_bytes());
        self.current = Some(Current { genesis: g });
        Ok(())
    }

    fn saved_as(
        &mut self,
        h: &Header,
        r: SavedAsRecord,
        refuse: &dyn Fn(Refusal, &str) -> IdentityError,
    ) -> Result<(), IdentityError> {
        // 11. Source side.
        let Some(cur) = &self.current else {
            return Err(refuse(Refusal::WrongProject, "a save-as entry before any genesis"));
        };
        if r.project != cur.genesis.record.project {
            return Err(refuse(Refusal::WrongProject, "the save-as record names another project"));
        }
        if r.head != h.prev {
            return Err(refuse(Refusal::StaleHead, "the save-as was not decided against the entry's own prev"));
        }
        if self.copies.contains(r.copy.as_bytes()) || self.projects.contains(r.copy.as_bytes()) {
            return Err(refuse(Refusal::DuplicateCopy, "the copy's project ID was already named"));
        }
        self.copies.insert(*r.copy.as_bytes());
        self.saved_as.push(SavedAs { seq: h.seq, entry: h.hash, record: r });
        Ok(())
    }

    /// The identity after the last entry pushed.
    pub fn finish(self) -> Result<Identity, IdentityError> {
        let head_seq = self.next_seq.saturating_sub(1);
        let Some(cur) = self.current else {
            return Err(IdentityError {
                refusal: Refusal::NoGenesis,
                seq: if head_seq == 0 { None } else { Some(head_seq) },
                entry: if head_seq == 0 { None } else { Some(self.prev_hash) },
                detail: "the journal holds no genesis".into(),
            });
        };
        Ok(Identity {
            genesis: cur.genesis,
            lineage: self.lineage,
            saved_as: self.saved_as,
            head: self.prev_hash,
            head_seq,
        })
    }
}
