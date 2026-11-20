//! The generic export command `export.file@1` and the export skeleton every
//! `export.<format>@1` command is built from (spec node `IO.REG.ENV.EXPORT`,
//! REQ-IO.REG.ENV-005).
//!
//! A format writer implements [`Writer`]: entities and options in, bytes out to a
//! [`ByteSink`], lossy items reported to a [`ReportBuilder`]. The skeleton runs every export
//! the same way, whichever command started it:
//!
//! 1. `force_extension` from any principal but a human is refused (`policy/human_only`);
//! 2. the writer's format must be registered for export in the format [`Registry`]
//!    (`format_unknown`), and the writer checks the options and fills in their defaults;
//! 3. the target is resolved through [`RootSet::resolve_export`] (`IO.REG.ROOTS`): agents and
//!    scripts pass `root://` handles only (`raw_path_forbidden`), and the path stays inside
//!    the root, links included (`path_outside_root`);
//! 4. the target's extension is cross-checked against the registry (`IO.REG.DETECT`): an
//!    extension that some export format declares, but not the chosen one, fails
//!    `format_mismatch` unless a human set `force_extension` (REQ-004). An extension no
//!    export format declares, or none, is not this check's business;
//! 5. the bytes go to an [`AtomicWriter`] (`IO.REG.WRITE`: no clobbering, all or nothing), or,
//!    for `dry_run`, to a [`CountingSink`] after the same target checks, so no file is
//!    created, temporary or final (REQ-002);
//! 6. a writer error, a cancel (checked before the first byte, on every write and before the
//!    commit) or a failed commit drops the atomic writer, which removes its temp file; the
//!    error is returned and no [`ExportEvent`] exists (REQ-003);
//! 7. on success the [`ExportReport`] is finished and, unless it was a dry run, the
//!    [`ExportEvent`] for the journal is built: command, source, target handle, format and
//!    writer version, effective options, output SHA-256 and byte count, lossy list and the
//!    report digest (REQ-001).
//!
//! Every refusal maps to a `code/reason` pair of the bus error catalogue
//! ([`ExportError::bus_code`]). Writing into a local export root is W1 with
//! `fs_effect: write_root` (CONSTITUTION §3.1); nothing here sends data off the machine.
//! Bytes and digest are D0.

pub mod command;
pub mod conformance;

use crate::detect::{extension_of, OriginFilter, Registry};
use crate::error::{IoError, Position};
use crate::limits::{LimitKind, Principal, DEFAULT_BYTES};
use crate::prov::VersionToken;
use crate::report::{ExportReport, FormatId, Location, ReportBuilder, ReportError, TypeName};
use crate::roots::{Direction, RootHandle, RootSet};
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

/// Wraps the real sink: every write first checks the cancel flag.
struct Guarded<'a> {
    inner: &'a mut dyn ByteSink,
    cancel: Option<&'a AtomicBool>,
    cancelled: bool,
}

impl ByteSink for Guarded<'_> {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        if self.cancelled || is_set(self.cancel) {
            self.cancelled = true;
            return Err(IoError::Io {
                kind: io::ErrorKind::Interrupted,
                at: Position::byte(self.inner.bytes_written()),
            });
        }
        self.inner.write_bytes(buf)
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
    /// lossy names to `report`. Must be deterministic (D0) for the same inputs.
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
    /// A reader-harness error: root resolution, the extension check, the atomic writer or
    /// the format writer.
    Io(IoError),
    /// The export was cancelled.
    Cancelled,
    /// `force_extension` was set by a principal that is not a human.
    ForceExtensionHumanOnly,
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
    /// The leaf's own stable code: the [`IoError`] code, or `cancelled`,
    /// `force_extension_human_only`, `bad_params`, `bad_options`, `bad_event`, `internal`.
    pub fn code(&self) -> &'static str {
        match self {
            ExportError::Io(e) => e.code(),
            ExportError::Cancelled => "cancelled",
            ExportError::ForceExtensionHumanOnly => "force_extension_human_only",
            ExportError::BadParams(_) => "bad_params",
            ExportError::BadOptions(_) => "bad_options",
            ExportError::BadEvent(_) => "bad_event",
            ExportError::Internal(_) => "internal",
        }
    }

    /// The bus error `(code, reason)` this failure is reported as (catalogue v3 of
    /// `bus-errors.json`, CORE.BUS.PIPE.ERR). Refusals (the caller can fix them) are
    /// `validation`, `policy`, `precondition` or `cancelled`; only environment faults (a
    /// file-system error other than a missing folder or a protected target) and bugs (a
    /// writer emitting a reader error, a crashed plugin, a broken invariant) are `internal`.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            ExportError::Io(e) => io_bus_code(e),
            ExportError::Cancelled => ("cancelled", "requested"),
            ExportError::ForceExtensionHumanOnly => ("policy", "human_only"),
            ExportError::BadParams(_) | ExportError::BadOptions(_) | ExportError::BadEvent(_) => {
                ("validation", "schema")
            }
            ExportError::Internal(_) => ("internal", "invariant_violated"),
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
        IoError::Io { kind: io::ErrorKind::InvalidInput | io::ErrorKind::PermissionDenied, .. } => {
            ("policy", "target_protected")
        }
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
}

// ---------------------------------------------------------------------------------------
// Journal event

/// The journal record of one committed export (`export.file@1` or `export.<format>@1`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExportEvent {
    /// The command that ran.
    pub command: CommandId,
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
    /// SHA-256 of the file written.
    pub sha256: Digest,
    /// Its size in bytes.
    pub bytes: u64,
    /// Names the format could not represent, sorted.
    pub lossy: BTreeSet<TypeName>,
    /// SHA-256 of the canonical [`ExportReport`] (the result digest, CONSTITUTION §3).
    pub report_sha256: Digest,
}

const EVENT_KEYS: [&str; 10] =
    ["bytes", "command", "format", "lossy", "options", "report_sha256", "sha256", "source", "target", "writer_version"];

fn bad_event(reason: &'static str) -> ExportError {
    ExportError::BadEvent(reason)
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, ExportError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(bad_event("a field is missing or not a string")),
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

impl ExportEvent {
    /// The record as JSON; `bytes` is a decimal string (the `fid-canon` 64-bit rule).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("bytes".to_string(), CValue::U64(self.bytes));
        o.insert("command".to_string(), CValue::Str(self.command.as_str().to_string()));
        o.insert("format".to_string(), CValue::Str(self.format.as_str().to_string()));
        o.insert(
            "lossy".to_string(),
            CValue::Arr(self.lossy.iter().map(|n| CValue::Str(n.as_str().to_string())).collect()),
        );
        o.insert("options".to_string(), self.options.clone());
        o.insert("report_sha256".to_string(), CValue::Str(self.report_sha256.as_str().to_string()));
        o.insert("sha256".to_string(), CValue::Str(self.sha256.as_str().to_string()));
        o.insert("source".to_string(), self.source.to_value());
        o.insert("target".to_string(), CValue::Str(self.target.wire()));
        o.insert("writer_version".to_string(), CValue::Str(self.writer_version.as_str().to_string()));
        CValue::Obj(o)
    }

    /// RFC 8785 canonical bytes of [`ExportEvent::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, ExportError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|_| ExportError::Internal("event is not canonical"))
    }

    /// Reads a record back strictly: bounded input, no duplicate, unknown or missing keys,
    /// valid IDs and digests, `lossy` sorted without repeats.
    pub fn from_json(bytes: &[u8]) -> Result<ExportEvent, ExportError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
            .map_err(|_| bad_event("not acceptable JSON"))?;
        let CValue::Obj(o) = v else { return Err(bad_event("must be an object")) };
        if o.len() != EVENT_KEYS.len() || o.keys().any(|k| !EVENT_KEYS.contains(&k.as_str())) {
            return Err(bad_event("unknown or missing field"));
        }
        let command = CommandId::parse(str_field(&o, "command")?).map_err(|_| bad_event("invalid command ID"))?;
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
            source,
            target,
            format,
            writer_version,
            options,
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
    /// Set to cancel; checked before the first byte, on every write and before the commit.
    pub cancel: Option<&'a AtomicBool>,
    /// Largest file an export may produce, in bytes.
    pub max_bytes: u64,
}

impl<'a> ExportEnv<'a> {
    /// An environment with no cancel flag and the default size bound ([`DEFAULT_BYTES`]).
    pub fn new(registry: &'a Registry, roots: &'a RootSet, principal: Principal) -> ExportEnv<'a> {
        ExportEnv { registry, roots, principal, cancel: None, max_bytes: DEFAULT_BYTES }
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

/// The report location of a target text, as given (normal form for a handle).
fn location_of(target: &str) -> Result<Location, ExportError> {
    let loc = match RootHandle::parse(target) {
        Ok(h) => Location::handle(h),
        Err(_) => Location::human_path(target),
    };
    loc.map_err(|_: ReportError| ExportError::BadParams("target: not a representable location"))
}

/// REQ-004: the formats whose export descriptors declare the target's extension must
/// include `format`, unless a human forced it.
fn check_extension(reg: &Registry, format: &FormatId, target: &str, force: bool) -> Result<(), ExportError> {
    let Some(ext) = extension_of(target) else { return Ok(()) };
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
    let mut g = Guarded { inner: sink, cancel: env.cancel, cancelled: false };
    let r = writer.write(source, opts, &mut g, report);
    if g.cancelled || is_set(env.cancel) {
        return Err(ExportError::Cancelled);
    }
    r.map_err(ExportError::Io)
}

/// The export skeleton shared by `export.file@1` and every `export.<format>@1`.
pub fn run_export<W: Writer + ?Sized>(
    env: &ExportEnv<'_>,
    command: &CommandId,
    writer: &W,
    params: &ExportParams,
    source: &W::Source,
) -> Result<ExportOutput, ExportError> {
    if params.force_extension && env.principal != Principal::Human {
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
    check_extension(env.registry, &format, &params.target, params.force_extension)?;

    let wopts = WriteOptions { overwrite: params.overwrite, max_bytes: env.max_bytes };
    let mut report = ReportBuilder::new();
    let receipt = if params.dry_run {
        AtomicWriter::check(&target, wopts)?;
        drop(target);
        let mut sink = CountingSink::new(env.max_bytes);
        write_guarded(env, writer, source, &options, &mut sink, &mut report)?;
        sink.finish()?
    } else {
        // On every early return below, dropping `sink` removes its temp file.
        let mut sink = AtomicWriter::create(target, wopts)?;
        write_guarded(env, writer, source, &options, &mut sink, &mut report)?;
        if is_set(env.cancel) {
            sink.cancel();
            return Err(ExportError::Cancelled);
        }
        sink.commit()?
    };

    let report = report.finish_export(format.clone(), location.clone(), receipt.sha256, receipt.bytes, params.dry_run);
    if params.dry_run {
        return Ok(ExportOutput { report, event: None });
    }
    let report_sha256 = report.digest().map_err(|_| ExportError::Internal("report is not canonical"))?;
    let event = ExportEvent {
        command: command.clone(),
        source: params.source.clone(),
        target: location,
        format,
        writer_version: writer.version().clone(),
        options,
        sha256: receipt.sha256,
        bytes: receipt.bytes,
        lossy: report.lossy().clone(),
        report_sha256,
    };
    Ok(ExportOutput { report, event: Some(event) })
}

/// Exact-mode replay of a recorded export: the same command, writer and parameters run again
/// against `env` (an empty export root for AC-6). `IDENTICAL` when the new record equals the
/// recorded one (same bytes, digest, lossy list, writer version and report), `DIVERGENT`
/// otherwise; `NOT_REPLAYABLE` for another command, another format or a human's raw path.
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
    let Some(handle) = event.target.as_handle() else { return Ok(ReplayOutcome::NotReplayable) };
    let mut params = ExportParams::new(
        event.source.clone(),
        if with_format { Some(event.format.clone()) } else { None },
        &handle.to_string(),
    );
    params.options = event.options.clone();
    let out = run_export(env, command, writer, &params, source)?;
    Ok(match out.event {
        Some(e) if e == *event => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    })
}

/// The command ID `export.<format>@1` of a per-format command: the format ID with `-`
/// written as `_` (command names allow `a-z0-9_.`). Fails for a format ID that does not
/// end in a letter or digit.
pub fn format_command_id(format: &FormatId) -> Result<CommandId, ExportError> {
    let name = format.as_str().replace('-', "_");
    CommandId::parse(&format!("export.{name}@1")).map_err(|_| ExportError::BadParams("format: no valid command ID"))
}

fn export_file_id() -> Result<CommandId, ExportError> {
    CommandId::parse(EXPORT_FILE).map_err(|_| ExportError::Internal("export.file@1 is not a command ID"))
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

    /// Runs `export.file@1`.
    pub fn run(&self, env: &ExportEnv<'_>, params: &ExportParams, source: &S) -> Result<ExportOutput, ExportError> {
        let writer = self.writer(params.format.as_ref())?;
        run_export(env, &export_file_id()?, writer, params, source)
    }

    /// Reads the params object, then [`ExportFile::run`].
    pub fn run_value(&self, env: &ExportEnv<'_>, params: &CValue, source: &S) -> Result<ExportOutput, ExportError> {
        self.run(env, &ExportParams::from_value(params, true)?, source)
    }

    /// Exact-mode replay of an `export.file@1` record ([`replay_export`]).
    pub fn replay(&self, env: &ExportEnv<'_>, event: &ExportEvent, source: &S) -> Result<ReplayOutcome, ExportError> {
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
    /// The command for `writer`, named by [`format_command_id`].
    pub fn new(writer: W) -> Result<ExportCommand<W>, ExportError> {
        let id = format_command_id(writer.format())?;
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

    /// Runs the command. `params.format` must be `None` or the writer's format.
    pub fn run(
        &self,
        env: &ExportEnv<'_>,
        params: &ExportParams,
        source: &W::Source,
    ) -> Result<ExportOutput, ExportError> {
        if params.format.as_ref().is_some_and(|f| f != self.writer.format()) {
            return Err(ExportError::BadParams("format is fixed by the command"));
        }
        run_export(env, &self.id, &self.writer, params, source)
    }

    /// Reads the params object (no `format` field), then [`ExportCommand::run`].
    pub fn run_value(
        &self,
        env: &ExportEnv<'_>,
        params: &CValue,
        source: &W::Source,
    ) -> Result<ExportOutput, ExportError> {
        self.run(env, &ExportParams::from_value(params, false)?, source)
    }

    /// Exact-mode replay of a record of this command ([`replay_export`]).
    pub fn replay(
        &self,
        env: &ExportEnv<'_>,
        event: &ExportEvent,
        source: &W::Source,
    ) -> Result<ReplayOutcome, ExportError> {
        replay_export(env, &self.id, &self.writer, false, event, source)
    }
}
