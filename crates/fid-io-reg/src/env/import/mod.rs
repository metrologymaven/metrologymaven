//! The generic import command `import.file@1` and the import skeleton every
//! `import.<format>@1` command is built from (spec node `IO.REG.ENV.IMPORT`,
//! REQ-IO.REG.ENV-001 to -005 and -007).
//!
//! A format reader implements [`Reader`]: the bytes of an untrusted file (a [`HashingSource`]
//! under the `IO.REG.LIMITS` budget) and options in, entities out to a [`StageSink`],
//! warnings and dropped items to a [`ReportBuilder`]. The skeleton runs every import the same
//! way, whichever command started it, in two phases (the EXPORT pattern, #329).
//!
//! **Prepare** ([`prepare_import`]); nothing in the project changes:
//!
//! 1. the target stage is resolved **before the source is opened** (REQ-004): `stage` is a
//!    stage ID or a stage label (untrusted text); [`fid_project::Tree::target_stage`] or
//!    [`fid_project::Tree::stage_by_label`]; none is `stage_not_found`, several labels
//!    `stage_ambiguous`; no `stage` is the active stage;
//! 2. the source is resolved through [`RootSet::resolve_import`] (`IO.REG.ROOTS`): agents and
//!    scripts pass `root://` handles only (`raw_path_forbidden`), inside the root, links
//!    included (`path_outside_root`). A component holding `~` and a digit (a Windows 8.3
//!    short-name alias), and a human raw path's file name that is not a plain name (trailing
//!    dot or space, `:` as in `a.ftp::$DATA`, a device name) fail `path_outside_root` too;
//! 3. every `IO.REG.LIMITS` bound is set up before anything is allocated for the content:
//!    the file's size is checked against the byte limit first, the detection prefix is read
//!    bounded at [`DETECT_PREFIX_LEN`], and the reader reads through a [`HashingSource`] over a
//!    [`BoundedSource`], so every byte, element, nesting level and tracked allocation is
//!    charged as it happens, and each staged entity is charged ([`STAGED_ENTITY_COST`] plus
//!    its name and payload) before it is kept;
//! 4. the format is detected or checked, consistently: with no `format`, the
//!    `IO.REG.DETECT` rule (content signature, then the extension cross-check); with a
//!    `format` (or a per-format command), the content signatures and the extension must not
//!    name another format (`format_mismatch`): content that matches no signature is accepted
//!    for the named format, a tie must include it, an extension some reader declares must be
//!    one of the named format's;
//! 5. the reader reads; a cancel (checked before reading, on every staged entity, after the
//!    reader returns) or any error ends the import with nothing to commit;
//! 6. payloads go to the content-addressed store through a [`BlobSink`] (`CORE.STO.CAS`;
//!    a dry run hashes them and stores nothing). A blob written by a failed or cancelled
//!    import is referenced by nothing (REQ-007);
//! 7. the [`SourceProvenance`] (`IO.REG.PROV`), the [`ImportReport`] and the commit's events
//!    are built: one `ElementCreated` and one `ElementSlotSet` (state `current`, the payload's
//!    digest) **in the target stage only** per entity (REQ-003; other stages keep empty slots,
//!    so their per-stage digests do not change), then the one [`ImportEvent`] record holding
//!    the provenance and the report digest (REQ-002). IDs are UUIDv5s of the project
//!    namespace over the source digest, the stage and the entity's index, so replay
//!    reproduces them. The events are applied to a copy of the tree, so a commit cannot fail
//!    on the tree's rules.
//!
//! **Commit** ([`PreparedImport::commit`]): the events go to the tree as one all-or-nothing
//! commit; for a dry run nothing happens (REQ-005). The bus adapter
//! (`CORE.BUS.PIPE.STAGE`, REQ-011, not built yet) commits the same events as one journal
//! entry.
//!
//! File content is untrusted: names are kept verbatim as untrusted text (`ElementName`, no
//! control or bidi characters; an entity whose name breaks the rule is dropped with reason
//! `bad_name`), nothing in the content is interpreted as a command, path or option, and the
//! only events an import yields are the three kinds above. Every failure maps to a
//! `code/reason` pair of the bus error catalogue v3 ([`ImportError::bus_code`]).

pub mod command;
pub mod conformance;

use crate::budget::Budget;
use crate::detect::{read_prefix, DetectError, Registry, DETECT_PREFIX_LEN};
use crate::error::{IoError, Position};
use crate::limits::{LimitKind, LimitRequest, Principal};
use crate::prov::{
    Detection, EffectiveOptions, HashingSource, LengthUnit, PrincipalCommit, ProvError, ReaderInfo, SourceProvenance,
};
use crate::report::{Code, FormatId, ImportReport, Location, ReportBuilder, ReportError, ReportPos, TypeName};
use crate::roots::{plain_name, Direction, RootHandle, RootSet};
use crate::source::BoundedSource;
use crate::untrusted::UntrustedText;
use fid_canon::CValue;
use fid_project::{ElementName, ElementTypeRegistry, Tree, TreeError, TreeEvent, Uuid};
use fid_replay::ReplayOutcome;
use fid_types::{CommandId, Digest};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};

/// Command ID of the generic import command.
pub const IMPORT_FILE: &str = "import.file@1";
/// Import command names no format command may take.
pub const RESERVED_IMPORT_NAMES: &[&str] = &["import.file", "import.list_formats"];
/// Largest payload of one entity, in bytes: one content-addressed blob (`CORE.STO.CAS`
/// `CHUNK_SIZE`), the most a value read (`CORE.PRJ.STAGE.SWITCH`) resolves.
pub const MAX_PAYLOAD_BYTES: u64 = 8 * 1024 * 1024;
/// Tracked allocation charged per staged entity besides its name and payload; at least the
/// size of a [`StagedEntity`] plus a Vec slot (tested).
pub const STAGED_ENTITY_COST: u64 = 256;
/// Longest `source` text, in bytes.
pub const MAX_SOURCE_BYTES: usize = 4096;
/// Longest stage label given as `stage`, in bytes.
pub const MAX_STAGE_TEXT_BYTES: usize = 1024;
/// Largest params document or record [`ImportParams::from_json`] / [`ImportEvent::from_json`]
/// accept.
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
/// Deepest nesting of a params document or record.
pub const MAX_JSON_DEPTH: usize = 64;
/// Kind tag of the entity ID derivation.
pub const ENTITY_ID_KIND: &str = "io.reg.import.entity";

// ---------------------------------------------------------------------------------------
// Errors

/// Why an import failed. Nothing was committed and nothing is to be journaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// A harness error: root resolution, limits, detection, the source, the blob store.
    Io(IoError),
    /// The reader itself failed on the content (an error it returned).
    Reader(IoError),
    /// The import was cancelled.
    Cancelled,
    /// The named stage is not a live stage of the project.
    StageNotFound,
    /// More than one live stage has the given label.
    StageAmbiguous,
    /// The parameters do not fit the command's params schema.
    BadParams(&'static str),
    /// The reader refused the options.
    BadOptions(&'static str),
    /// A record could not be read back.
    BadEvent(&'static str),
    /// The provenance record could not be built (an oversized untrusted text and the like).
    Provenance(String),
    /// The tree refused the commit (an unregistered element type, an ID already taken).
    Tree(TreeError),
    /// A per-format command ID is reserved or already registered.
    CommandIdTaken,
    /// A broken invariant (a bug, never a caller mistake).
    Internal(&'static str),
}

impl From<IoError> for ImportError {
    fn from(e: IoError) -> ImportError {
        ImportError::Io(e)
    }
}

impl From<ProvError> for ImportError {
    fn from(e: ProvError) -> ImportError {
        ImportError::Provenance(e.code().to_string())
    }
}

impl ImportError {
    /// The leaf's own stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ImportError::Io(e) => e.code(),
            ImportError::Reader(e) => match e {
                IoError::Malformed { .. } | IoError::TruncatedFile { .. } | IoError::LimitExceeded { .. } => e.code(),
                _ => "reader_failed",
            },
            ImportError::Cancelled => "cancelled",
            ImportError::StageNotFound => "stage_not_found",
            ImportError::StageAmbiguous => "stage_ambiguous",
            ImportError::BadParams(_) => "bad_params",
            ImportError::BadOptions(_) => "bad_options",
            ImportError::BadEvent(_) => "bad_event",
            ImportError::Provenance(_) => "bad_provenance",
            ImportError::Tree(e) => e.code(),
            ImportError::CommandIdTaken => "command_id_taken",
            ImportError::Internal(_) => "internal",
        }
    }

    /// The bus error `(code, reason)` of catalogue v3 (CORE.BUS.PIPE.ERR). Content the file
    /// gets wrong (malformed, truncated, an oversized text) is `validation/bad_artifact`; a
    /// limit is `validation/effect_limit` (`cancelled/timeout` for wall time); a stage that is
    /// not there or not unique is `validation/bad_stage`; root, raw-path and extension
    /// refusals are `policy`; a reader's own fault is `internal/handler_failed`.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            ImportError::Io(e) | ImportError::Reader(e) => io_bus_code(e, matches!(self, ImportError::Reader(_))),
            ImportError::Cancelled => ("cancelled", "requested"),
            ImportError::StageNotFound | ImportError::StageAmbiguous => ("validation", "bad_stage"),
            ImportError::BadParams(_) | ImportError::BadOptions(_) | ImportError::BadEvent(_) => {
                ("validation", "schema")
            }
            ImportError::Provenance(_) => ("validation", "bad_artifact"),
            ImportError::Tree(e) => tree_bus_code(e),
            ImportError::CommandIdTaken | ImportError::Internal(_) => ("internal", "invariant_violated"),
        }
    }
}

fn io_bus_code(e: &IoError, from_reader: bool) -> (&'static str, &'static str) {
    match e {
        IoError::RawPathForbidden => ("policy", "raw_path_refused"),
        IoError::PathOutsideRoot => ("policy", "outside_root"),
        IoError::RootNotFound => ("policy", "bad_root"),
        IoError::FormatMismatch => ("policy", "extension_refused"),
        IoError::LimitRaiseForbidden { .. } => ("policy", "human_only"),
        IoError::FormatUnknown | IoError::FormatAmbiguous => ("validation", "schema"),
        IoError::LimitExceeded { limit: LimitKind::Wall, .. } => ("cancelled", "timeout"),
        IoError::LimitExceeded { .. } => ("validation", "effect_limit"),
        IoError::Malformed { .. } | IoError::TruncatedFile { .. } => ("validation", "bad_artifact"),
        IoError::UnitRequired | IoError::UnitConflict => ("precondition", "false"),
        IoError::Io { kind: io::ErrorKind::NotFound, .. } => ("precondition", "false"),
        IoError::Io { .. } if from_reader => ("internal", "handler_failed"),
        IoError::Io { .. } => ("internal", "storage"),
        IoError::ReaderCrashed => ("internal", "handler_failed"),
        IoError::TargetExists | IoError::FormatDuplicate => ("internal", "invariant_violated"),
    }
}

fn tree_bus_code(e: &TreeError) -> (&'static str, &'static str) {
    match e {
        TreeError::UnregisteredType(_) => ("validation", "unregistered_type"),
        TreeError::Name => ("validation", "name"),
        TreeError::BadStage(_) | TreeError::StageNotFound(_) => ("validation", "bad_stage"),
        TreeError::IdCollision(_) => ("conflict", "id_collision"),
        _ => ("internal", "invariant_violated"),
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(e) => write!(f, "{e}"),
            ImportError::Reader(e) => write!(f, "{}: {e}", self.code()),
            ImportError::BadParams(r) | ImportError::BadOptions(r) | ImportError::BadEvent(r) => {
                write!(f, "{}: {r}", self.code())
            }
            ImportError::Provenance(c) => write!(f, "bad_provenance: {c}"),
            ImportError::Tree(e) => write!(f, "{e}"),
            ImportError::Internal(r) => write!(f, "internal: {r}"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for ImportError {}

// ---------------------------------------------------------------------------------------
// Blobs and the stage sink

/// Where payloads are stored: the content-addressed store. `put` returns the content
/// address (SHA-256 of the bytes).
pub trait BlobSink {
    fn put(&mut self, bytes: &[u8]) -> Result<Digest, IoError>;
}

/// An in-memory [`BlobSink`] (tests, conformance kit): digest to bytes.
#[derive(Debug, Clone, Default)]
pub struct MemoryBlobs {
    /// The stored blobs.
    pub blobs: BTreeMap<Digest, Vec<u8>>,
}

impl BlobSink for MemoryBlobs {
    fn put(&mut self, bytes: &[u8]) -> Result<Digest, IoError> {
        let d = sha256_of(bytes);
        self.blobs.entry(d).or_insert_with(|| bytes.to_vec());
        Ok(d)
    }
}

/// [`BlobSink`] over the project blob store (feature `cas`). A payload above
/// [`MAX_PAYLOAD_BYTES`] never reaches it (the sink refuses it first), so every blob is a
/// single blob and its address is the SHA-256 of its bytes.
#[cfg(feature = "cas")]
#[derive(Debug)]
pub struct CasBlobs<'a>(pub &'a fid_store::cas::BlobStore);

#[cfg(feature = "cas")]
impl BlobSink for CasBlobs<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<Digest, IoError> {
        match self.0.put_bytes(bytes, fid_store::cas::PutOpts::default()) {
            Ok(fid_store::cas::Address::Blob(b)) => Ok(b.digest),
            Ok(fid_store::cas::Address::Object(_)) => {
                Err(IoError::LimitExceeded { limit: LimitKind::Alloc, value: MAX_PAYLOAD_BYTES, at: Position::byte(0) })
            }
            Err(e) => Err(IoError::Io { kind: e.io_kind().unwrap_or(io::ErrorKind::Other), at: Position::byte(0) }),
        }
    }
}

fn sha256_of(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// One entity a reader staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedEntity {
    /// Registered element type ID.
    pub type_id: String,
    /// The untrusted name, checked.
    pub name: ElementName,
    /// Content address of the payload.
    pub payload: Digest,
}

/// A fault injected for the conformance kit and tests: the real command path runs, only
/// the sink misbehaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The sink takes `n` entities, then fails with `io{Other}` (a failing store).
    FailAfter(u64),
    /// A cancel arrives right after the `n`-th entity.
    CancelAfter(u64),
}

/// Where a reader puts what it read: entities with their payloads, the length unit the file
/// declares, untrusted metadata. Every entity is charged to the budget before it is kept.
pub struct StageSink<'a, 'c> {
    blobs: Option<&'a mut dyn BlobSink>,
    entities: Vec<StagedEntity>,
    declared_unit: Option<LengthUnit>,
    declared: BTreeMap<String, UntrustedText>,
    cancel: Option<&'c AtomicBool>,
    fault: Option<Fault>,
    cancelled: bool,
    alloc_limit: u64,
    alloc_used: u64,
    elements_limit: u64,
}

impl fmt::Debug for StageSink<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StageSink").field("entities", &self.entities.len()).finish_non_exhaustive()
    }
}

impl StageSink<'_, '_> {
    /// Stages one entity of a registered element type with a name and its payload bytes.
    /// The payload goes to the store (hashed only on a dry run). A name that breaks the
    /// element name rules is not staged: the entity is dropped with reason `bad_name`.
    pub fn add_entity(
        &mut self,
        type_id: &str,
        name: &UntrustedText,
        payload: &[u8],
        at: Position,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        if self.cancelled || self.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            self.cancelled = true;
            return Err(IoError::Io { kind: io::ErrorKind::Interrupted, at });
        }
        let n = self.entities.len() as u64;
        if let Some(Fault::FailAfter(k)) = self.fault {
            if n >= k {
                return Err(IoError::Io { kind: io::ErrorKind::Other, at });
            }
        }
        if n.saturating_add(1) > self.elements_limit {
            return Err(IoError::LimitExceeded { limit: LimitKind::Elements, value: self.elements_limit, at });
        }
        let size = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        if size > MAX_PAYLOAD_BYTES {
            return Err(IoError::LimitExceeded { limit: LimitKind::Alloc, value: MAX_PAYLOAD_BYTES, at });
        }
        let cost = STAGED_ENTITY_COST
            .saturating_add(u64::try_from(name.raw().len()).unwrap_or(u64::MAX))
            .saturating_add(u64::try_from(type_id.len()).unwrap_or(u64::MAX));
        let after = self.alloc_used.saturating_add(cost);
        if after > self.alloc_limit {
            return Err(IoError::LimitExceeded { limit: LimitKind::Alloc, value: self.alloc_limit, at });
        }
        let kind =
            TypeName::new(type_id).map_err(|_| IoError::Malformed { at, reason: "entity type is not a type name" })?;
        let Ok(element_name) = ElementName::new(name.raw()) else {
            let pos = ReportPos::new(at).map_err(|_| IoError::Malformed { at, reason: "bad position" })?;
            let reason = Code::new("bad_name").map_err(|_| IoError::Malformed { at, reason: "bad code" })?;
            report.drop_item(&kind, pos, &reason, name);
            return Ok(());
        };
        let payload = match self.blobs.as_deref_mut() {
            Some(b) => b.put(payload)?,
            None => sha256_of(payload),
        };
        self.alloc_used = after;
        self.entities.try_reserve(1).map_err(|_| IoError::LimitExceeded {
            limit: LimitKind::Alloc,
            value: self.alloc_limit,
            at,
        })?;
        self.entities.push(StagedEntity { type_id: type_id.to_string(), name: element_name, payload });
        report.count(&kind, 1);
        if let Some(Fault::CancelAfter(k)) = self.fault {
            if self.entities.len() as u64 >= k {
                self.cancelled = true;
            }
        }
        Ok(())
    }

    /// The length unit the file declares.
    pub fn declare_unit(&mut self, unit: LengthUnit) {
        self.declared_unit = Some(unit);
    }

    /// Untrusted metadata the file declares, kept in the provenance record.
    pub fn declare(&mut self, key: &str, value: UntrustedText) {
        self.declared.insert(key.to_string(), value);
    }

    /// Entities staged so far.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

// ---------------------------------------------------------------------------------------
// Readers

/// The source a reader reads: the untrusted file through the hashing, bounded source.
pub type ImportSource<'a> = HashingSource<&'a mut dyn Read>;

/// One format reader.
pub trait Reader {
    /// The format it reads; must be registered for import.
    fn format(&self) -> &FormatId;

    /// Reader ID and version, journaled with every import.
    fn info(&self) -> &ReaderInfo;

    /// Checks the caller's options and returns the effective options (defaults filled in).
    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        match given {
            CValue::Obj(_) => Ok(given.clone()),
            _ => Err("options must be an object"),
        }
    }

    /// Reads `src` with the effective options into `sink`, reporting to `report`. Must be
    /// deterministic for the same bytes and options, must treat every byte as untrusted data,
    /// and must charge what it allocates to `src.budget_mut()`.
    fn read(
        &self,
        src: &mut ImportSource<'_>,
        opts: &CValue,
        sink: &mut StageSink<'_, '_>,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError>;
}

// ---------------------------------------------------------------------------------------
// Parameters

/// The parameters of `import.file@1` (with optional `format`) or `import.<format>@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportParams {
    /// A `root://` handle, or for a human principal also a raw path.
    pub source: String,
    /// The format (`import.file@1` only; detected when absent).
    pub format: Option<FormatId>,
    /// The caller's length unit.
    pub unit: Option<LengthUnit>,
    /// The target stage: a stage ID or label; the active stage when absent.
    pub stage: Option<String>,
    /// Reader options; `{}` when not given.
    pub options: CValue,
    /// Read and report only; commit nothing, store no blob.
    pub dry_run: bool,
}

fn unit_of(symbol: &str) -> Option<LengthUnit> {
    LengthUnit::ALL.into_iter().find(|u| u.symbol() == symbol)
}

impl ImportParams {
    /// Parameters with the given source and every optional field at its default.
    pub fn new(source: &str) -> ImportParams {
        ImportParams {
            source: source.to_string(),
            format: None,
            unit: None,
            stage: None,
            options: CValue::Obj(BTreeMap::new()),
            dry_run: false,
        }
    }

    /// Reads a params object strictly (unknown fields, wrong types: `bad_params`).
    /// `with_format` is true for `import.file@1`; per-format commands refuse `format`.
    pub fn from_value(v: &CValue, with_format: bool) -> Result<ImportParams, ImportError> {
        let CValue::Obj(o) = v else { return Err(ImportError::BadParams("params must be an object")) };
        const KEYS: [&str; 6] = ["dry_run", "format", "options", "source", "stage", "unit"];
        if o.keys().any(|k| !KEYS.contains(&k.as_str()) || (k == "format" && !with_format)) {
            return Err(ImportError::BadParams("unknown field"));
        }
        let source = match o.get("source") {
            Some(CValue::Str(s)) if !s.is_empty() && s.len() <= MAX_SOURCE_BYTES => s.clone(),
            _ => return Err(ImportError::BadParams("source: a non-empty string")),
        };
        let format = match o.get("format") {
            None => None,
            Some(CValue::Str(s)) => {
                Some(FormatId::new(s).map_err(|_| ImportError::BadParams("format: not a format ID"))?)
            }
            Some(_) => return Err(ImportError::BadParams("format must be a string")),
        };
        let unit = match o.get("unit") {
            None => None,
            Some(CValue::Str(s)) => Some(unit_of(s).ok_or(ImportError::BadParams("unit: not a length unit"))?),
            Some(_) => return Err(ImportError::BadParams("unit must be a string")),
        };
        let stage = match o.get("stage") {
            None => None,
            Some(CValue::Str(s)) if !s.is_empty() && s.len() <= MAX_STAGE_TEXT_BYTES => Some(s.clone()),
            Some(_) => return Err(ImportError::BadParams("stage: a non-empty string")),
        };
        let options = match o.get("options") {
            None => CValue::Obj(BTreeMap::new()),
            Some(v @ CValue::Obj(_)) => v.clone(),
            Some(_) => return Err(ImportError::BadParams("options must be an object")),
        };
        let dry_run = match o.get("dry_run") {
            None => false,
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(ImportError::BadParams("dry_run must be a boolean")),
        };
        Ok(ImportParams { source, format, unit, stage, options, dry_run })
    }

    /// Parses params JSON (bounded, duplicate keys refused), then [`ImportParams::from_value`].
    pub fn from_json(bytes: &[u8], with_format: bool) -> Result<ImportParams, ImportError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| ImportError::BadParams("not acceptable JSON"))?;
        ImportParams::from_value(&v, with_format)
    }

    /// Folds the bus's own `dry_run` into the import's (the adapter calls it first).
    pub fn apply_bus_dry_run(&mut self, bus_dry_run: bool) {
        self.dry_run |= bus_dry_run;
    }
}

// ---------------------------------------------------------------------------------------
// The record

/// The journal record of one committed import: the last event of its commit, of type
/// `command` (`import.file@1` or `import.<format>@1`).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportEvent {
    /// The command that ran (also the event type).
    pub command: CommandId,
    /// The principal kind that ran it.
    pub principal: Principal,
    /// The source handle (or a human's raw path).
    pub source: Location,
    /// The format read.
    pub format: FormatId,
    /// The `format` parameter, if one was given (replay passes it again).
    pub format_named: bool,
    /// The caller's unit, if given.
    pub unit: Option<LengthUnit>,
    /// The stage the entities went to.
    pub stage: Uuid,
    /// The effective options.
    pub options: CValue,
    /// The provenance record (`IO.REG.PROV` canonical value).
    pub provenance: CValue,
    /// SHA-256 of the provenance record's canonical bytes.
    pub provenance_sha256: Digest,
    /// SHA-256 of the canonical [`ImportReport`] without its `dry_run` flag.
    pub report_sha256: Digest,
    /// The staged entities' payload addresses, in entity order (the blobs the commit makes
    /// reachable).
    pub blobs: Vec<Digest>,
}

const EVENT_KEYS: [&str; 12] = [
    "blobs",
    "command",
    "format",
    "format_named",
    "options",
    "principal",
    "provenance",
    "provenance_sha256",
    "report_sha256",
    "source",
    "stage",
    "unit",
];

fn bad_event(r: &'static str) -> ImportError {
    ImportError::BadEvent(r)
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, ImportError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(bad_event("a field is missing or not a string")),
    }
}

fn principal_from_wire(s: &str) -> Option<Principal> {
    [Principal::Human, Principal::Agent, Principal::Script].into_iter().find(|p| p.as_str() == s)
}

impl ImportEvent {
    /// The record as JSON.
    pub fn to_value(&self) -> CValue {
        let s = |v: &str| CValue::Str(v.to_string());
        let mut o = BTreeMap::new();
        o.insert("blobs".to_string(), CValue::Arr(self.blobs.iter().map(|d| s(d.as_str())).collect()));
        o.insert("command".to_string(), s(self.command.as_str()));
        o.insert("format".to_string(), s(self.format.as_str()));
        o.insert("format_named".to_string(), CValue::Bool(self.format_named));
        o.insert("options".to_string(), self.options.clone());
        o.insert("principal".to_string(), s(self.principal.as_str()));
        o.insert("provenance".to_string(), self.provenance.clone());
        o.insert("provenance_sha256".to_string(), s(self.provenance_sha256.as_str()));
        o.insert("report_sha256".to_string(), s(self.report_sha256.as_str()));
        o.insert("source".to_string(), s(&self.source.wire()));
        o.insert("stage".to_string(), s(&self.stage.to_string()));
        o.insert("unit".to_string(), self.unit.map_or(CValue::Null, |u| s(u.symbol())));
        CValue::Obj(o)
    }

    /// RFC 8785 canonical bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, ImportError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|_| ImportError::Internal("record is not canonical"))
    }

    /// The tree event of this record (type = the command ID).
    pub fn tree_event(&self) -> TreeEvent {
        TreeEvent::new(self.command.as_str(), self.to_value())
    }

    /// Reads a record back strictly: canonical bytes, exactly the keys, valid values, and a
    /// provenance whose digest matches `provenance_sha256`.
    pub fn from_json(bytes: &[u8]) -> Result<ImportEvent, ImportError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| bad_event("not acceptable JSON"))?;
        match fid_canon::to_canonical_bytes(&v) {
            Ok(c) if c == bytes => {}
            _ => return Err(bad_event("not in canonical form")),
        }
        ImportEvent::from_value(&v)
    }

    /// Reads a record value strictly (see [`ImportEvent::from_json`]).
    pub fn from_value(v: &CValue) -> Result<ImportEvent, ImportError> {
        let CValue::Obj(o) = v else { return Err(bad_event("must be an object")) };
        if o.len() != EVENT_KEYS.len() || o.keys().any(|k| !EVENT_KEYS.contains(&k.as_str())) {
            return Err(bad_event("unknown or missing field"));
        }
        let command = CommandId::parse(str_field(o, "command")?).map_err(|_| bad_event("invalid command ID"))?;
        let principal = principal_from_wire(str_field(o, "principal")?).ok_or(bad_event("invalid principal"))?;
        let source = Location::from_wire(str_field(o, "source")?).map_err(|_| bad_event("invalid source"))?;
        let format = FormatId::new(str_field(o, "format")?).map_err(|_| bad_event("invalid format"))?;
        let format_named = match o.get("format_named") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(bad_event("format_named must be a boolean")),
        };
        let unit = match o.get("unit") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(unit_of(s).ok_or(bad_event("invalid unit"))?),
            _ => return Err(bad_event("unit must be a string or null")),
        };
        let stage_text = str_field(o, "stage")?;
        let stage = Uuid::parse(stage_text).ok().filter(|u| u.to_string() == stage_text).ok_or(bad_event("stage"))?;
        let options = match o.get("options") {
            Some(v @ CValue::Obj(_)) => v.clone(),
            _ => return Err(bad_event("options must be an object")),
        };
        let provenance = match o.get("provenance") {
            Some(v @ CValue::Obj(_)) => v.clone(),
            _ => return Err(bad_event("provenance must be an object")),
        };
        let provenance_sha256 =
            Digest::parse(str_field(o, "provenance_sha256")?).map_err(|_| bad_event("invalid provenance_sha256"))?;
        let canon = fid_canon::to_canonical_bytes(&provenance).map_err(|_| bad_event("provenance not canonical"))?;
        if sha256_of(&canon) != provenance_sha256 {
            return Err(bad_event("provenance does not match its digest"));
        }
        let report_sha256 =
            Digest::parse(str_field(o, "report_sha256")?).map_err(|_| bad_event("invalid report_sha256"))?;
        let Some(CValue::Arr(items)) = o.get("blobs") else { return Err(bad_event("blobs must be an array")) };
        let blobs = items
            .iter()
            .map(|i| match i {
                CValue::Str(s) => Digest::parse(s).map_err(|_| bad_event("invalid blob digest")),
                _ => Err(bad_event("blob digest must be a string")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ImportEvent {
            command,
            principal,
            source,
            format,
            format_named,
            unit,
            stage,
            options,
            provenance,
            provenance_sha256,
            report_sha256,
            blobs,
        })
    }
}

/// Registers the record event types of `commands` (`import.file@1`, `import.<format>@1`)
/// with the element tree, translated into no tree change: the tree refuses any event type a
/// domain did not register, so every project that journals imports registers them at
/// start-up.
pub fn register_import_events(reg: &mut ElementTypeRegistry, commands: &[&CommandId]) -> Result<(), ImportError> {
    for c in commands {
        reg.register_event(c.as_str(), |_| Ok(Vec::new())).map_err(ImportError::Tree)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// The skeleton

/// What an import runs against.
pub struct ImportEnv<'a> {
    /// The format registry (import descriptors decide detection).
    pub registry: &'a Registry,
    /// The configured import roots.
    pub roots: &'a RootSet,
    /// Who is calling.
    pub principal: Principal,
    /// The principal's journal header commitment (`IO.REG.PROV`).
    pub principal_commit: PrincipalCommit,
    /// The caller's limit overrides (`IO.REG.LIMITS`; only a human may raise one).
    pub limits: LimitRequest,
    /// The project state (read only): stages and the IDs already taken.
    pub tree: &'a Tree,
    /// The project namespace (entity ID derivation).
    pub namespace: Uuid,
    /// Set to cancel; checked before reading, on every staged entity and after the reader.
    pub cancel: Option<&'a AtomicBool>,
    /// An injected sink fault (conformance kit and tests); `None` in production.
    pub fault: Option<Fault>,
}

impl fmt::Debug for ImportEnv<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportEnv").field("principal", &self.principal).finish_non_exhaustive()
    }
}

/// The outcome of a successful import.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOutput {
    /// The command result.
    pub report: ImportReport,
    /// The record a real run commits (the would-be record for a dry run).
    pub event: ImportEvent,
}

/// A prepared import: entities staged, events built and checked against a copy of the tree,
/// nothing committed. [`PreparedImport::commit`] applies them; dropping it commits nothing.
#[derive(Debug, Clone)]
pub struct PreparedImport {
    events: Vec<TreeEvent>,
    out: ImportOutput,
}

impl PreparedImport {
    /// The report the import will return.
    pub fn report(&self) -> &ImportReport {
        &self.out.report
    }

    /// The record (the would-be record for a dry run).
    pub fn event(&self) -> &ImportEvent {
        &self.out.event
    }

    /// Every event of the commit, the record last; empty for a dry run.
    pub fn events(&self) -> &[TreeEvent] {
        &self.events
    }

    /// Applies the commit to `tree`, all or nothing (nothing for a dry run).
    pub fn commit(self, tree: &mut Tree) -> Result<ImportOutput, ImportError> {
        if !self.events.is_empty() {
            tree.apply_commit(&self.events).map_err(ImportError::Tree)?;
        }
        Ok(self.out)
    }
}

/// True for a name holding `~` followed by a digit (a Windows 8.3 short-name alias), refused
/// on every platform as EXPORT does.
fn short_name_alias(name: &str) -> bool {
    name.as_bytes().windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit())
}

fn requested_components(source: &str) -> Vec<String> {
    match RootHandle::parse(source) {
        Ok(h) => h.rel.components().to_vec(),
        Err(_) => {
            std::path::Path::new(source).file_name().map(|n| vec![n.to_string_lossy().into_owned()]).unwrap_or_default()
        }
    }
}

/// Step 2's lexical checks, before anything is resolved: no component with a short-name
/// alias, and a human raw path's file name must be a plain name.
fn confine(source: &str) -> Result<(), ImportError> {
    if requested_components(source).iter().any(|c| short_name_alias(c)) {
        return Err(IoError::PathOutsideRoot.into());
    }
    if RootHandle::parse(source).is_err() && !source.starts_with(crate::roots::SCHEME) {
        let name = std::path::Path::new(source).file_name().and_then(|n| n.to_str());
        // `Path::file_name` drops a trailing `.` component; check the text's last part too.
        let last = source.rsplit(['/', '\\']).next().unwrap_or(source);
        if !name.is_some_and(plain_name) || !plain_name(last) {
            return Err(IoError::PathOutsideRoot.into());
        }
    }
    Ok(())
}

fn location_of(source: &str) -> Result<Location, ImportError> {
    let loc = match RootHandle::parse(source) {
        Ok(h) => Location::handle(h),
        Err(_) => Location::human_path(source),
    };
    loc.map_err(|_: ReportError| ImportError::BadParams("source: not a representable location"))
}

/// The target stage (step 1).
fn target_stage(tree: &Tree, stage: Option<&str>) -> Result<Uuid, ImportError> {
    let Some(text) = stage else { return Ok(tree.active_stage()) };
    if let Some(id) = Uuid::parse(text).ok().filter(|u| u.to_string() == text) {
        return tree.target_stage(Some(&id)).map_err(|_| ImportError::StageNotFound);
    }
    tree.stage_by_label(text).map_err(|e| match e.code() {
        "ambiguous_stage" => ImportError::StageAmbiguous,
        _ => ImportError::StageNotFound,
    })
}

fn detect_error(e: DetectError) -> ImportError {
    match e {
        DetectError::Unknown | DetectError::NotRegistered { .. } => IoError::FormatUnknown.into(),
        DetectError::Ambiguous { .. } => IoError::FormatAmbiguous.into(),
        DetectError::Mismatch { .. } => IoError::FormatMismatch.into(),
        _ => ImportError::Io(IoError::FormatUnknown),
    }
}

/// Step 4: the format, detected or named, consistently with content and extension.
fn choose_format(
    reg: &Registry,
    prefix: &[u8],
    len: u64,
    name: Option<&str>,
    named: Option<&FormatId>,
) -> Result<Detection, ImportError> {
    let Some(named) = named else {
        return Detection::detect(reg, prefix, len, name, None).map_err(detect_error);
    };
    if reg.get(named, Direction::Import).is_none() {
        return Err(IoError::FormatUnknown.into());
    }
    match reg.detect(prefix, len, None, None) {
        Ok(f) if f != *named => return Err(IoError::FormatMismatch.into()),
        Err(DetectError::Ambiguous { candidates, .. }) if !candidates.contains(named) => {
            return Err(IoError::FormatMismatch.into())
        }
        Ok(_) | Err(DetectError::Unknown) | Err(DetectError::Ambiguous { .. }) => {}
        Err(e) => return Err(detect_error(e)),
    }
    if let Some(ext) = name.and_then(crate::detect::extension_of) {
        let by_ext: Vec<&FormatId> = reg
            .list(Direction::Import, crate::detect::OriginFilter::Any)
            .into_iter()
            .filter(|d| d.extensions().contains(&ext))
            .map(|d| d.id())
            .collect();
        if !by_ext.is_empty() && !by_ext.contains(&named) {
            return Err(IoError::FormatMismatch.into());
        }
    }
    Detection::detect(reg, prefix, len, name, Some(named)).map_err(detect_error)
}

fn report_digest(report: &ImportReport) -> Result<Digest, ImportError> {
    let mut v = report.to_cvalue();
    if let CValue::Obj(o) = &mut v {
        o.remove("dry_run");
    }
    let bytes = fid_canon::to_canonical_bytes(&v).map_err(|_| ImportError::Internal("report is not canonical"))?;
    Ok(sha256_of(&bytes))
}

fn normalise(v: &CValue) -> Result<CValue, ImportError> {
    let bytes = fid_canon::to_canonical_bytes(v).map_err(|_| ImportError::BadOptions("options are not canonical"))?;
    fid_canon::parse_with_limits(&bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
        .map_err(|_| ImportError::BadOptions("options are too large or too deep"))
}

fn is_set(flag: Option<&AtomicBool>) -> bool {
    flag.is_some_and(|f| f.load(Ordering::SeqCst))
}

/// The ID of the `index`-th entity of an import of `source` into `stage`.
pub fn entity_id(namespace: Uuid, source: &Digest, stage: &Uuid, index: u64) -> Result<Uuid, ImportError> {
    let mut o = BTreeMap::new();
    o.insert("index".to_string(), CValue::U64(index));
    o.insert("kind".to_string(), CValue::Str(ENTITY_ID_KIND.to_string()));
    o.insert("source".to_string(), CValue::Str(source.as_str().to_string()));
    o.insert("stage".to_string(), CValue::Str(stage.to_string()));
    let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(o)).map_err(|_| ImportError::Internal("id input"))?;
    Ok(fid_project::uuid_v5(&namespace, &bytes))
}

fn entity_events(
    env: &ImportEnv<'_>,
    source: &Digest,
    stage: &Uuid,
    staged: &[StagedEntity],
) -> Result<Vec<TreeEvent>, ImportError> {
    let s = |v: String| CValue::Str(v);
    let root = env.tree.root().to_string();
    let mut out = Vec::new();
    out.try_reserve(staged.len().saturating_mul(2).saturating_add(1))
        .map_err(|_| ImportError::Internal("cannot reserve the commit"))?;
    for (i, e) in staged.iter().enumerate() {
        let id = entity_id(env.namespace, source, stage, i as u64)?.to_string();
        let mut created = BTreeMap::new();
        created.insert("id".to_string(), s(id.clone()));
        created.insert("meta".to_string(), CValue::Obj(BTreeMap::new()));
        created.insert("name".to_string(), s(e.name.as_untrusted_str().to_string()));
        created.insert("parent".to_string(), s(root.clone()));
        created.insert("type".to_string(), s(e.type_id.clone()));
        created.insert("visible".to_string(), CValue::Bool(true));
        out.push(TreeEvent::new(fid_project::ELEMENT_CREATED, CValue::Obj(created)));
        let mut set = BTreeMap::new();
        set.insert("digest".to_string(), s(e.payload.as_str().to_string()));
        set.insert("element".to_string(), s(id));
        set.insert("stage".to_string(), s(stage.to_string()));
        set.insert("state".to_string(), s("current".to_string()));
        out.push(TreeEvent::new(fid_project::ELEMENT_SLOT_SET, CValue::Obj(set)));
    }
    Ok(out)
}

/// How a run records: normally the caller; on replay the recorded principal and commitment.
struct Authority {
    record_as: Principal,
    commit: PrincipalCommit,
}

fn prepare_with<R: Reader + ?Sized>(
    env: &ImportEnv<'_>,
    auth: &Authority,
    command: &CommandId,
    reader: &R,
    params: &ImportParams,
    blobs: Option<&mut dyn BlobSink>,
) -> Result<PreparedImport, ImportError> {
    // 1. The stage, before anything touches the source.
    let stage = target_stage(env.tree, params.stage.as_deref())?;
    let format = reader.format().clone();
    if params.format.as_ref().is_some_and(|f| *f != format) {
        return Err(ImportError::Internal("the reader does not read the requested format"));
    }
    if env.registry.get(&format, Direction::Import).is_none() {
        return Err(IoError::FormatUnknown.into());
    }
    let options = normalise(&reader.options(&params.options).map_err(ImportError::BadOptions)?)?;
    let budget = Budget::new(env.principal, &env.limits)?;
    if is_set(env.cancel) {
        return Err(ImportError::Cancelled);
    }

    // 2. The source, confined.
    if env.principal != Principal::Human && !params.source.starts_with(crate::roots::SCHEME) {
        return Err(IoError::RawPathForbidden.into());
    }
    confine(&params.source)?;
    let mut file = env.roots.resolve_import(env.principal, &params.source)?;
    let location = location_of(&params.source)?;
    let name = file.path.file_name().and_then(|n| n.to_str()).map(str::to_string);
    if file.root.is_none() && !name.as_deref().is_some_and(plain_name) {
        return Err(IoError::PathOutsideRoot.into());
    }

    // 3. Limits before allocation: the size first, then a bounded prefix.
    let len = file.file.metadata().map_err(|e| IoError::Io { kind: e.kind(), at: Position::byte(0) })?.len();
    if len > budget.limits().bytes() {
        return Err(IoError::LimitExceeded {
            limit: LimitKind::Bytes,
            value: budget.limits().bytes(),
            at: Position::byte(0),
        }
        .into());
    }
    let prefix = read_prefix((&mut file.file).take(DETECT_PREFIX_LEN as u64))
        .map_err(|_| IoError::Io { kind: io::ErrorKind::Other, at: Position::byte(0) })?;
    file.file.seek(SeekFrom::Start(0)).map_err(|e| IoError::Io { kind: e.kind(), at: Position::byte(0) })?;

    // 4. The format, consistently.
    let named = if params.format.is_some() || command.as_str() != IMPORT_FILE { Some(&format) } else { None };
    let detection = choose_format(env.registry, &prefix, len, name.as_deref(), named)?;
    if *detection.format() != format {
        return Err(IoError::FormatMismatch.into());
    }
    drop(prefix);

    // 5. Read.
    let limits = *budget.limits();
    let inner: &mut dyn Read = &mut file.file;
    let mut src = HashingSource::new(BoundedSource::new(inner, budget))?;
    let mut report = ReportBuilder::new();
    let mut sink = StageSink {
        blobs: if params.dry_run { None } else { blobs },
        entities: Vec::new(),
        declared_unit: None,
        declared: BTreeMap::new(),
        cancel: env.cancel,
        fault: env.fault,
        cancelled: false,
        alloc_limit: limits.alloc(),
        alloc_used: 0,
        elements_limit: limits.elements(),
    };
    let r = reader.read(&mut src, &options, &mut sink, &mut report);
    if sink.cancelled || is_set(env.cancel) {
        return Err(ImportError::Cancelled);
    }
    match r {
        Ok(()) => {}
        Err(e @ (IoError::LimitExceeded { .. } | IoError::Io { .. })) => return Err(e.into()),
        Err(e) => return Err(ImportError::Reader(e)),
    }
    let (digest, _budget) = src.finish()?;

    // 6./7. Provenance, report, events.
    let unit = crate::prov::resolve_unit(sink.declared_unit, params.unit, false)?;
    let effective = EffectiveOptions::new(options.clone())?;
    let mut b = SourceProvenance::builder(digest, detection, reader.info().clone(), auth.commit, effective, unit);
    if let Some(n) = &name {
        b.source_name(UntrustedText::new(n))?;
    }
    for (k, v) in &sink.declared {
        b.declare(k, v.clone())?;
    }
    let provenance = b.build();
    // The record holds the provenance in its journal (canonical, read-back) form, so a record
    // read from the journal compares equal.
    let provenance_value = fid_canon::parse_with_limits(&provenance.canonical_bytes()?, MAX_JSON_BYTES, MAX_JSON_DEPTH)
        .map_err(|_| ImportError::Internal("provenance does not read back"))?;
    let provenance_sha256 = provenance.digest()?;
    let report = report.finish_import(format.clone(), location.clone(), params.dry_run);
    let report_sha256 = report_digest(&report)?;
    let event = ImportEvent {
        command: command.clone(),
        principal: auth.record_as,
        source: location,
        format,
        format_named: params.format.is_some(),
        unit: params.unit,
        stage,
        options,
        provenance: provenance_value,
        provenance_sha256,
        report_sha256,
        blobs: sink.entities.iter().map(|e| e.payload).collect(),
    };
    let mut events = entity_events(env, &digest.sha256(), &stage, &sink.entities)?;
    events.push(event.tree_event());
    // The commit must succeed on the tree's rules: try it on a copy.
    env.tree.clone().apply_commit(&events).map_err(ImportError::Tree)?;
    if is_set(env.cancel) {
        return Err(ImportError::Cancelled);
    }
    Ok(PreparedImport { events: if params.dry_run { Vec::new() } else { events }, out: ImportOutput { report, event } })
}

fn caller(env: &ImportEnv<'_>) -> Authority {
    Authority { record_as: env.principal, commit: env.principal_commit }
}

/// Phase one of the skeleton (see the module docs). Nothing in the project changes.
pub fn prepare_import<R: Reader + ?Sized>(
    env: &ImportEnv<'_>,
    command: &CommandId,
    reader: &R,
    params: &ImportParams,
    blobs: Option<&mut dyn BlobSink>,
) -> Result<PreparedImport, ImportError> {
    prepare_with(env, &caller(env), command, reader, params, blobs)
}

/// Exact-mode replay of a recorded import: the same command, reader, source and parameters
/// read again (with the recorded principal and commitment) give the same record and events.
///
/// **Trust:** `event` must come from the verified journal, never from a caller.
/// `env.tree` is the state before the recorded commit and `env.principal_commit` the
/// recorded entry header's commitment (the bus reads both from the journal). `IDENTICAL` when the new record and events equal the
/// recorded ones, `DIVERGENT` otherwise; `NOT_REPLAYABLE` for another command or format, or a
/// human's raw path.
pub fn replay_import<R: Reader + ?Sized>(
    env: &ImportEnv<'_>,
    command: &CommandId,
    reader: &R,
    event: &ImportEvent,
    recorded_events: &[TreeEvent],
) -> Result<ReplayOutcome, ImportError> {
    if event.command != *command || event.format != *reader.format() {
        return Ok(ReplayOutcome::NotReplayable);
    }
    let Some(handle) = event.source.as_handle() else { return Ok(ReplayOutcome::NotReplayable) };
    let mut params = ImportParams::new(&handle.to_string());
    params.format = if event.format_named { Some(event.format.clone()) } else { None };
    params.unit = event.unit;
    params.stage = Some(event.stage.to_string());
    params.options = event.options.clone();
    let auth = Authority { record_as: event.principal, commit: env.principal_commit };
    let p = prepare_with(env, &auth, command, reader, &params, None)?;
    let same = p.out.event == *event && p.events == recorded_events;
    Ok(if same { ReplayOutcome::Identical } else { ReplayOutcome::Divergent })
}

/// The command ID `import.<format>@1` of a per-format command (`-` written as `_`); fails
/// `command_id_taken` for a reserved or registered name.
pub fn format_command_id(format: &FormatId, taken: &[&str]) -> Result<CommandId, ImportError> {
    let name = format!("import.{}", format.as_str().replace('-', "_"));
    let base = |s: &str| s.split('@').next().unwrap_or(s).to_string();
    if RESERVED_IMPORT_NAMES.contains(&name.as_str()) || taken.iter().any(|t| base(t) == name) {
        return Err(ImportError::CommandIdTaken);
    }
    CommandId::parse(&format!("{name}@1")).map_err(|_| ImportError::BadParams("format: no valid command ID"))
}

/// The command ID of `import.file@1`.
pub fn import_file_id() -> Result<CommandId, ImportError> {
    CommandId::parse(IMPORT_FILE).map_err(|_| ImportError::Internal("import.file@1 is not a command ID"))
}

/// An import command as the conformance kit and the bus adapter see it.
pub trait ImportRun {
    /// The command ID (also its record's event type).
    fn command_id(&self) -> Result<CommandId, ImportError>;
    /// Whether its params take `format` (`import.file@1`).
    fn takes_format(&self) -> bool;
    /// Phase one ([`prepare_import`]).
    fn prepare(
        &self,
        env: &ImportEnv<'_>,
        params: &ImportParams,
        blobs: Option<&mut dyn BlobSink>,
    ) -> Result<PreparedImport, ImportError>;
    /// Exact-mode replay ([`replay_import`]).
    fn replay(
        &self,
        env: &ImportEnv<'_>,
        event: &ImportEvent,
        recorded_events: &[TreeEvent],
    ) -> Result<ReplayOutcome, ImportError>;
}

/// `import.file@1`: one skeleton, the reader picked by detection or by `format`.
pub struct ImportFile<'r> {
    readers: BTreeMap<FormatId, Box<dyn Reader + 'r>>,
}

impl fmt::Debug for ImportFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportFile").field("formats", &self.readers.keys().collect::<Vec<_>>()).finish()
    }
}

impl Default for ImportFile<'_> {
    fn default() -> Self {
        ImportFile { readers: BTreeMap::new() }
    }
}

impl<'r> ImportFile<'r> {
    pub fn new() -> Self {
        ImportFile::default()
    }

    /// Adds a reader; a second reader for the same format fails `format_duplicate`.
    pub fn add(&mut self, reader: Box<dyn Reader + 'r>) -> Result<(), ImportError> {
        let id = reader.format().clone();
        if self.readers.contains_key(&id) {
            return Err(IoError::FormatDuplicate.into());
        }
        self.readers.insert(id, reader);
        Ok(())
    }

    /// The reader for `params`: the named format's, or the one detection picks (the stage
    /// is checked first, and detection runs again inside the skeleton).
    fn reader_for(&self, env: &ImportEnv<'_>, params: &ImportParams) -> Result<&(dyn Reader + 'r), ImportError> {
        target_stage(env.tree, params.stage.as_deref())?;
        let format = match &params.format {
            Some(f) => f.clone(),
            None => {
                if env.principal != Principal::Human && !params.source.starts_with(crate::roots::SCHEME) {
                    return Err(IoError::RawPathForbidden.into());
                }
                confine(&params.source)?;
                let mut f = env.roots.resolve_import(env.principal, &params.source)?;
                let len = f.file.metadata().map_err(|e| IoError::Io { kind: e.kind(), at: Position::byte(0) })?.len();
                let budget = Budget::new(env.principal, &env.limits)?;
                if len > budget.limits().bytes() {
                    return Err(IoError::LimitExceeded {
                        limit: LimitKind::Bytes,
                        value: budget.limits().bytes(),
                        at: Position::byte(0),
                    }
                    .into());
                }
                let prefix = read_prefix((&mut f.file).take(DETECT_PREFIX_LEN as u64))
                    .map_err(|_| IoError::Io { kind: io::ErrorKind::Other, at: Position::byte(0) })?;
                let name = f.path.file_name().and_then(|n| n.to_str()).map(str::to_string);
                env.registry.detect(&prefix, len, name.as_deref(), None).map_err(detect_error)?
            }
        };
        self.readers.get(&format).map(|r| r.as_ref()).ok_or(ImportError::Io(IoError::FormatUnknown))
    }
}

impl ImportRun for ImportFile<'_> {
    fn command_id(&self) -> Result<CommandId, ImportError> {
        import_file_id()
    }

    fn takes_format(&self) -> bool {
        true
    }

    fn prepare(
        &self,
        env: &ImportEnv<'_>,
        params: &ImportParams,
        blobs: Option<&mut dyn BlobSink>,
    ) -> Result<PreparedImport, ImportError> {
        let reader = self.reader_for(env, params)?;
        prepare_import(env, &import_file_id()?, reader, params, blobs)
    }

    fn replay(
        &self,
        env: &ImportEnv<'_>,
        event: &ImportEvent,
        recorded_events: &[TreeEvent],
    ) -> Result<ReplayOutcome, ImportError> {
        match self.readers.get(&event.format) {
            Some(r) => replay_import(env, &import_file_id()?, r.as_ref(), event, recorded_events),
            None => Ok(ReplayOutcome::NotReplayable),
        }
    }
}

/// `import.<format>@1`: the skeleton with one fixed reader.
#[derive(Debug)]
pub struct ImportCommand<R> {
    reader: R,
    id: CommandId,
}

impl<R: Reader> ImportCommand<R> {
    /// The command for `reader`, named by [`format_command_id`].
    pub fn new(reader: R, taken: &[&str]) -> Result<ImportCommand<R>, ImportError> {
        let id = format_command_id(reader.format(), taken)?;
        Ok(ImportCommand { reader, id })
    }

    pub fn id(&self) -> &CommandId {
        &self.id
    }

    pub fn reader(&self) -> &R {
        &self.reader
    }
}

impl<R: Reader> ImportRun for ImportCommand<R> {
    fn command_id(&self) -> Result<CommandId, ImportError> {
        Ok(self.id.clone())
    }

    fn takes_format(&self) -> bool {
        false
    }

    fn prepare(
        &self,
        env: &ImportEnv<'_>,
        params: &ImportParams,
        blobs: Option<&mut dyn BlobSink>,
    ) -> Result<PreparedImport, ImportError> {
        if params.format.as_ref().is_some_and(|f| f != self.reader.format()) {
            return Err(ImportError::BadParams("format is fixed by the command"));
        }
        prepare_import(env, &self.id, &self.reader, params, blobs)
    }

    fn replay(
        &self,
        env: &ImportEnv<'_>,
        event: &ImportEvent,
        recorded_events: &[TreeEvent],
    ) -> Result<ReplayOutcome, ImportError> {
        replay_import(env, &self.id, &self.reader, event, recorded_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_entity_cost_covers_its_size() {
        let size = std::mem::size_of::<StagedEntity>() as u64;
        // The struct, plus Vec growth (at most doubling: one more struct's worth).
        assert!(STAGED_ENTITY_COST >= 2 * size, "{} < 2 * {size}", STAGED_ENTITY_COST);
    }
}
