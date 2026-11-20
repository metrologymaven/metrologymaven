//! The generic export command `export.file@1` and the export skeleton every
//! `export.<format>@1` command is built from (spec node `IO.REG.ENV.EXPORT`,
//! REQ-IO.REG.ENV-005).
//!
//! A format writer implements [`Writer`]: entities and options in, bytes out to a
//! [`ByteSink`], lossy items reported to a [`ReportBuilder`]. The skeleton runs every export
//! the same way, whichever command started it, in two phases.
//!
//! **Prepare** ([`prepare_export`]):
//!
//! 1. `force_extension` from any principal but a human is refused (`policy/human_only`);
//! 2. the writer's format must be registered for export in the format [`Registry`]
//!    (`format_unknown`), and the writer checks the options and fills in their defaults;
//! 3. the target is resolved through [`RootSet::resolve_export`] (`IO.REG.ROOTS`): agents and
//!    scripts pass `root://` handles only (`raw_path_forbidden`), and the path stays inside
//!    the root, links included (`path_outside_root`). A human's raw path whose file name is
//!    not a plain name (trailing dot or space, `:`, a device name; the `IO.REG.ROOTS` rule
//!    for handle components) fails `path_outside_root` too, so `a.stl.` or `a.stl::$DATA`
//!    cannot dodge the extension check;
//! 4. the extension of the **resolved** file name is cross-checked against the registry
//!    (`IO.REG.DETECT`): an extension that some export format declares, but not the chosen
//!    one, fails `format_mismatch` unless a human set `force_extension` (REQ-004). An
//!    extension no export format declares, or none, is not this check's business;
//! 5. the target checks of `IO.REG.WRITE` run explicitly (`target_exists`; a link, folder
//!    or read-only file under `overwrite` is `target_protected`);
//! 6. the bytes go to an [`AtomicWriter`]'s temp file, which is then flushed, synced and
//!    closed, or, for `dry_run`, to a [`CountingSink`], so no file is created (REQ-002);
//! 7. the [`ExportReport`], its digest and (unless a dry run) the [`ExportEvent`] are built.
//!
//! **Place** ([`PreparedExport::place`]): the synced temp file is moved onto the target
//! (no clobbering). [`PreparedExport::discard`] (or drop) removes the temp file instead. The
//! bus adapter places only after the journal commit succeeded, and discards otherwise.
//!
//! A writer error, a sink error, a cancel (checked before the first byte, on every write,
//! after the writer returns and before placing) or a failed placement leaves no file and
//! no record (REQ-003). Every failure maps to a `code/reason` pair of the bus error
//! catalogue ([`ExportError::bus_code`]). Writing into a local export root is W1 with
//! `fs_effect: write_root` (CONSTITUTION §3.1). Bytes and digest are D0.

pub mod command;
pub mod conformance;

use crate::detect::{extension_of, OriginFilter, Registry};
use crate::error::{IoError, Position};
use crate::limits::{LimitKind, Principal, DEFAULT_BYTES};
use crate::prov::VersionToken;
use crate::report::{ExportReport, FormatId, Location, ReportBuilder, ReportError, TypeName, MAX_LOSSY};
use crate::roots::{plain_name, Direction, RootHandle, RootSet};
use crate::write::{AtomicWriter, WriteOptions, WriteReceipt};
use fid_canon::CValue;
use fid_replay::ReplayOutcome;
use fid_types::{CommandId, Digest};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

/// Command ID of the generic export command.
pub const EXPORT_FILE: &str = "export.file@1";
/// Export command names no format may take: `export.file` and `export.list_formats`.
pub const RESERVED_EXPORT_NAMES: &[&str] = &["export.file", "export.list_formats"];
/// Most entity IDs one `source` selection may name.
pub const MAX_SOURCE_IDS: usize = 100_000;
/// Longest `target` text, in bytes.
pub const MAX_TARGET_BYTES: usize = 4096;
/// Largest params document or journal event [`ExportParams::from_json`] and
/// [`ExportEvent::from_json`] accept, in bytes.
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
/// Deepest nesting of a params document or journal event (options included).
pub const MAX_JSON_DEPTH: usize = 64;

// ---------------------------------------------------------------------------------------
// Sinks

/// Where a writer puts its bytes: the atomic file writer, or a counting sink for a dry run.
pub trait ByteSink {
    /// Appends `buf`. After an error every later call fails.
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError>;
    /// Bytes accepted so far.
    fn bytes_written(&self) -> u64;
}

impl ByteSink for AtomicWriter {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        AtomicWriter::write_bytes(self, buf)
    }

    fn bytes_written(&self) -> u64 {
        AtomicWriter::bytes_written(self)
    }
}

/// The dry-run sink: takes SHA-256 and the byte count, stores nothing, and enforces the same
/// size bound as the atomic writer (`limit_exceeded` (`bytes`) past `max_bytes`).
#[derive(Clone)]
pub struct CountingSink {
    hasher: Sha256,
    bytes: u64,
    max_bytes: u64,
    failed: Option<IoError>,
}

impl fmt::Debug for CountingSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CountingSink")
            .field("bytes", &self.bytes)
            .field("max_bytes", &self.max_bytes)
            .field("failed", &self.failed)
            .finish()
    }
}

impl CountingSink {
    /// An empty sink accepting at most `max_bytes` bytes.
    pub fn new(max_bytes: u64) -> CountingSink {
        CountingSink { hasher: Sha256::new(), bytes: 0, max_bytes, failed: None }
    }

    /// The digest and size of everything accepted, or the first error.
    pub fn finish(self) -> Result<WriteReceipt, IoError> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        Ok(WriteReceipt { sha256: Digest::from_bytes(self.hasher.finalize().into()), bytes: self.bytes })
    }
}

impl ByteSink for CountingSink {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        let end = u64::try_from(buf.len()).ok().and_then(|n| self.bytes.checked_add(n));
        match end {
            Some(end) if end <= self.max_bytes => {
                self.hasher.update(buf);
                self.bytes = end;
                Ok(())
            }
            _ => {
                let e = IoError::LimitExceeded {
                    limit: LimitKind::Bytes,
                    value: self.max_bytes,
                    at: Position::byte(self.bytes),
                };
                self.failed = Some(e.clone());
                Err(e)
            }
        }
    }

    fn bytes_written(&self) -> u64 {
        self.bytes
    }
}

/// A fault injected into the sink, for the conformance kit and tests: the real command path
/// runs, only the sink misbehaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The sink takes `n` bytes, then fails with `io{Other}` (a failing disk).
    FailAfter(u64),
    /// A cancel arrives right after the writer's first write call.
    CancelMidWrite,
}

/// Wraps the real sink: checks the cancel flag on every write, applies an injected fault,
/// and remembers the first sink error so a writer cannot swallow it.
struct Guarded<'a> {
    inner: &'a mut dyn ByteSink,
    cancel: Option<&'a AtomicBool>,
    fault: Option<Fault>,
    cancelled: bool,
    sink_error: Option<IoError>,
}

impl Guarded<'_> {
    fn sink_err(&mut self, e: IoError) -> IoError {
        if self.sink_error.is_none() {
            self.sink_error = Some(e.clone());
        }
        e
    }
}

impl ByteSink for Guarded<'_> {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        let at = Position::byte(self.inner.bytes_written());
        if self.cancelled || is_set(self.cancel) {
            self.cancelled = true;
            return Err(IoError::Io { kind: io::ErrorKind::Interrupted, at });
        }
        if let Some(e) = &self.sink_error {
            return Err(e.clone());
        }
        let r = match self.fault {
            Some(Fault::FailAfter(n)) => {
                let room = usize::try_from(n.saturating_sub(self.inner.bytes_written())).unwrap_or(usize::MAX);
                if buf.len() > room {
                    match self.inner.write_bytes(buf.get(..room).unwrap_or_default()) {
                        Ok(()) => Err(IoError::Io { kind: io::ErrorKind::Other, at: Position::byte(n) }),
                        Err(e) => Err(e),
                    }
                } else {
                    self.inner.write_bytes(buf)
                }
            }
            Some(Fault::CancelMidWrite) => {
                self.cancelled = true;
                self.inner.write_bytes(buf)
            }
            None => self.inner.write_bytes(buf),
        };
        r.map_err(|e| self.sink_err(e))
    }

    fn bytes_written(&self) -> u64 {
        self.inner.bytes_written()
    }
}

fn is_set(flag: Option<&AtomicBool>) -> bool {
    flag.is_some_and(|f| f.load(Ordering::SeqCst))
}

// ---------------------------------------------------------------------------------------
// Writers

/// One format writer.
pub trait Writer {
    /// The entities a writer reads (the project's entity view; a test type in tests).
    type Source: ?Sized;

    /// The format it writes; must be registered for export.
    fn format(&self) -> &FormatId;

    /// The writer version, journaled with every export.
    fn version(&self) -> &VersionToken;

    /// Checks the caller's options and returns the effective options (defaults filled in).
    /// The default accepts any JSON object unchanged. An error is a static reason; the
    /// export then fails `validation/schema` before any file is touched.
    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        match given {
            CValue::Obj(_) => Ok(given.clone()),
            _ => Err("options must be an object"),
        }
    }

    /// Serialises `src` with the effective options `opts` into `sink`, reporting warnings and
    /// lossy names to `report`. Must be deterministic (D0) for the same inputs. An error it
    /// returns (other than one the sink gave it) is a writer failure (`internal/handler_failed`).
    fn write(
        &self,
        src: &Self::Source,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError>;
}

impl<W: Writer + ?Sized> Writer for &W {
    type Source = W::Source;

    fn format(&self) -> &FormatId {
        (**self).format()
    }

    fn version(&self) -> &VersionToken {
        (**self).version()
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        (**self).options(given)
    }

    fn write(
        &self,
        src: &Self::Source,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        (**self).write(src, opts, sink, report)
    }
}

// ---------------------------------------------------------------------------------------
// Errors

/// Why an export failed. Nothing was written and nothing is to be journaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// A harness error: root resolution, the extension check, the target check, or the
    /// sink (atomic writer or counting sink).
    Io(IoError),
    /// The writer itself failed (returned an error the sink did not give it).
    Writer(IoError),
    /// The target is a link, folder or read-only file and `overwrite` is set (the explicit
    /// target check of `IO.REG.WRITE`).
    TargetProtected(IoError),
    /// The export was cancelled.
    Cancelled,
    /// `force_extension` was set by a principal that is not a human.
    ForceExtensionHumanOnly,
    /// A per-format command ID is reserved or already registered.
    CommandIdTaken,
    /// The parameters do not fit the command's params schema.
    BadParams(&'static str),
    /// The writer refused the options.
    BadOptions(&'static str),
    /// A journal event could not be read back.
    BadEvent(&'static str),
    /// A broken invariant (a bug, never a caller mistake).
    Internal(&'static str),
}

impl From<IoError> for ExportError {
    fn from(e: IoError) -> ExportError {
        ExportError::Io(e)
    }
}

impl ExportError {
    /// The leaf's own stable code: the [`IoError`] code, or `writer_failed`,
    /// `target_protected`, `cancelled`, `force_extension_human_only`, `command_id_taken`,
    /// `bad_params`, `bad_options`, `bad_event`, `internal`.
    pub fn code(&self) -> &'static str {
        match self {
            ExportError::Io(e) => e.code(),
            ExportError::Writer(_) => "writer_failed",
            ExportError::TargetProtected(_) => "target_protected",
            ExportError::Cancelled => "cancelled",
            ExportError::ForceExtensionHumanOnly => "force_extension_human_only",
            ExportError::CommandIdTaken => "command_id_taken",
            ExportError::BadParams(_) => "bad_params",
            ExportError::BadOptions(_) => "bad_options",
            ExportError::BadEvent(_) => "bad_event",
            ExportError::Internal(_) => "internal",
        }
    }

    /// The bus error `(code, reason)` this failure is reported as (catalogue v3 of
    /// `bus-errors.json`, CORE.BUS.PIPE.ERR). Refusals (the caller can fix them) are
    /// `validation`, `policy`, `precondition` or `cancelled`. Environment faults (a
    /// file-system error while writing, including an OS permission error, which is
    /// `internal/storage`: only the explicit target check gives `policy/target_protected`)
    /// and bugs (a writer failure, a crashed plugin, a broken invariant) are `internal`.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            ExportError::Io(e) => io_bus_code(e),
            ExportError::Writer(_) => ("internal", "handler_failed"),
            ExportError::TargetProtected(_) => ("policy", "target_protected"),
            ExportError::Cancelled => ("cancelled", "requested"),
            ExportError::ForceExtensionHumanOnly => ("policy", "human_only"),
            ExportError::BadParams(_) | ExportError::BadOptions(_) | ExportError::BadEvent(_) => {
                ("validation", "schema")
            }
            ExportError::CommandIdTaken | ExportError::Internal(_) => ("internal", "invariant_violated"),
        }
    }
}

fn io_bus_code(e: &IoError) -> (&'static str, &'static str) {
    match e {
        IoError::RawPathForbidden => ("policy", "raw_path_refused"),
        IoError::PathOutsideRoot => ("policy", "outside_root"),
        IoError::RootNotFound => ("policy", "bad_root"),
        IoError::TargetExists => ("policy", "would_overwrite"),
        IoError::FormatMismatch => ("policy", "extension_refused"),
        IoError::LimitRaiseForbidden { .. } => ("policy", "human_only"),
        IoError::FormatUnknown | IoError::FormatAmbiguous => ("validation", "schema"),
        IoError::LimitExceeded { limit: LimitKind::Wall, .. } => ("cancelled", "timeout"),
        IoError::LimitExceeded { .. } => ("validation", "effect_limit"),
        IoError::UnitRequired | IoError::UnitConflict => ("precondition", "false"),
        IoError::Io { kind: io::ErrorKind::NotFound, .. } => ("precondition", "false"),
        IoError::Io { .. } => ("internal", "storage"),
        IoError::Malformed { .. } | IoError::TruncatedFile { .. } | IoError::ReaderCrashed => {
            ("internal", "handler_failed")
        }
        IoError::FormatDuplicate => ("internal", "invariant_violated"),
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Io(e) => write!(f, "{e}"),
            ExportError::Writer(e) | ExportError::TargetProtected(e) => write!(f, "{}: {e}", self.code()),
            ExportError::BadParams(r) | ExportError::BadOptions(r) | ExportError::BadEvent(r) => {
                write!(f, "{}: {r}", self.code())
            }
            ExportError::Internal(r) => write!(f, "internal: {r}"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for ExportError {}

// ---------------------------------------------------------------------------------------
// Parameters

/// The `source` parameter: one entity, or a selection of entities, by lowercase UUID.
/// Resolving it to entities is the caller's (the bus's state view); the skeleton only
/// records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// One entity (`EntityRef`).
    One(String),
    /// A non-empty selection (`Selection`), in the caller's order.
    Selection(Vec<String>),
}

/// The pattern of an entity ID: a lowercase hyphenated UUID.
pub const ENTITY_ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

fn is_entity_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => matches!(c, b'0'..=b'9' | b'a'..=b'f'),
        })
}

impl SourceRef {
    /// Reads the parameter value: a string, or a non-empty array of at most
    /// [`MAX_SOURCE_IDS`] strings, each an entity ID.
    pub fn from_value(v: &CValue) -> Result<SourceRef, ExportError> {
        match v {
            CValue::Str(s) if is_entity_id(s) => Ok(SourceRef::One(s.clone())),
            CValue::Arr(items) if !items.is_empty() && items.len() <= MAX_SOURCE_IDS => items
                .iter()
                .map(|i| match i {
                    CValue::Str(s) if is_entity_id(s) => Ok(s.clone()),
                    _ => Err(ExportError::BadParams("source: not an entity ID")),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(SourceRef::Selection),
            _ => Err(ExportError::BadParams("source: an entity ID or a non-empty list of them")),
        }
    }

    /// The parameter value.
    pub fn to_value(&self) -> CValue {
        match self {
            SourceRef::One(s) => CValue::Str(s.clone()),
            SourceRef::Selection(ids) => CValue::Arr(ids.iter().map(|s| CValue::Str(s.clone())).collect()),
        }
    }
}

/// The parameters of `export.file@1` (with `format`) or of an `export.<format>@1` command
/// (without: the command fixes the format).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportParams {
    /// What to export.
    pub source: SourceRef,
    /// The format (`export.file@1` only).
    pub format: Option<FormatId>,
    /// A `root://` handle, or for a human principal also a raw path.
    pub target: String,
    /// Writer options; `{}` when not given.
    pub options: CValue,
    /// Replace an existing regular file at the target.
    pub overwrite: bool,
    /// Write even if the extension names another format (human principals only).
    pub force_extension: bool,
    /// Serialise and report only; create no file.
    pub dry_run: bool,
}

fn flag(o: &BTreeMap<String, CValue>, key: &str) -> Result<bool, ExportError> {
    match o.get(key) {
        None => Ok(false),
        Some(CValue::Bool(b)) => Ok(*b),
        Some(_) => Err(ExportError::BadParams("a flag must be a boolean")),
    }
}

impl ExportParams {
    /// Parameters with the given source, format and target, and every optional field at its
    /// default.
    pub fn new(source: SourceRef, format: Option<FormatId>, target: &str) -> ExportParams {
        ExportParams {
            source,
            format,
            target: target.to_string(),
            options: CValue::Obj(BTreeMap::new()),
            overwrite: false,
            force_extension: false,
            dry_run: false,
        }
    }

    /// Reads a params object strictly: unknown fields, missing required fields and wrong
    /// types fail `validation/schema`. `with_format` is true for `export.file@1`, whose
    /// params require `format`; per-format commands refuse it.
    pub fn from_value(v: &CValue, with_format: bool) -> Result<ExportParams, ExportError> {
        let CValue::Obj(o) = v else { return Err(ExportError::BadParams("params must be an object")) };
        const KEYS: [&str; 7] = ["dry_run", "force_extension", "format", "options", "overwrite", "source", "target"];
        if o.keys().any(|k| !KEYS.contains(&k.as_str()) || (k == "format" && !with_format)) {
            return Err(ExportError::BadParams("unknown field"));
        }
        let source = SourceRef::from_value(o.get("source").ok_or(ExportError::BadParams("source is required"))?)?;
        let format = if with_format {
            match o.get("format") {
                Some(CValue::Str(s)) => {
                    Some(FormatId::new(s).map_err(|_| ExportError::BadParams("format: not a format ID"))?)
                }
                _ => return Err(ExportError::BadParams("format is required")),
            }
        } else {
            None
        };
        let target = match o.get("target") {
            Some(CValue::Str(s)) if !s.is_empty() && s.len() <= MAX_TARGET_BYTES => s.clone(),
            _ => return Err(ExportError::BadParams("target: a non-empty string")),
        };
        let options = match o.get("options") {
            None => CValue::Obj(BTreeMap::new()),
            Some(v @ CValue::Obj(_)) => v.clone(),
            Some(_) => return Err(ExportError::BadParams("options must be an object")),
        };
        Ok(ExportParams {
            source,
            format,
            target,
            options,
            overwrite: flag(o, "overwrite")?,
            force_extension: flag(o, "force_extension")?,
            dry_run: flag(o, "dry_run")?,
        })
    }

    /// Parses params JSON (bounded, duplicate keys refused by `fid-canon`), then
    /// [`ExportParams::from_value`].
    pub fn from_json(bytes: &[u8], with_format: bool) -> Result<ExportParams, ExportError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| ExportError::BadParams("not acceptable JSON"))?;
        ExportParams::from_value(&v, with_format)
    }

    /// The params object, every field written out.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("dry_run".to_string(), CValue::Bool(self.dry_run));
        o.insert("force_extension".to_string(), CValue::Bool(self.force_extension));
        if let Some(f) = &self.format {
            o.insert("format".to_string(), CValue::Str(f.as_str().to_string()));
        }
        o.insert("options".to_string(), self.options.clone());
        o.insert("overwrite".to_string(), CValue::Bool(self.overwrite));
        o.insert("source".to_string(), self.source.to_value());
        o.insert("target".to_string(), CValue::Str(self.target.clone()));
        CValue::Obj(o)
    }

    /// Folds the bus's own `dry_run` (CORE.BUS) into the export's: a bus dry run must never
    /// reach the atomic writer, so it always becomes an export dry run. The bus adapter calls
    /// this before [`prepare_export`].
    pub fn apply_bus_dry_run(&mut self, bus_dry_run: bool) {
        self.dry_run |= bus_dry_run;
    }
}

// ---------------------------------------------------------------------------------------
// Journal event

/// The journal record of one committed export (`export.file@1` or `export.<format>@1`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportEvent {
    /// The command that ran.
    pub command: CommandId,
    /// The principal kind that ran it (replay honours `force_extension` only for `human`).
    pub principal: Principal,
    /// What was exported.
    pub source: SourceRef,
    /// The target handle (or a human's raw path).
    pub target: Location,
    /// The format written.
    pub format: FormatId,
    /// The writer's version.
    pub writer_version: VersionToken,
    /// The effective options.
    pub options: CValue,
    /// The `overwrite` parameter.
    pub overwrite: bool,
    /// The `force_extension` parameter.
    pub force_extension: bool,
    /// SHA-256 of the file written.
    pub sha256: Digest,
    /// Its size in bytes.
    pub bytes: u64,
    /// Names the format could not represent, sorted.
    pub lossy: BTreeSet<TypeName>,
    /// SHA-256 of the canonical [`ExportReport`] (the result digest, CONSTITUTION §3).
    pub report_sha256: Digest,
}

const EVENT_KEYS: [&str; 13] = [
    "bytes",
    "command",
    "force_extension",
    "format",
    "lossy",
    "options",
    "overwrite",
    "principal",
    "report_sha256",
    "sha256",
    "source",
    "target",
    "writer_version",
];

fn bad_event(reason: &'static str) -> ExportError {
    ExportError::BadEvent(reason)
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, ExportError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(bad_event("a field is missing or not a string")),
    }
}

fn bool_field(o: &BTreeMap<String, CValue>, key: &str) -> Result<bool, ExportError> {
    match o.get(key) {
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(bad_event("a flag is missing or not a boolean")),
    }
}

fn decimal_u64(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    let ok = !b.is_empty() && b.len() <= 20 && b.iter().all(u8::is_ascii_digit) && (b.len() == 1 || b[0] != b'0');
    if ok {
        s.parse().ok()
    } else {
        None
    }
}

fn principal_from_wire(s: &str) -> Option<Principal> {
    [Principal::Human, Principal::Agent, Principal::Script].into_iter().find(|p| p.as_str() == s)
}

impl ExportEvent {
    /// The record as JSON; `bytes` is a decimal string (the `fid-canon` 64-bit rule).
    pub fn to_value(&self) -> CValue {
        let s = |v: &str| CValue::Str(v.to_string());
        let mut o = BTreeMap::new();
        o.insert("bytes".to_string(), CValue::U64(self.bytes));
        o.insert("command".to_string(), s(self.command.as_str()));
        o.insert("force_extension".to_string(), CValue::Bool(self.force_extension));
        o.insert("format".to_string(), s(self.format.as_str()));
        o.insert("lossy".to_string(), CValue::Arr(self.lossy.iter().map(|n| s(n.as_str())).collect()));
        o.insert("options".to_string(), self.options.clone());
        o.insert("overwrite".to_string(), CValue::Bool(self.overwrite));
        o.insert("principal".to_string(), s(self.principal.as_str()));
        o.insert("report_sha256".to_string(), s(self.report_sha256.as_str()));
        o.insert("sha256".to_string(), s(self.sha256.as_str()));
        o.insert("source".to_string(), self.source.to_value());
        o.insert("target".to_string(), CValue::Str(self.target.wire()));
        o.insert("writer_version".to_string(), s(self.writer_version.as_str()));
        CValue::Obj(o)
    }

    /// RFC 8785 canonical bytes of [`ExportEvent::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, ExportError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|_| ExportError::Internal("event is not canonical"))
    }

    /// Reads a record back strictly: bounded input in RFC 8785 canonical form (so one record
    /// has one byte form), no duplicate, unknown or missing keys, valid IDs and digests,
    /// `lossy` sorted, without repeats and at most [`MAX_LOSSY`] names.
    pub fn from_json(bytes: &[u8]) -> Result<ExportEvent, ExportError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| bad_event("not acceptable JSON"))?;
        match fid_canon::to_canonical_bytes(&v) {
            Ok(c) if c == bytes => {}
            _ => return Err(bad_event("not in canonical form")),
        }
        let CValue::Obj(o) = v else { return Err(bad_event("must be an object")) };
        if o.len() != EVENT_KEYS.len() || o.keys().any(|k| !EVENT_KEYS.contains(&k.as_str())) {
            return Err(bad_event("unknown or missing field"));
        }
        let command = CommandId::parse(str_field(&o, "command")?).map_err(|_| bad_event("invalid command ID"))?;
        let principal = principal_from_wire(str_field(&o, "principal")?).ok_or(bad_event("invalid principal"))?;
        let source = SourceRef::from_value(o.get("source").ok_or(bad_event("source"))?)
            .map_err(|_| bad_event("invalid source"))?;
        let target = Location::from_wire(str_field(&o, "target")?).map_err(|_| bad_event("invalid target"))?;
        let format = FormatId::new(str_field(&o, "format")?).map_err(|_| bad_event("invalid format"))?;
        let writer_version =
            VersionToken::new(str_field(&o, "writer_version")?).map_err(|_| bad_event("invalid writer version"))?;
        let options = match o.get("options") {
            Some(v @ CValue::Obj(_)) => v.clone(),
            _ => return Err(bad_event("options must be an object")),
        };
        let sha256 = Digest::parse(str_field(&o, "sha256")?).map_err(|_| bad_event("invalid sha256"))?;
        let report_sha256 =
            Digest::parse(str_field(&o, "report_sha256")?).map_err(|_| bad_event("invalid report_sha256"))?;
        let bytes = decimal_u64(str_field(&o, "bytes")?).ok_or(bad_event("bytes: not a decimal u64 string"))?;
        let Some(CValue::Arr(items)) = o.get("lossy") else { return Err(bad_event("lossy must be an array")) };
        if items.len() > MAX_LOSSY {
            return Err(bad_event("lossy: too many names"));
        }
        let mut lossy = BTreeSet::new();
        for item in items {
            let CValue::Str(s) = item else { return Err(bad_event("lossy: not a string")) };
            let name = TypeName::new(s).map_err(|_| bad_event("lossy: invalid name"))?;
            if lossy.last().is_some_and(|last| *last >= name) {
                return Err(bad_event("lossy: not sorted or repeated"));
            }
            lossy.insert(name);
        }
        Ok(ExportEvent {
            command,
            principal,
            source,
            target,
            format,
            writer_version,
            options,
            overwrite: bool_field(&o, "overwrite")?,
            force_extension: bool_field(&o, "force_extension")?,
            sha256,
            bytes,
            lossy,
            report_sha256,
        })
    }
}

// ---------------------------------------------------------------------------------------
// The skeleton

/// What an export runs against.
#[derive(Debug, Clone, Copy)]
pub struct ExportEnv<'a> {
    /// The format registry (export descriptors decide the extension check).
    pub registry: &'a Registry,
    /// The configured export roots.
    pub roots: &'a RootSet,
    /// Who is calling.
    pub principal: Principal,
    /// Set to cancel; checked before the first byte, on every write, after the writer and
    /// before placing.
    pub cancel: Option<&'a AtomicBool>,
    /// Largest file an export may produce, in bytes.
    pub max_bytes: u64,
    /// An injected sink fault (conformance kit and tests); `None` in production.
    pub fault: Option<Fault>,
}

impl<'a> ExportEnv<'a> {
    /// An environment with no cancel flag, no fault and the default size bound
    /// ([`DEFAULT_BYTES`]).
    pub fn new(registry: &'a Registry, roots: &'a RootSet, principal: Principal) -> ExportEnv<'a> {
        ExportEnv { registry, roots, principal, cancel: None, max_bytes: DEFAULT_BYTES, fault: None }
    }
}

/// The outcome of a successful export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportOutput {
    /// The command result.
    pub report: ExportReport,
    /// The journal record; `None` for a dry run, which commits nothing.
    pub event: Option<ExportEvent>,
}

/// A prepared export: bytes written to a synced temp file next to the target (none for a
/// dry run), report and record computed, target untouched. [`PreparedExport::place`] moves
/// the file into place; [`PreparedExport::discard`] or drop removes the temp file.
#[derive(Debug)]
pub struct PreparedExport {
    file: Option<AtomicWriter>,
    out: ExportOutput,
}

impl PreparedExport {
    /// The report the export will return.
    pub fn report(&self) -> &ExportReport {
        &self.out.report
    }

    /// The record to journal; `None` for a dry run.
    pub fn event(&self) -> Option<&ExportEvent> {
        self.out.event.as_ref()
    }

    /// Moves the file into place (nothing to do for a dry run). Fails `target_exists` if a
    /// target appeared since preparing, or `io`; the temp file is then removed.
    pub fn place(self) -> Result<ExportOutput, ExportError> {
        if let Some(w) = self.file {
            let r = w.commit()?;
            if r.sha256 != *self.out.report.sha256() || r.bytes != self.out.report.bytes() {
                return Err(ExportError::Internal("placed bytes differ from the prepared ones"));
            }
        }
        Ok(self.out)
    }

    /// Abandons the export: the temp file is removed, the target never touched.
    pub fn discard(self) {
        drop(self);
    }
}

/// How a run records and authorises: normally the caller's principal; on replay the
/// recorded one.
#[derive(Debug, Clone, Copy)]
struct Authority {
    force_allowed: bool,
    record_as: Principal,
}

/// The report location of a target text, as given (normal form for a handle).
fn location_of(target: &str) -> Result<Location, ExportError> {
    let loc = match RootHandle::parse(target) {
        Ok(h) => Location::handle(h),
        Err(_) => Location::human_path(target),
    };
    loc.map_err(|_: ReportError| ExportError::BadParams("target: not a representable location"))
}

/// REQ-004 on the resolved file name: the formats whose export descriptors declare its
/// extension must include `format`, unless a human forced it.
fn check_extension(reg: &Registry, format: &FormatId, name: &str, force: bool) -> Result<(), ExportError> {
    let Some(ext) = extension_of(name) else { return Ok(()) };
    let named: Vec<&FormatId> = reg
        .list(Direction::Export, OriginFilter::Any)
        .into_iter()
        .filter(|d| d.extensions().contains(&ext))
        .map(|d| d.id())
        .collect();
    if named.is_empty() || named.contains(&format) || force {
        Ok(())
    } else {
        Err(IoError::FormatMismatch.into())
    }
}

/// Canonical JSON round trip, so recorded options compare equal after the journal.
fn normalise(v: &CValue) -> Result<CValue, ExportError> {
    let bytes =
        fid_canon::to_canonical_bytes(v).map_err(|_| ExportError::BadOptions("options are not canonical JSON"))?;
    fid_canon::parse_with_limits(&bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
        .map_err(|_| ExportError::BadOptions("options are too large or too deep"))
}

fn write_guarded<W: Writer + ?Sized>(
    env: &ExportEnv<'_>,
    writer: &W,
    source: &W::Source,
    opts: &CValue,
    sink: &mut dyn ByteSink,
    report: &mut ReportBuilder,
) -> Result<(), ExportError> {
    let mut g = Guarded { inner: sink, cancel: env.cancel, fault: env.fault, cancelled: false, sink_error: None };
    let r = writer.write(source, opts, &mut g, report);
    if g.cancelled || is_set(env.cancel) {
        return Err(ExportError::Cancelled);
    }
    // A sink error wins even if the writer swallowed it.
    if let Some(e) = g.sink_error {
        return Err(ExportError::Io(e));
    }
    r.map_err(ExportError::Writer)
}

fn prepare_with<W: Writer + ?Sized>(
    env: &ExportEnv<'_>,
    auth: Authority,
    command: &CommandId,
    writer: &W,
    params: &ExportParams,
    source: &W::Source,
) -> Result<PreparedExport, ExportError> {
    if params.force_extension && !auth.force_allowed {
        return Err(ExportError::ForceExtensionHumanOnly);
    }
    let format = writer.format().clone();
    if params.format.as_ref().is_some_and(|f| *f != format) {
        return Err(ExportError::Internal("the writer does not write the requested format"));
    }
    if env.registry.get(&format, Direction::Export).is_none() {
        return Err(IoError::FormatUnknown.into());
    }
    let options = normalise(&writer.options(&params.options).map_err(ExportError::BadOptions)?)?;
    if is_set(env.cancel) {
        return Err(ExportError::Cancelled);
    }

    let target = env.roots.resolve_export(env.principal, &params.target)?;
    let location = location_of(&params.target)?;
    let name = target.name().to_str().ok_or(ExportError::BadParams("target: the file name is not UTF-8"))?;
    if target.root().is_none() && !plain_name(name) {
        // A human's raw path: the same component rule as a handle's.
        return Err(IoError::PathOutsideRoot.into());
    }
    check_extension(env.registry, &format, name, params.force_extension)?;

    let wopts = WriteOptions { overwrite: params.overwrite, max_bytes: env.max_bytes };
    AtomicWriter::check(&target, wopts).map_err(|e| match e {
        IoError::Io { kind: io::ErrorKind::InvalidInput | io::ErrorKind::PermissionDenied, .. } => {
            ExportError::TargetProtected(e)
        }
        e => ExportError::Io(e),
    })?;

    let mut report = ReportBuilder::new();
    let (receipt, file) = if params.dry_run {
        drop(target);
        let mut sink = CountingSink::new(env.max_bytes);
        write_guarded(env, writer, source, &options, &mut sink, &mut report)?;
        (sink.finish()?, None)
    } else {
        // On every early return below, dropping `sink` removes its temp file.
        let mut sink = AtomicWriter::create(target, wopts)?;
        write_guarded(env, writer, source, &options, &mut sink, &mut report)?;
        let r = sink.prepare()?;
        (r, Some(sink))
    };

    let report = report.finish_export(format.clone(), location.clone(), receipt.sha256, receipt.bytes, params.dry_run);
    let report_sha256 = report.digest().map_err(|_| ExportError::Internal("report is not canonical"))?;
    let event = if params.dry_run {
        None
    } else {
        Some(ExportEvent {
            command: command.clone(),
            principal: auth.record_as,
            source: params.source.clone(),
            target: location,
            format,
            writer_version: writer.version().clone(),
            options,
            overwrite: params.overwrite,
            force_extension: params.force_extension,
            sha256: receipt.sha256,
            bytes: receipt.bytes,
            lossy: report.lossy().clone(),
            report_sha256,
        })
    };
    Ok(PreparedExport { file, out: ExportOutput { report, event } })
}

fn caller(env: &ExportEnv<'_>) -> Authority {
    Authority { force_allowed: env.principal == Principal::Human, record_as: env.principal }
}

/// Phase one of the skeleton (see the module documentation): everything but placing the
/// file. Nothing at the target changes.
pub fn prepare_export<W: Writer + ?Sized>(
    env: &ExportEnv<'_>,
    command: &CommandId,
    writer: &W,
    params: &ExportParams,
    source: &W::Source,
) -> Result<PreparedExport, ExportError> {
    prepare_with(env, caller(env), command, writer, params, source)
}

fn run_with<W: Writer + ?Sized>(
    env: &ExportEnv<'_>,
    auth: Authority,
    command: &CommandId,
    writer: &W,
    params: &ExportParams,
    source: &W::Source,
) -> Result<ExportOutput, ExportError> {
    let prepared = prepare_with(env, auth, command, writer, params, source)?;
    if is_set(env.cancel) {
        prepared.discard();
        return Err(ExportError::Cancelled);
    }
    prepared.place()
}

/// The whole skeleton without a journal in between: [`prepare_export`], a last cancel
/// check, then [`PreparedExport::place`].
pub fn run_export<W: Writer + ?Sized>(
    env: &ExportEnv<'_>,
    command: &CommandId,
    writer: &W,
    params: &ExportParams,
    source: &W::Source,
) -> Result<ExportOutput, ExportError> {
    run_with(env, caller(env), command, writer, params, source)
}

/// Exact-mode replay of a recorded export: the same command, writer and parameters
/// (`overwrite` and `force_extension` included) run again against `env` (an empty export
/// root for AC-6). A recorded `force_extension` is honoured only when the recorded principal
/// is a human. `IDENTICAL` when the new record equals the recorded one (same bytes, digest,
/// lossy list, writer version and report), `DIVERGENT` otherwise; `NOT_REPLAYABLE` for
/// another command, another format, a human's raw path, or a forced extension recorded for
/// a principal that may not force it.
pub fn replay_export<W: Writer + ?Sized>(
    env: &ExportEnv<'_>,
    command: &CommandId,
    writer: &W,
    with_format: bool,
    event: &ExportEvent,
    source: &W::Source,
) -> Result<ReplayOutcome, ExportError> {
    if event.command != *command || event.format != *writer.format() {
        return Ok(ReplayOutcome::NotReplayable);
    }
    if event.force_extension && event.principal != Principal::Human {
        return Ok(ReplayOutcome::NotReplayable);
    }
    let Some(handle) = event.target.as_handle() else { return Ok(ReplayOutcome::NotReplayable) };
    let mut params = ExportParams::new(
        event.source.clone(),
        if with_format { Some(event.format.clone()) } else { None },
        &handle.to_string(),
    );
    params.options = event.options.clone();
    params.overwrite = event.overwrite;
    params.force_extension = event.force_extension;
    let auth = Authority { force_allowed: event.principal == Principal::Human, record_as: event.principal };
    let out = run_with(env, auth, command, writer, &params, source)?;
    Ok(match out.event {
        Some(e) if e == *event => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    })
}

/// The command ID `export.<format>@1` of a per-format command: the format ID with `-`
/// written as `_`. One-to-one, because format IDs hold no `_`. Fails `command_id_taken` for a
/// reserved name ([`RESERVED_EXPORT_NAMES`]) or one in `taken` (the command IDs or names
/// already registered, compared without `@major`), and `bad_params` for a format ID that
/// does not end in a letter or digit.
pub fn format_command_id(format: &FormatId, taken: &[&str]) -> Result<CommandId, ExportError> {
    let name = format!("export.{}", format.as_str().replace('-', "_"));
    let base = |s: &str| s.split('@').next().unwrap_or(s).to_string();
    if RESERVED_EXPORT_NAMES.contains(&name.as_str()) || taken.iter().any(|t| base(t) == name) {
        return Err(ExportError::CommandIdTaken);
    }
    CommandId::parse(&format!("{name}@1")).map_err(|_| ExportError::BadParams("format: no valid command ID"))
}

fn export_file_id() -> Result<CommandId, ExportError> {
    CommandId::parse(EXPORT_FILE).map_err(|_| ExportError::Internal("export.file@1 is not a command ID"))
}

/// An export command as the conformance kit and the bus adapter see it.
pub trait ExportRun<S: ?Sized> {
    /// The command ID.
    fn command_id(&self) -> Result<CommandId, ExportError>;
    /// Whether its params take `format` (`export.file@1`).
    fn takes_format(&self) -> bool;
    /// Phase one ([`prepare_export`]).
    fn prepare(&self, env: &ExportEnv<'_>, params: &ExportParams, source: &S) -> Result<PreparedExport, ExportError>;
    /// The whole command ([`run_export`]).
    fn run(&self, env: &ExportEnv<'_>, params: &ExportParams, source: &S) -> Result<ExportOutput, ExportError>;
    /// Exact-mode replay ([`replay_export`]).
    fn replay(&self, env: &ExportEnv<'_>, event: &ExportEvent, source: &S) -> Result<ReplayOutcome, ExportError>;
}

/// `export.file@1`: one skeleton, the writer picked by the `format` parameter.
pub struct ExportFile<'w, S: ?Sized> {
    writers: BTreeMap<FormatId, Box<dyn Writer<Source = S> + 'w>>,
}

impl<S: ?Sized> fmt::Debug for ExportFile<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExportFile").field("formats", &self.writers.keys().collect::<Vec<_>>()).finish()
    }
}

impl<S: ?Sized> Default for ExportFile<'_, S> {
    fn default() -> Self {
        ExportFile { writers: BTreeMap::new() }
    }
}

impl<'w, S: ?Sized> ExportFile<'w, S> {
    /// No writers.
    pub fn new() -> Self {
        ExportFile::default()
    }

    /// Adds a writer; a second writer for the same format fails `format_duplicate`.
    pub fn add(&mut self, writer: Box<dyn Writer<Source = S> + 'w>) -> Result<(), ExportError> {
        let id = writer.format().clone();
        if self.writers.contains_key(&id) {
            return Err(IoError::FormatDuplicate.into());
        }
        self.writers.insert(id, writer);
        Ok(())
    }

    /// The formats with a writer, sorted.
    pub fn formats(&self) -> impl Iterator<Item = &FormatId> {
        self.writers.keys()
    }

    fn writer(&self, format: Option<&FormatId>) -> Result<&(dyn Writer<Source = S> + 'w), ExportError> {
        let format = format.ok_or(ExportError::BadParams("format is required"))?;
        self.writers.get(format).map(|w| w.as_ref()).ok_or(ExportError::Io(IoError::FormatUnknown))
    }

    /// Reads the params object, then runs the command.
    pub fn run_value(&self, env: &ExportEnv<'_>, params: &CValue, source: &S) -> Result<ExportOutput, ExportError> {
        ExportRun::run(self, env, &ExportParams::from_value(params, true)?, source)
    }
}

impl<S: ?Sized> ExportRun<S> for ExportFile<'_, S> {
    fn command_id(&self) -> Result<CommandId, ExportError> {
        export_file_id()
    }

    fn takes_format(&self) -> bool {
        true
    }

    fn prepare(&self, env: &ExportEnv<'_>, params: &ExportParams, source: &S) -> Result<PreparedExport, ExportError> {
        let writer = self.writer(params.format.as_ref())?;
        prepare_export(env, &export_file_id()?, writer, params, source)
    }

    fn run(&self, env: &ExportEnv<'_>, params: &ExportParams, source: &S) -> Result<ExportOutput, ExportError> {
        let writer = self.writer(params.format.as_ref())?;
        run_export(env, &export_file_id()?, writer, params, source)
    }

    fn replay(&self, env: &ExportEnv<'_>, event: &ExportEvent, source: &S) -> Result<ReplayOutcome, ExportError> {
        match self.writers.get(&event.format) {
            Some(w) => replay_export(env, &export_file_id()?, w.as_ref(), true, event, source),
            None => Ok(ReplayOutcome::NotReplayable),
        }
    }
}

/// `export.<format>@1`: the skeleton with one fixed writer.
#[derive(Debug)]
pub struct ExportCommand<W> {
    writer: W,
    id: CommandId,
}

impl<W: Writer> ExportCommand<W> {
    /// The command for `writer`, named by [`format_command_id`]; `taken` lists the command
    /// IDs already in the registry.
    pub fn new(writer: W, taken: &[&str]) -> Result<ExportCommand<W>, ExportError> {
        let id = format_command_id(writer.format(), taken)?;
        Ok(ExportCommand { writer, id })
    }

    /// The command ID.
    pub fn id(&self) -> &CommandId {
        &self.id
    }

    /// The writer.
    pub fn writer(&self) -> &W {
        &self.writer
    }

    fn check_format(&self, params: &ExportParams) -> Result<(), ExportError> {
        if params.format.as_ref().is_some_and(|f| f != self.writer.format()) {
            return Err(ExportError::BadParams("format is fixed by the command"));
        }
        Ok(())
    }

    /// Reads the params object (no `format` field), then runs the command.
    pub fn run_value(
        &self,
        env: &ExportEnv<'_>,
        params: &CValue,
        source: &W::Source,
    ) -> Result<ExportOutput, ExportError> {
        ExportRun::run(self, env, &ExportParams::from_value(params, false)?, source)
    }
}

impl<W: Writer> ExportRun<W::Source> for ExportCommand<W> {
    fn command_id(&self) -> Result<CommandId, ExportError> {
        Ok(self.id.clone())
    }

    fn takes_format(&self) -> bool {
        false
    }

    fn prepare(
        &self,
        env: &ExportEnv<'_>,
        params: &ExportParams,
        source: &W::Source,
    ) -> Result<PreparedExport, ExportError> {
        self.check_format(params)?;
        prepare_export(env, &self.id, &self.writer, params, source)
    }

    fn run(&self, env: &ExportEnv<'_>, params: &ExportParams, source: &W::Source) -> Result<ExportOutput, ExportError> {
        self.check_format(params)?;
        run_export(env, &self.id, &self.writer, params, source)
    }

    fn replay(
        &self,
        env: &ExportEnv<'_>,
        event: &ExportEvent,
        source: &W::Source,
    ) -> Result<ReplayOutcome, ExportError> {
        replay_export(env, &self.id, &self.writer, false, event, source)
    }
}
