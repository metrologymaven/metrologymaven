//! The generic import command `import.file@1` and the import skeleton every
//! `import.<format>@1` command is built from (spec node `IO.REG.ENV.IMPORT`,
//! REQ-IO.REG.ENV-001 to -005 and -007).
//!
//! A format reader implements [`Reader`]: it reads the untrusted file through
//! [`StageSink::source`] (a [`HashingSource`] over a [`BoundedSource`] under the
//! `IO.REG.LIMITS` budget) and stages entities, the declared unit and the format version in
//! the [`StageSink`]; warnings go to a [`ReportBuilder`]. The skeleton runs every import the
//! same way, whichever command started it, in two phases (the EXPORT pattern, #329).
//!
//! **Prepare** ([`prepare_import`]); nothing in the project changes:
//!
//! 1. the target stage is resolved **before the source is opened** (REQ-004): `stage` is a
//!    stage ID or a stage label (untrusted text); [`fid_project::Tree::target_stage`] or
//!    [`fid_project::Tree::stage_by_label`]; none is `stage_not_found`, several labels
//!    `stage_ambiguous`; no `stage` is the active stage. The limit request is checked here
//!    too (an agent raising a limit is `limit_raise_forbidden`), before any file is opened;
//! 2. the source is resolved through [`RootSet::resolve_import`] (`IO.REG.ROOTS`): agents and
//!    scripts pass `root://` handles only (`raw_path_forbidden`), inside the root, links
//!    included (`path_outside_root`). A component holding `~` and a digit (a Windows 8.3
//!    short-name alias), and a human raw path's file name that is not a plain name (trailing
//!    dot or space, `:` as in `a.ftp::$DATA`, a device name) fail `path_outside_root` too.
//!    A [`crate::RootWarning`] (a hard-linked file) becomes a report warning;
//! 3. every `IO.REG.LIMITS` bound is set up before anything is allocated for the content:
//!    the file's size is checked against the byte limit first, the detection prefix is read
//!    bounded at [`DETECT_PREFIX_LEN`], and the reader reads through a [`HashingSource`] over a
//!    [`BoundedSource`] whose every read first checks the cancel flag ([`CancelRead`]);
//! 4. the format is detected or checked, consistently: with no `format`, the
//!    `IO.REG.DETECT` rule (content signature, then the extension cross-check); with a
//!    `format` (or a per-format command), the content signatures and the extension must not
//!    name another format (`format_mismatch`);
//! 5. the reader reads. **One budget pays for everything**: the reader's own charges, and,
//!    per staged entity, [`STAGED_ENTITY_COST`] plus [`COMMIT_ENTITY_COST`] (the commit's two
//!    events, the tree element in the checked copy and in the project, the record's entity ID
//!    and manifest entry; measured, see `tests/import_alloc.rs`), its texts and its payload
//!    bytes, all charged before the entity is kept; after the read, the copy of the existing
//!    tree is charged before it is made. At most [`MAX_IMPORT_ENTITIES`] entities are staged
//!    (`limit_exceeded{elements}`). A cancel (checked before reading, on every read of the
//!    file, on every staged entity, after the reader returns) or any error ends the import
//!    with nothing to commit;
//! 6. payloads go to the content-addressed store through a [`BlobSink`] (`CORE.STO.CAS`;
//!    a dry run hashes them and stores nothing), and so does the blob manifest, the canonical
//!    JSON array of the payload addresses. A blob written by a failed or cancelled import is
//!    referenced by nothing (REQ-007);
//! 7. the [`SourceProvenance`] (`IO.REG.PROV`, with the reader's format version), the
//!    [`ImportReport`] and the commit's events are built: one `ElementCreated` (its `meta`
//!    holds the [`PROVENANCE_META_KEY`] marker: the value is claimed, never measured) and one
//!    `ElementSlotSet` (state `current`, the payload's digest) **in the target stage only** per
//!    entity (REQ-003), then the one [`ImportEvent`] record holding the provenance, the report
//!    digest, the entity IDs and the manifest's digest and count (REQ-002). The record is
//!    read back through [`ImportEvent::from_json`] before anything is returned, so a committed
//!    record is always readable. The events are applied to a copy of the tree, so a commit
//!    cannot fail on the tree's rules.
//!
//! **Commit** ([`PreparedImport::commit`]): the events go to the tree as one all-or-nothing
//! commit; for a dry run nothing happens (REQ-005). The bus adapter
//! (`CORE.BUS.PIPE.STAGE`, REQ-011, not built yet) commits the same events as one journal
//! entry.
//!
//! File content is untrusted: names are kept verbatim as untrusted text (`ElementName`). A
//! name holding a [`fid_types::text::FORBIDDEN_RANGES`] character, longer than
//! [`fid_project::MAX_NAME_CHARS`] or not NFC never loses its entity: the entity is kept as
//! `unnamed-<index>` and the report warns (`name_forbidden_character`, `name_too_long`,
//! `name_not_nfc`) with the position and the original text. Nothing in the content is
//! interpreted as a command, path or option, and the only events an import yields are the
//! three kinds above. Every failure maps to a `code/reason` pair of the bus error catalogue
//! v3 ([`ImportError::bus_code`]).

pub mod command;
#[cfg(any(test, feature = "test-hooks"))]
pub mod conformance;

use crate::budget::Budget;
use crate::detect::{read_prefix, DetectError, Registry, DETECT_PREFIX_LEN};
use crate::error::{IoError, Position};
use crate::limits::{LimitKind, LimitRequest, Principal};
use crate::prov::{
    Detection, EffectiveOptions, HashingSource, LengthUnit, PrincipalCommit, ProvError, ReaderInfo, SourceProvenance,
    VersionToken,
};
use crate::report::{Code, FormatId, ImportReport, Location, ReportBuilder, ReportError, ReportPos, TypeName};
use crate::roots::{plain_name, Direction, RootHandle, RootSet, RootWarning};
use crate::source::BoundedSource;
use crate::untrusted::UntrustedText;
use fid_canon::CValue;
use fid_project::{ElementName, ElementTypeRegistry, Tree, TreeError, TreeEvent, Uuid, MAX_NAME_CHARS};
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
/// `CHUNK_SIZE`), the most a value read (`CORE.PRJ.STAGE.SWITCH`) resolves. The blob manifest
/// of an import is bounded by the same size.
pub const MAX_PAYLOAD_BYTES: u64 = 8 * 1024 * 1024;
/// Most entities one import stages: an `IO.REG.LIMITS` element bound of this command (it
/// fails `limit_exceeded{elements}` with this value), below the caller's element limit and
/// not raisable. It keeps the record's entity ID list and the blob manifest (one blob, at
/// most [`MAX_PAYLOAD_BYTES`]) readable; both bounds are tested.
pub const MAX_IMPORT_ENTITIES: u64 = 100_000;
/// Tracked allocation charged per staged entity for the staged entry itself: at least the
/// size of a [`StagedEntity`] plus a Vec slot (tested).
pub const STAGED_ENTITY_COST: u64 = 256;
/// Tracked allocation charged per staged entity for building and checking the commit: its
/// two events, its element in the checked copy of the tree and in the project, its ID in the
/// record and its manifest entry (texts and payload are charged on top). Measured by
/// `tests/import_alloc.rs`, which fails if the real peak exceeds what was charged.
pub const COMMIT_ENTITY_COST: u64 = 24 * 1024;
/// How many copies of an entity's name and type text the import holds at once (staged entry,
/// event, checked copy, project), charged per byte.
pub const TEXT_COPIES: u64 = 4;
/// Tracked allocation charged once per import for the copy of the tree's fixed parts and the
/// record (besides [`COMMIT_ENTITY_COST`] per element already in the tree).
pub const TREE_BASE_COST: u64 = 64 * 1024;
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
/// Key of the provenance marker in an imported element's `meta`:
/// `{"claimed": true, "evidence": <IO.REG.PROV evidence>, "provenance_sha256": <digest>}`.
/// An imported value is claimed, never measured (VIZ.OVERLAY.STATUS REQ-007).
pub const PROVENANCE_META_KEY: &str = "provenance";
/// Prefix of the placeholder name of an entity whose name is refused.
pub const PLACEHOLDER_PREFIX: &str = "unnamed-";
/// Warning code: a name holding a forbidden character.
pub const NAME_FORBIDDEN: &str = "name_forbidden_character";
/// Warning code: a name longer than [`MAX_NAME_CHARS`].
pub const NAME_TOO_LONG: &str = "name_too_long";
/// Warning code: a name not in NFC.
pub const NAME_NOT_NFC: &str = "name_not_nfc";
/// Warning code of a hard-linked source file (`IO.REG.ROOTS`).
pub const HARD_LINKED: &str = "hard_linked";

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
        TreeError::BadStage(_) | TreeError::StageNotFound(_) | TreeError::AmbiguousStage(_) => {
            ("validation", "bad_stage")
        }
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
// Blobs, the cancellable source and the stage sink

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

fn len64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The file under the bounded source: every read first checks the cancel flag, so a long
/// read is cancellable between any two reads, whatever the reader does.
pub struct CancelRead<'s> {
    inner: &'s mut dyn Read,
    cancel: Option<&'s AtomicBool>,
}

impl fmt::Debug for CancelRead<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancelRead").field("cancel", &self.cancel.is_some()).finish_non_exhaustive()
    }
}

impl Read for CancelRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if is_set(self.cancel) {
            return Err(io::Error::other("import cancelled"));
        }
        self.inner.read(buf)
    }
}

/// The source a reader reads: the untrusted file through the cancellable, hashing, bounded
/// source.
pub type ImportSource<'s> = HashingSource<CancelRead<'s>>;

/// One entity a reader staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedEntity {
    /// Registered element type ID.
    pub type_id: String,
    /// The checked name (a placeholder if the file's name was refused).
    pub name: ElementName,
    /// Content address of the payload.
    pub payload: Digest,
}

/// A fault injected for the conformance kit and tests (feature `test-hooks`): the real
/// command path runs, only the sink misbehaves.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The sink takes `n` entities, then fails with `io{Other}` (a failing store).
    FailAfter(u64),
    /// A cancel arrives right after the `n`-th entity.
    CancelAfter(u64),
}

/// The test seams of an import; empty unless the `test-hooks` feature is on.
#[derive(Debug, Clone, Copy, Default)]
struct Hooks {
    #[cfg(any(test, feature = "test-hooks"))]
    fault: Option<Fault>,
}

#[cfg(any(test, feature = "test-hooks"))]
impl Hooks {
    fn fail_at(&self, staged: u64) -> bool {
        matches!(self.fault, Some(Fault::FailAfter(k)) if staged >= k)
    }

    fn cancel_after(&self, staged: u64) -> bool {
        matches!(self.fault, Some(Fault::CancelAfter(k)) if staged >= k)
    }
}

#[cfg(not(any(test, feature = "test-hooks")))]
impl Hooks {
    fn fail_at(&self, _staged: u64) -> bool {
        false
    }

    fn cancel_after(&self, _staged: u64) -> bool {
        false
    }
}

/// Tracked allocation of one staged entity with `text` bytes of name and type and a payload
/// of `payload` bytes (see [`COMMIT_ENTITY_COST`]).
pub fn entity_cost(text: u64, payload: u64) -> u64 {
    STAGED_ENTITY_COST
        .saturating_add(COMMIT_ENTITY_COST)
        .saturating_add(text.saturating_mul(TEXT_COPIES))
        .saturating_add(payload)
}

/// Why a name from the file cannot be an element name, if it cannot.
fn name_problem(raw: &str) -> Option<&'static str> {
    if raw.chars().any(fid_types::text::is_forbidden_char) {
        Some(NAME_FORBIDDEN)
    } else if raw.chars().nth(MAX_NAME_CHARS).is_some() {
        Some(NAME_TOO_LONG)
    } else if ElementName::new(raw).is_err() {
        Some(NAME_NOT_NFC)
    } else {
        None
    }
}

/// Where a reader reads from and puts what it read: the source, entities with their
/// payloads, the length unit and format version the file declares, untrusted metadata.
/// Every entity is charged to the source's budget before it is kept.
pub struct StageSink<'s, 'b> {
    src: ImportSource<'s>,
    blobs: Option<&'b mut dyn BlobSink>,
    entities: Vec<StagedEntity>,
    declared_unit: Option<LengthUnit>,
    format_version: Option<VersionToken>,
    declared: BTreeMap<String, UntrustedText>,
    cancel: Option<&'s AtomicBool>,
    hooks: Hooks,
    cancelled: bool,
}

impl fmt::Debug for StageSink<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StageSink").field("entities", &self.entities.len()).finish_non_exhaustive()
    }
}

impl<'s> StageSink<'s, '_> {
    /// The file, through the cancellable, hashing, bounded source. Its budget
    /// ([`HashingSource::budget_mut`]) is the one budget of the import.
    pub fn source(&mut self) -> &mut ImportSource<'s> {
        &mut self.src
    }

    /// Stages one entity of a registered element type with a name and its payload bytes.
    /// The payload goes to the store (hashed only on a dry run). The entity and its texts and
    /// payload are charged to the budget first ([`entity_cost`], one element). A name that
    /// cannot be an element name keeps the entity under `unnamed-<index>`, with a report
    /// warning holding the position, the reason and the original text.
    pub fn add_entity(
        &mut self,
        type_id: &str,
        name: &UntrustedText,
        payload: &[u8],
        at: Position,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        if self.cancelled || is_set(self.cancel) {
            self.cancelled = true;
            return Err(IoError::Io { kind: io::ErrorKind::Interrupted, at });
        }
        let n = len64(self.entities.len());
        if self.hooks.fail_at(n) {
            return Err(IoError::Io { kind: io::ErrorKind::Other, at });
        }
        if n >= MAX_IMPORT_ENTITIES {
            return Err(IoError::LimitExceeded { limit: LimitKind::Elements, value: MAX_IMPORT_ENTITIES, at });
        }
        let size = len64(payload.len());
        if size > MAX_PAYLOAD_BYTES {
            return Err(IoError::LimitExceeded { limit: LimitKind::Alloc, value: MAX_PAYLOAD_BYTES, at });
        }
        let kind =
            TypeName::new(type_id).map_err(|_| IoError::Malformed { at, reason: "entity type is not a type name" })?;
        let placeholder = format!("{PLACEHOLDER_PREFIX}{n}");
        let text = len64(name.raw().len().max(placeholder.len())).saturating_add(len64(type_id.len()));
        let budget = self.src.budget_mut();
        budget.charge_elements(1, at)?;
        budget.charge_alloc(entity_cost(text, size), at)?;
        let element_name = match name_problem(name.raw()) {
            None => ElementName::new(name.raw()).map_err(|_| IoError::Malformed { at, reason: "name" })?,
            Some(reason) => {
                let pos = ReportPos::new(at).map_err(|_| IoError::Malformed { at, reason: "bad position" })?;
                let code = Code::new(reason).map_err(|_| IoError::Malformed { at, reason: "bad code" })?;
                report.warn(&code, Some(pos), &UntrustedText::new(&format!("{placeholder}: {}", name.raw())));
                ElementName::new(&placeholder).map_err(|_| IoError::Malformed { at, reason: "placeholder" })?
            }
        };
        let payload = match self.blobs.as_deref_mut() {
            Some(b) => b.put(payload)?,
            None => sha256_of(payload),
        };
        self.entities.try_reserve(1).map_err(|_| IoError::Io { kind: io::ErrorKind::OutOfMemory, at })?;
        self.entities.push(StagedEntity { type_id: type_id.to_string(), name: element_name, payload });
        report.count(&kind, 1);
        if self.hooks.cancel_after(len64(self.entities.len())) {
            self.cancelled = true;
        }
        Ok(())
    }

    /// The length unit the file declares.
    pub fn declare_unit(&mut self, unit: LengthUnit) {
        self.declared_unit = Some(unit);
    }

    /// The format version the reader identified (recorded in the provenance, PROV-001).
    pub fn declare_format_version(&mut self, version: VersionToken) {
        self.format_version = Some(version);
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

    /// Reads [`StageSink::source`] with the effective options into `sink`, reporting to
    /// `report`. Must be deterministic for the same bytes and options, must treat every byte
    /// as untrusted data, and must charge what it allocates to the source's budget.
    fn read(&self, opts: &CValue, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError>;
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
    /// The provenance record (`IO.REG.PROV` canonical value, with the format version).
    pub provenance: CValue,
    /// SHA-256 of the provenance record's canonical bytes.
    pub provenance_sha256: Digest,
    /// SHA-256 of the canonical [`ImportReport`] without its `dry_run` flag.
    pub report_sha256: Digest,
    /// The created elements, in entity order (at most [`MAX_IMPORT_ENTITIES`]).
    pub entities: Vec<Uuid>,
    /// Content address of the blob manifest: the canonical JSON array of the entities'
    /// payload addresses, in entity order (the blobs the commit makes reachable).
    pub blob_manifest: Digest,
    /// Number of addresses in the manifest (one per entity).
    pub blob_count: u64,
}

const EVENT_KEYS: [&str; 14] = [
    "blob_count",
    "blob_manifest",
    "command",
    "entities",
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

fn canonical_uuid(s: &str) -> Option<Uuid> {
    Uuid::parse(s).ok().filter(|u| u.to_string() == s)
}

/// A `u64` field: the value, or its canonical decimal text (the read-back form).
fn u64_field(v: Option<&CValue>) -> Option<u64> {
    match v {
        Some(CValue::U64(n)) => Some(*n),
        Some(CValue::Str(s)) => s.parse::<u64>().ok().filter(|n| n.to_string() == *s),
        _ => None,
    }
}

/// The blob manifest of `payloads`: canonical JSON array of the addresses.
pub fn blob_manifest(payloads: &[Digest]) -> Result<Vec<u8>, ImportError> {
    let v = CValue::Arr(payloads.iter().map(|d| CValue::Str(d.as_str().to_string())).collect());
    fid_canon::to_canonical_bytes(&v).map_err(|_| ImportError::Internal("manifest is not canonical"))
}

impl ImportEvent {
    /// The record as JSON.
    pub fn to_value(&self) -> CValue {
        let s = |v: &str| CValue::Str(v.to_string());
        let mut o = BTreeMap::new();
        o.insert("blob_count".to_string(), CValue::U64(self.blob_count));
        o.insert("blob_manifest".to_string(), s(self.blob_manifest.as_str()));
        o.insert("command".to_string(), s(self.command.as_str()));
        o.insert("entities".to_string(), CValue::Arr(self.entities.iter().map(|u| s(&u.to_string())).collect()));
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
        let stage = canonical_uuid(str_field(o, "stage")?).ok_or(bad_event("stage"))?;
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
        let Some(CValue::Arr(items)) = o.get("entities") else { return Err(bad_event("entities must be an array")) };
        if len64(items.len()) > MAX_IMPORT_ENTITIES {
            return Err(bad_event("too many entities"));
        }
        let entities = items
            .iter()
            .map(|i| match i {
                CValue::Str(s) => canonical_uuid(s).ok_or(bad_event("invalid entity ID")),
                _ => Err(bad_event("entity ID must be a string")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let blob_manifest =
            Digest::parse(str_field(o, "blob_manifest")?).map_err(|_| bad_event("invalid blob_manifest"))?;
        let blob_count = u64_field(o.get("blob_count")).ok_or(bad_event("blob_count must be a u64"))?;
        if blob_count != len64(entities.len()) {
            return Err(bad_event("blob_count is not the entity count"));
        }
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
            entities,
            blob_manifest,
            blob_count,
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

/// What an import runs against. Built with [`ImportEnv::new`]; the public fields may then be
/// changed.
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
    /// Set to cancel; checked before reading, on every read of the file, on every staged
    /// entity and after the reader. Stands in for the `CORE.JOB` cancel token until the bus
    /// adapter passes the job's flag.
    pub cancel: Option<&'a AtomicBool>,
    hooks: Hooks,
}

impl fmt::Debug for ImportEnv<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportEnv").field("principal", &self.principal).finish_non_exhaustive()
    }
}

impl<'a> ImportEnv<'a> {
    /// An environment with the default limits and no cancel flag.
    pub fn new(
        registry: &'a Registry,
        roots: &'a RootSet,
        principal: Principal,
        principal_commit: PrincipalCommit,
        tree: &'a Tree,
        namespace: Uuid,
    ) -> ImportEnv<'a> {
        ImportEnv {
            registry,
            roots,
            principal,
            principal_commit,
            limits: LimitRequest::default(),
            tree,
            namespace,
            cancel: None,
            hooks: Hooks::default(),
        }
    }

    /// Test seam: injects a sink fault. Exists only with the `test-hooks` feature.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn set_fault(&mut self, fault: Option<Fault>) {
        self.hooks.fault = fault;
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
    manifest: Vec<u8>,
    alloc_charged: u64,
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

    /// The bytes of the blob manifest (stored unless a dry run).
    pub fn manifest(&self) -> &[u8] {
        &self.manifest
    }

    /// The tracked allocation the import charged to its budget, in bytes.
    pub fn alloc_charged(&self) -> u64 {
        self.alloc_charged
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

/// Step 2's lexical checks, before anything is resolved: agents and scripts pass handles
/// only, no component with a short-name alias, and a human raw path's file name must be a
/// plain name.
fn confine(principal: Principal, source: &str) -> Result<(), ImportError> {
    if principal != Principal::Human && !source.starts_with(crate::roots::SCHEME) {
        return Err(IoError::RawPathForbidden.into());
    }
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
    if let Some(id) = canonical_uuid(text) {
        return tree.target_stage(Some(&id)).map_err(|_| ImportError::StageNotFound);
    }
    tree.stage_by_label(text).map_err(|e| match e {
        TreeError::AmbiguousStage(_) => ImportError::StageAmbiguous,
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

/// The effective options in read-back form, within the `IO.REG.PROV` option bounds (so the
/// record, which nests them twice, stays within [`MAX_JSON_DEPTH`]).
fn normalise(v: &CValue) -> Result<CValue, ImportError> {
    let bytes = fid_canon::to_canonical_bytes(v).map_err(|_| ImportError::BadOptions("options are not canonical"))?;
    if bytes.len() > crate::prov::MAX_OPTIONS_BYTES {
        return Err(ImportError::BadOptions("options are too large"));
    }
    fid_canon::parse_with_limits(&bytes, crate::prov::MAX_OPTIONS_BYTES, crate::prov::MAX_OPTIONS_DEPTH)
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

/// The provenance marker of an imported element's `meta` (see [`PROVENANCE_META_KEY`]).
pub fn provenance_meta(evidence: &str, provenance_sha256: &Digest) -> CValue {
    let mut marker = BTreeMap::new();
    marker.insert("claimed".to_string(), CValue::Bool(true));
    marker.insert("evidence".to_string(), CValue::Str(evidence.to_string()));
    marker.insert("provenance_sha256".to_string(), CValue::Str(provenance_sha256.as_str().to_string()));
    let mut meta = BTreeMap::new();
    meta.insert(PROVENANCE_META_KEY.to_string(), CValue::Obj(marker));
    CValue::Obj(meta)
}

fn entity_events(
    root: &Uuid,
    stage: &Uuid,
    ids: &[Uuid],
    staged: &[StagedEntity],
    meta: &CValue,
) -> Result<Vec<TreeEvent>, ImportError> {
    let s = |v: String| CValue::Str(v);
    let root = root.to_string();
    let mut out = Vec::new();
    out.try_reserve(staged.len().saturating_mul(2).saturating_add(1))
        .map_err(|_| ImportError::Internal("cannot reserve the commit"))?;
    for (id, e) in ids.iter().zip(staged) {
        let id = id.to_string();
        let mut created = BTreeMap::new();
        created.insert("id".to_string(), s(id.clone()));
        created.insert("meta".to_string(), meta.clone());
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

/// A rough upper bound of the heap a value holds (BTreeMap nodes counted at twice the entry
/// size for their slack).
fn value_heap(v: &CValue) -> u64 {
    let slot = len64(std::mem::size_of::<CValue>());
    let key = len64(std::mem::size_of::<String>());
    match v {
        CValue::Str(s) => len64(s.len()),
        CValue::Arr(a) => {
            a.iter().fold(len64(a.len()).saturating_mul(slot), |acc, x| acc.saturating_add(value_heap(x)))
        }
        CValue::Obj(o) => o.iter().fold(len64(o.len()).saturating_mul(2 * (slot + key)), |acc, (k, x)| {
            acc.saturating_add(len64(k.len())).saturating_add(value_heap(x))
        }),
        _ => 0,
    }
}

/// Tracked allocation of the copy of `tree` the commit is checked on.
fn tree_copy_cost(tree: &Tree) -> u64 {
    tree.elements().fold(TREE_BASE_COST, |acc, e| {
        acc.saturating_add(COMMIT_ENTITY_COST)
            .saturating_add(len64(e.name().as_untrusted_str().len()).saturating_mul(TEXT_COPIES))
            .saturating_add(value_heap(e.meta()))
    })
}

/// How a run records: normally the caller; on replay the recorded principal and commitment.
struct Authority {
    record_as: Principal,
    commit: PrincipalCommit,
}

fn finished<T>(env: &ImportEnv<'_>, r: Result<T, IoError>) -> Result<T, ImportError> {
    match r {
        _ if is_set(env.cancel) => Err(ImportError::Cancelled),
        Ok(v) => Ok(v),
        Err(e) => Err(e.into()),
    }
}

fn prepare_with<R: Reader + ?Sized>(
    env: &ImportEnv<'_>,
    auth: &Authority,
    command: &CommandId,
    reader: &R,
    params: &ImportParams,
    blobs: Option<&mut dyn BlobSink>,
) -> Result<PreparedImport, ImportError> {
    // 1. The stage and the limit request, before anything touches the source.
    let stage = target_stage(env.tree, params.stage.as_deref())?;
    let budget = Budget::new(env.principal, &env.limits)?;
    let format = reader.format().clone();
    if params.format.as_ref().is_some_and(|f| *f != format) {
        return Err(ImportError::Internal("the reader does not read the requested format"));
    }
    if env.registry.get(&format, Direction::Import).is_none() {
        return Err(IoError::FormatUnknown.into());
    }
    let options = normalise(&reader.options(&params.options).map_err(ImportError::BadOptions)?)?;
    if is_set(env.cancel) {
        return Err(ImportError::Cancelled);
    }

    // 2. The source, confined.
    confine(env.principal, &params.source)?;
    let mut file = env.roots.resolve_import(env.principal, &params.source)?;
    let location = location_of(&params.source)?;
    let name = file.path.file_name().and_then(|n| n.to_str()).map(str::to_string);
    if file.root.is_none() && !name.as_deref().is_some_and(plain_name) {
        return Err(IoError::PathOutsideRoot.into());
    }
    let mut report = ReportBuilder::new();
    for w in &file.warnings {
        let code = Code::new(HARD_LINKED).map_err(|_| ImportError::Internal("warning code"))?;
        let RootWarning::HardLinked { links } = w;
        report.warn(&code, None, &UntrustedText::new(&format!("{links} links")));
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

    // 5. Read, under the one budget.
    let inner = CancelRead { inner: &mut file.file, cancel: env.cancel };
    let src = HashingSource::new(BoundedSource::new(inner, budget))?;
    let mut sink = StageSink {
        src,
        blobs: if params.dry_run { None } else { blobs },
        entities: Vec::new(),
        declared_unit: None,
        format_version: None,
        declared: BTreeMap::new(),
        cancel: env.cancel,
        hooks: env.hooks,
        cancelled: false,
    };
    let r = reader.read(&options, &mut sink, &mut report);
    if sink.cancelled || is_set(env.cancel) {
        return Err(ImportError::Cancelled);
    }
    match r {
        Ok(()) => {}
        Err(e @ (IoError::LimitExceeded { .. } | IoError::Io { .. })) => return Err(e.into()),
        Err(e) => return Err(ImportError::Reader(e)),
    }
    let StageSink { src, blobs, entities, declared_unit, format_version, declared, .. } = sink;
    let (digest, mut budget) = finished(env, src.finish())?;
    // The checked copy of the tree, before it is made.
    finished(env, budget.charge_alloc(tree_copy_cost(env.tree), Position::byte(digest.bytes())))?;

    // 6./7. Provenance, report, manifest, events, record.
    let unit = crate::prov::resolve_unit(declared_unit, params.unit, false)?;
    let effective = EffectiveOptions::new(options.clone())?;
    let mut b = SourceProvenance::builder(digest, detection, reader.info().clone(), auth.commit, effective, unit);
    if let Some(n) = &name {
        b.source_name(UntrustedText::new(n))?;
    }
    if let Some(v) = format_version {
        b.format_version(v);
    }
    for (k, v) in &declared {
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
    let payloads: Vec<Digest> = entities.iter().map(|e| e.payload).collect();
    let manifest = blob_manifest(&payloads)?;
    if len64(manifest.len()) > MAX_PAYLOAD_BYTES {
        return Err(ImportError::Internal("blob manifest above one blob"));
    }
    let blob_manifest = match blobs {
        Some(b) => b.put(&manifest)?,
        None => sha256_of(&manifest),
    };
    let ids = (0..entities.len())
        .map(|i| entity_id(env.namespace, &digest.sha256(), &stage, len64(i)))
        .collect::<Result<Vec<_>, _>>()?;
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
        entities: ids,
        blob_manifest,
        blob_count: len64(payloads.len()),
    };
    // Every committed record must read back: check it now, on the bytes the journal keeps.
    let json = event.to_json()?;
    finished(env, budget.charge_alloc(len64(json.len()).saturating_mul(4), Position::byte(digest.bytes())))?;
    match ImportEvent::from_json(&json) {
        Ok(back) if back == event => {}
        _ => return Err(ImportError::Internal("the record does not read back")),
    }
    drop(json);
    let meta = provenance_meta(provenance.evidence().as_str(), &provenance_sha256);
    let mut events = entity_events(&env.tree.root(), &stage, &event.entities, &entities, &meta)?;
    drop(entities);
    events.push(event.tree_event());
    // The commit must succeed on the tree's rules: try it on a copy.
    env.tree.clone().apply_commit(&events).map_err(ImportError::Tree)?;
    if is_set(env.cancel) {
        return Err(ImportError::Cancelled);
    }
    Ok(PreparedImport {
        events: if params.dry_run { Vec::new() } else { events },
        out: ImportOutput { report, event },
        manifest,
        alloc_charged: budget.alloc_used(),
    })
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
/// recorded entry header's commitment (the bus reads both from the journal). `IDENTICAL`
/// when the new record and events equal the recorded ones, `DIVERGENT` otherwise;
/// `NOT_REPLAYABLE` for another command or format, or a human's raw path.
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
    /// The format a per-format command reads (`None` for `import.file@1`).
    fn fixed_format(&self) -> Option<&FormatId>;
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
#[derive(Default)]
pub struct ImportFile<'r> {
    readers: BTreeMap<FormatId, Box<dyn Reader + 'r>>,
}

impl fmt::Debug for ImportFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportFile").field("formats", &self.readers.keys().collect::<Vec<_>>()).finish()
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

    /// The reader for `params`: the named format's, or the one detection picks. The stage
    /// and the limit request are checked first, before the source is opened, and detection
    /// runs again inside the skeleton.
    fn reader_for(&self, env: &ImportEnv<'_>, params: &ImportParams) -> Result<&(dyn Reader + 'r), ImportError> {
        target_stage(env.tree, params.stage.as_deref())?;
        let budget = Budget::new(env.principal, &env.limits)?;
        let format = match &params.format {
            Some(f) => f.clone(),
            None => {
                confine(env.principal, &params.source)?;
                let mut f = env.roots.resolve_import(env.principal, &params.source)?;
                let len = f.file.metadata().map_err(|e| IoError::Io { kind: e.kind(), at: Position::byte(0) })?.len();
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

    fn fixed_format(&self) -> Option<&FormatId> {
        None
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

    fn fixed_format(&self) -> Option<&FormatId> {
        Some(self.reader.format())
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
        // The commit cost holds at least two events and the two element copies.
        let event = std::mem::size_of::<TreeEvent>() as u64;
        assert!(COMMIT_ENTITY_COST >= 4 * event, "{} < 4 * {event}", COMMIT_ENTITY_COST);
    }

    #[test]
    fn the_manifest_of_the_entity_cap_is_one_blob() {
        let d = sha256_of(b"x");
        let n = usize::try_from(MAX_IMPORT_ENTITIES).unwrap_or(usize::MAX);
        let manifest = blob_manifest(&vec![d; n]).unwrap_or_default();
        assert!(!manifest.is_empty());
        assert!(len64(manifest.len()) <= MAX_PAYLOAD_BYTES, "{}", manifest.len());
    }

    #[test]
    fn hooks_are_off_by_default() {
        let h = Hooks::default();
        assert!(!h.fail_at(u64::MAX) && !h.cancel_after(u64::MAX));
    }
}
