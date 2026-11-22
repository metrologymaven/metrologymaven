//! Report-file export envelope (spec node `IO.RES.REPORT`), the library behind
//! `export.results_report@1` (W1, `fs_effect: write_root`, D0; CLI
//! `fid export results-report`, MCP `export_results_report`).
//!
//! IO owns only the approved-root file write, digest verification and journaling. RPT
//! owns report content, layout and rendering; this module never inspects, re-renders or
//! modifies report bytes.

use fid_canon::CValue;
use fid_io_reg::{
    AtomicWriter, ExportReport, FormatId, IoError, Location, Position, Principal, ReportBuilder, ReportError,
    RootHandle, RootSet, UntrustedText, WriteOptions, WriteReceipt,
};
use fid_registry::CommandDef;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as Sha2Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{Cursor, Read};

/// Registry command identifier (CONSTITUTION §3).
pub const COMMAND: &str = "export.results_report@1";
/// Export format ID reported in [`ExportReport`].
pub const FORMAT_ID: &str = "results-report";
/// Fixed chunk size used to stream report bytes unchanged into [`AtomicWriter`].
pub const STREAM_CHUNK_BYTES: usize = 64 * 1024;
/// Maximum canonical JSON event size accepted by [`ReportExportEvent::from_json`].
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
/// Maximum JSON nesting depth accepted by [`ReportExportEvent::from_json`].
pub const MAX_EVENT_DEPTH: usize = 4;
/// Maximum target handle byte length in the command schema.
pub const MAX_TARGET_BYTES: usize = 4096;
/// Maximum media-type byte length accepted from blob metadata.
pub const MAX_MEDIA_TYPE_BYTES: usize = 256;

const LIFECYCLE_SINCE: &str = "2026-11-22";
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";
const U64_PATTERN: &str = "^(0|[1-9][0-9]{0,18}|1[0-7][0-9]{18}|18[0-3][0-9]{17}|184[0-3][0-9]{16}|1844[0-5][0-9]{15}|18446[0-6][0-9]{14}|184467[0-3][0-9]{13}|1844674[0-3][0-9]{12}|184467440[0-6][0-9]{10}|1844674407[0-2][0-9]{9}|18446744073[0-6][0-9]{8}|1844674407370[0-8][0-9]{6}|18446744073709[0-4][0-9]{5}|184467440737095[0-4][0-9]{4}|18446744073709550[0-9]{3}|18446744073709551[0-5][0-9]{2}|1844674407370955160[0-9]|1844674407370955161[0-4]|18446744073709551615)$";
const OOXML_PREFIX: &str = "application/vnd.openxmlformats-officedocument.";

/// Errors returned by [`export_results_report`] and [`replay_results_report`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportExportError {
    /// `report_not_found`: no blob with this digest exists in the store.
    ReportNotFound {
        /// The requested blob digest.
        digest: Digest,
    },
    /// `media_type_not_report`: the blob's media type is not on the report allowlist.
    MediaTypeNotReport {
        /// The rejected media type string.
        media_type: String,
    },
    /// `digest_mismatch`: the streamed blob bytes did not hash to the requested digest.
    DigestMismatch {
        /// The requested digest.
        expected: Box<Digest>,
        /// The actual SHA-256 of the streamed bytes.
        actual: Box<Digest>,
    },
    /// File-system, root-guard or overwrite error (`target_exists`, `raw_path_forbidden`,
    /// `path_outside_root`, `root_not_found`, `root_read_only`, `io`, etc.).
    Io(IoError),
    /// Report builder error.
    Report(ReportError),
    /// `event_invalid`: a journal event failed strict decoding or canonicalisation.
    Event {
        /// Static reason code.
        reason: &'static str,
    },
    /// `internal`: an internal schema or digest invariant failed.
    Internal(&'static str),
}

impl ReportExportError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ReportExportError::ReportNotFound { .. } => "report_not_found",
            ReportExportError::MediaTypeNotReport { .. } => "media_type_not_report",
            ReportExportError::DigestMismatch { .. } => "digest_mismatch",
            ReportExportError::Io(e) => e.code(),
            ReportExportError::Report(e) => e.code(),
            ReportExportError::Event { .. } => "event_invalid",
            ReportExportError::Internal(_) => "internal",
        }
    }

    /// Static reason code, if any.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            ReportExportError::Event { reason } | ReportExportError::Internal(reason) => Some(reason),
            _ => None,
        }
    }
}

impl fmt::Display for ReportExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportExportError::ReportNotFound { digest } => {
                write!(f, "report_not_found: {}", digest.as_str())
            }
            ReportExportError::MediaTypeNotReport { media_type } => {
                write!(f, "media_type_not_report: {}", UntrustedText::new(media_type).as_display())
            }
            ReportExportError::DigestMismatch { expected, actual } => {
                write!(f, "digest_mismatch: expected {}, got {}", expected.as_str(), actual.as_str())
            }
            ReportExportError::Io(e) => write!(f, "{e}"),
            ReportExportError::Report(e) => write!(f, "{e}"),
            ReportExportError::Event { reason } => write!(f, "event_invalid: {reason}"),
            ReportExportError::Internal(reason) => write!(f, "internal: {reason}"),
        }
    }
}

impl std::error::Error for ReportExportError {}

impl From<IoError> for ReportExportError {
    fn from(e: IoError) -> Self {
        ReportExportError::Io(e)
    }
}

impl From<ReportError> for ReportExportError {
    fn from(e: ReportError) -> Self {
        ReportExportError::Report(e)
    }
}

/// Returns `true` if `media_type` is an allowed rendered-report media type:
/// `application/pdf`, `text/html`, or `application/vnd.openxmlformats-officedocument.*`.
pub fn is_report_media_type(media_type: &str) -> bool {
    normalize_report_media_type(media_type).is_ok()
}

/// Validates and normalises a report media type to lowercase ASCII `type/subtype`.
///
/// Rejects empty strings, parameters (`;`), whitespace, control characters and any media
/// type outside the allowlist with [`ReportExportError::MediaTypeNotReport`].
pub fn normalize_report_media_type(raw: &str) -> Result<String, ReportExportError> {
    let bad = || ReportExportError::MediaTypeNotReport { media_type: raw.to_owned() };
    if raw.is_empty() || raw.len() > MAX_MEDIA_TYPE_BYTES || !raw.is_ascii() {
        return Err(bad());
    }
    if raw.bytes().any(|b| b <= 0x20 || b >= 0x7f || b == b';') {
        return Err(bad());
    }
    let lower = raw.to_ascii_lowercase();
    if lower == "application/pdf" || lower == "text/html" {
        return Ok(lower);
    }
    if let Some(suffix) = lower.strip_prefix(OOXML_PREFIX) {
        if !suffix.is_empty()
            && !suffix.starts_with('.')
            && !suffix.ends_with('.')
            && !suffix.contains("..")
            && suffix.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        {
            return Ok(lower);
        }
    }
    Err(bad())
}

/// An open report blob stream returned by a [`ReportBlobStore`].
pub struct ReportStream<'a> {
    /// Stored media type of the rendered report blob.
    pub media_type: String,
    /// Reader yielding the raw rendered bytes unchanged.
    pub reader: Box<dyn Read + 'a>,
}

/// Read-only view of the project's rendered-report blob store (CORE.STO.CAS + RPT metadata).
pub trait ReportBlobStore {
    /// Opens the rendered report blob for `digest`, or returns `Ok(None)` if absent.
    fn open_report(&self, digest: &Digest) -> Result<Option<ReportStream<'_>>, ReportExportError>;
}

/// Deterministic in-memory [`ReportBlobStore`] for tests and offline verification.
#[derive(Debug, Default, Clone)]
pub struct MemoryReportStore {
    blobs: BTreeMap<Digest, (String, Vec<u8>)>,
}

impl MemoryReportStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `bytes` under their SHA-256 digest and returns that digest.
    pub fn insert(&mut self, media_type: impl Into<String>, bytes: Vec<u8>) -> Digest {
        let digest = sha256_digest(&bytes);
        self.blobs.insert(digest, (media_type.into(), bytes));
        digest
    }

    /// Stores `bytes` under an explicit `digest` (useful for corruption / mismatch tests).
    pub fn insert_under_digest(&mut self, digest: Digest, media_type: impl Into<String>, bytes: Vec<u8>) {
        self.blobs.insert(digest, (media_type.into(), bytes));
    }

    /// Returns `true` if `digest` is present in the store.
    pub fn contains(&self, digest: &Digest) -> bool {
        self.blobs.contains_key(digest)
    }
}

impl ReportBlobStore for MemoryReportStore {
    fn open_report(&self, digest: &Digest) -> Result<Option<ReportStream<'_>>, ReportExportError> {
        Ok(self.blobs.get(digest).map(|(media_type, bytes)| ReportStream {
            media_type: media_type.clone(),
            reader: Box::new(Cursor::new(bytes.as_slice())),
        }))
    }
}

/// Parameters for `export.results_report@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportExportRequest<'a> {
    /// SHA-256 digest of the RPT-rendered report blob.
    pub report: Digest,
    /// Target path or `root://<id>/<rel>` handle.
    pub target: &'a str,
    /// Replace an existing target file if `true`; fail `target_exists` if `false`.
    pub overwrite: bool,
    /// Validate and hash without writing to disk.
    pub dry_run: bool,
}

/// Command result payload `{ target, sha256, bytes, media_type }` (IO.RES.REPORT Design).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportExportResult {
    /// Target handle (or raw path for human runs).
    pub target: String,
    /// SHA-256 digest of the written (or would-be) report bytes.
    pub sha256: Digest,
    /// Byte length of the written (or would-be) report file.
    pub bytes: u64,
    /// Normalised report media type.
    pub media_type: String,
}

impl ReportExportResult {
    /// Encodes the result as a canonical JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("bytes".to_owned(), CValue::U64(self.bytes));
        m.insert("media_type".to_owned(), CValue::Str(self.media_type.clone()));
        m.insert("sha256".to_owned(), CValue::Str(self.sha256.as_str().to_owned()));
        m.insert("target".to_owned(), CValue::Str(self.target.clone()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ReportExportError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| ReportExportError::Internal("canonical_json"))
    }

    /// SHA-256 digest of [`Self::canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, ReportExportError> {
        Ok(sha256_digest(&self.canonical_bytes()?))
    }
}

/// Journal event for `export.results_report@1` (REQ-IO.RES.REPORT-003).
///
/// Records both the source blob SHA-256 (`report`) and the written file's SHA-256
/// (`written_sha256`), which are always equal, along with `media_type`, `target`,
/// `bytes`, `overwrite`, `dry_run` and `result_sha256`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportExportEvent {
    /// Source rendered-report blob SHA-256 digest.
    pub report: Digest,
    /// Normalised report media type.
    pub media_type: String,
    /// Canonical target handle or path string.
    pub target: String,
    /// `overwrite` parameter.
    pub overwrite: bool,
    /// `dry_run` parameter.
    pub dry_run: bool,
    /// SHA-256 digest of the written (or dry-run streamed) bytes; equals `report`.
    pub written_sha256: Digest,
    /// Written byte length.
    pub bytes: u64,
    /// Digest of the canonical [`ReportExportResult`].
    pub result_sha256: Digest,
}

impl ReportExportEvent {
    /// Encodes the journal event as a canonical JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("bytes".to_owned(), CValue::U64(self.bytes));
        m.insert("command".to_owned(), CValue::Str(COMMAND.to_owned()));
        m.insert("dry_run".to_owned(), CValue::Bool(self.dry_run));
        m.insert("media_type".to_owned(), CValue::Str(self.media_type.clone()));
        m.insert("overwrite".to_owned(), CValue::Bool(self.overwrite));
        m.insert("report".to_owned(), CValue::Str(self.report.as_str().to_owned()));
        m.insert("result_sha256".to_owned(), CValue::Str(self.result_sha256.as_str().to_owned()));
        m.insert("target".to_owned(), CValue::Str(self.target.clone()));
        m.insert("written_sha256".to_owned(), CValue::Str(self.written_sha256.as_str().to_owned()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ReportExportError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| ReportExportError::Event { reason: "json_syntax" })
    }

    /// SHA-256 digest of [`Self::canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, ReportExportError> {
        Ok(sha256_digest(&self.canonical_bytes()?))
    }

    /// Strictly parses a journal event from canonical JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ReportExportError> {
        let bad = |reason: &'static str| ReportExportError::Event { reason };
        let v =
            fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad("json_syntax"))?;
        let o = match &v {
            CValue::Obj(o) => o,
            _ => return Err(bad("wrong_type")),
        };
        let want = [
            "bytes",
            "command",
            "dry_run",
            "media_type",
            "overwrite",
            "report",
            "result_sha256",
            "target",
            "written_sha256",
        ];
        if o.len() != want.len() || want.iter().any(|k| !o.contains_key(*k)) {
            return Err(bad("bad_keys"));
        }
        let get_str = |k: &str| -> Result<&str, ReportExportError> {
            match o.get(k) {
                Some(CValue::Str(s)) => Ok(s.as_str()),
                _ => Err(bad("wrong_type")),
            }
        };
        let get_bool = |k: &str| -> Result<bool, ReportExportError> {
            match o.get(k) {
                Some(CValue::Bool(b)) => Ok(*b),
                _ => Err(bad("wrong_type")),
            }
        };
        let get_digest = |k: &str| -> Result<Digest, ReportExportError> {
            Digest::parse(get_str(k)?).map_err(|_| bad("bad_digest"))
        };
        let get_u64 = |k: &str| -> Result<u64, ReportExportError> {
            let t = get_str(k)?;
            let canonical =
                !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) && !(t.len() > 1 && t.starts_with('0'));
            if !canonical {
                return Err(bad("bad_integer"));
            }
            t.parse().map_err(|_| bad("bad_integer"))
        };

        if get_str("command")? != COMMAND {
            return Err(bad("wrong_command"));
        }
        let media_type = normalize_report_media_type(get_str("media_type")?).map_err(|_| bad("bad_media_type"))?;
        let target = get_str("target")?.to_owned();
        if target.is_empty() || target.len() > MAX_TARGET_BYTES {
            return Err(bad("bad_target"));
        }
        let report = get_digest("report")?;
        let written_sha256 = get_digest("written_sha256")?;
        let result_sha256 = get_digest("result_sha256")?;
        let bytes = get_u64("bytes")?;
        let overwrite = get_bool("overwrite")?;
        let dry_run = get_bool("dry_run")?;

        Ok(Self { report, media_type, target, overwrite, dry_run, written_sha256, bytes, result_sha256 })
    }
}

/// Successful return of [`export_results_report`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedReport {
    /// Command result payload (`{ target, sha256, bytes, media_type }`).
    pub result: ReportExportResult,
    /// Shared `IO.REG.ENV.REPORT` export envelope.
    pub export_report: ExportReport,
    /// Journal event recording both source and written digests.
    pub event: ReportExportEvent,
}

/// Runs `export.results_report@1`: validates target root and overwrite rules, looks up
/// the rendered-report blob, checks its media type against the report allowlist, streams
/// its bytes unchanged through [`AtomicWriter`], verifies the written digest matches the
/// source blob digest, and returns the result and journal event.
pub fn export_results_report(
    store: &dyn ReportBlobStore,
    roots: &RootSet,
    principal: Principal,
    req: &ReportExportRequest<'_>,
) -> Result<ExportedReport, ReportExportError> {
    // 1. Enforce root policy (raw_path_forbidden, path_outside_root, root_not_found, root_read_only).
    let target = roots.resolve_export(principal, req.target)?;
    let opts = WriteOptions::overwrite(req.overwrite);

    // 2. Enforce overwrite rule before opening or streaming the blob (target_exists).
    AtomicWriter::check(&target, opts)?;

    // 3. Look up rendered report blob.
    let mut stream = store.open_report(&req.report)?.ok_or(ReportExportError::ReportNotFound { digest: req.report })?;

    // 4. Check media type against the report allowlist.
    let media_type = normalize_report_media_type(&stream.media_type)?;

    // Capture canonical location before moving `target` into `AtomicWriter`.
    let (location, target_str) = match RootHandle::parse(req.target) {
        Ok(handle) => {
            let s = handle.to_string();
            (Location::handle(handle)?, s)
        }
        Err(_) => (Location::human_path(req.target)?, req.target.to_owned()),
    };

    // 5. Stream bytes unchanged and verify SHA-256 equals `req.report`.
    let receipt: WriteReceipt = if req.dry_run {
        stream_and_hash_only(stream.reader.as_mut(), req.report)?
    } else {
        let mut writer = AtomicWriter::create(target, opts)?;
        let mut buf = vec![0u8; STREAM_CHUNK_BYTES];
        loop {
            match stream.reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = writer.write_bytes(&buf[..n]) {
                        writer.cancel();
                        return Err(ReportExportError::Io(e));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    let at = Position::byte(writer.bytes_written());
                    writer.cancel();
                    return Err(ReportExportError::Io(IoError::Io { kind: e.kind(), at }));
                }
            }
        }
        let prepared = match writer.prepare() {
            Ok(r) => r,
            Err(e) => {
                writer.cancel();
                return Err(ReportExportError::Io(e));
            }
        };
        if prepared.sha256 != req.report {
            writer.cancel();
            return Err(ReportExportError::DigestMismatch {
                expected: Box::new(req.report),
                actual: Box::new(prepared.sha256),
            });
        }
        let committed = writer.commit()?;
        if committed.sha256 != req.report {
            return Err(ReportExportError::DigestMismatch {
                expected: Box::new(req.report),
                actual: Box::new(committed.sha256),
            });
        }
        committed
    };

    let result = ReportExportResult {
        target: target_str.clone(),
        sha256: receipt.sha256,
        bytes: receipt.bytes,
        media_type: media_type.clone(),
    };
    let result_sha256 = result.digest()?;
    let export_report = ReportBuilder::new().finish_export(
        FormatId::new(FORMAT_ID)?,
        location,
        receipt.sha256,
        receipt.bytes,
        req.dry_run,
    );
    let event = ReportExportEvent {
        report: req.report,
        media_type,
        target: target_str,
        overwrite: req.overwrite,
        dry_run: req.dry_run,
        written_sha256: receipt.sha256,
        bytes: receipt.bytes,
        result_sha256,
    };

    Ok(ExportedReport { result, export_report, event })
}

/// Replays a journaled `export.results_report@1` event into `roots` (AC-6, CORE.RPL):
/// - `NotReplayable` when the referenced report blob is not in `store`;
/// - `Identical` when the re-exported file digest, byte length, media type and result
///   digest match the recorded event;
/// - `Divergent` otherwise.
pub fn replay_results_report(
    event: &ReportExportEvent,
    store: &dyn ReportBlobStore,
    roots: &RootSet,
    principal: Principal,
) -> Result<ReplayOutcome, ReportExportError> {
    if event.written_sha256 != event.report {
        return Ok(ReplayOutcome::Divergent);
    }
    let req = ReportExportRequest {
        report: event.report,
        target: &event.target,
        overwrite: event.overwrite,
        dry_run: event.dry_run,
    };
    match export_results_report(store, roots, principal, &req) {
        Ok(exported) => {
            let same = exported.event.written_sha256 == event.written_sha256
                && exported.event.bytes == event.bytes
                && exported.event.media_type == event.media_type
                && exported.event.result_sha256 == event.result_sha256;
            Ok(if same { ReplayOutcome::Identical } else { ReplayOutcome::Divergent })
        }
        Err(ReportExportError::ReportNotFound { .. }) => Ok(ReplayOutcome::NotReplayable),
        Err(ReportExportError::DigestMismatch { .. }) | Err(ReportExportError::MediaTypeNotReport { .. }) => {
            Ok(ReplayOutcome::Divergent)
        }
        Err(e) => Err(e),
    }
}

fn stream_and_hash_only(reader: &mut dyn Read, expected: Digest) -> Result<WriteReceipt, ReportExportError> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; STREAM_CHUNK_BYTES];
    let mut total: u64 = 0;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buf[..n]);
                total = total.saturating_add(n as u64);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(ReportExportError::Io(IoError::Io { kind: e.kind(), at: Position::byte(total) }));
            }
        }
    }
    let out = hasher.finalize();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&out);
    let actual = Digest::from_bytes(raw);
    if actual != expected {
        return Err(ReportExportError::DigestMismatch { expected: Box::new(expected), actual: Box::new(actual) });
    }
    Ok(WriteReceipt { sha256: actual, bytes: total })
}

fn sha256_digest(bytes: &[u8]) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&out);
    Digest::from_bytes(raw)
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn strs<const N: usize>(items: [&str; N]) -> CValue {
    CValue::Arr(items.into_iter().map(text).collect())
}

fn num(n: usize) -> CValue {
    CValue::Num(n as f64)
}

fn flag(description: &str) -> CValue {
    obj([("type", text("boolean")), ("default", CValue::Bool(false)), ("description", text(description))])
}

/// Returns the registry [`CValue`] definition for `export.results_report@1`.
pub fn command_def_value() -> CValue {
    let params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["report", "target"])),
        (
            "properties",
            obj([
                (
                    "report",
                    obj([
                        ("type", text("string")),
                        ("pattern", text(DIGEST_PATTERN)),
                        ("description", text("SHA-256 digest of the RPT-rendered report blob.")),
                    ]),
                ),
                (
                    "target",
                    obj([
                        ("type", text("string")),
                        ("path_like", CValue::Bool(true)),
                        ("x-extensions", strs(["docx", "htm", "html", "pdf", "pptx", "xlsx"])),
                        ("minLength", num(1)),
                        ("maxLength", num(MAX_TARGET_BYTES)),
                        (
                            "description",
                            text("Target file: a root:// handle in an approved export root (humans may pass a raw path)."),
                        ),
                    ]),
                ),
                ("overwrite", flag("Replace an existing target file; defaults to false (fails target_exists).")),
                ("dry_run", flag("Verify blob, media type and target rules without writing to disk.")),
            ]),
        ),
    ]);
    let result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["bytes", "media_type", "sha256", "target"])),
        (
            "properties",
            obj([
                (
                    "bytes",
                    obj([
                        ("type", text("string")),
                        ("pattern", text(U64_PATTERN)),
                        ("description", text("Size of the written (or dry-run) report file in bytes.")),
                    ]),
                ),
                (
                    "media_type",
                    obj([
                        ("type", text("string")),
                        ("minLength", num(1)),
                        ("maxLength", num(MAX_MEDIA_TYPE_BYTES)),
                        ("description", text("Normalised MIME media type of the rendered report blob.")),
                    ]),
                ),
                (
                    "sha256",
                    obj([
                        ("type", text("string")),
                        ("pattern", text(DIGEST_PATTERN)),
                        (
                            "description",
                            text("SHA-256 digest of the written report bytes (equals source blob digest)."),
                        ),
                    ]),
                ),
                (
                    "target",
                    obj([
                        ("type", text("string")),
                        ("minLength", num(1)),
                        ("x-untrusted", CValue::Bool(true)),
                        ("description", text("Written target handle or path.")),
                    ]),
                ),
            ]),
        ),
    ]);
    obj([
        ("id", text("export.results_report")),
        ("major", CValue::Num(1.0)),
        ("title", text("Export rendered report")),
        (
            "description",
            text("Streams an RPT-rendered report blob unchanged into an approved export root via AtomicWriter and journals both source and written digests."),
        ),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("W1")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("write_root")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs([COMMAND])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("unavailable")),
        ("web_note", text("Writes into a local export root; the web build has no file-system roots.")),
        ("cli_bare", CValue::Bool(false)),
    ])
}

/// Validates and returns the registry [`CommandDef`] for `export.results_report@1`.
pub fn command_def() -> Result<CommandDef, ReportExportError> {
    CommandDef::from_value(command_def_value()).map_err(|_| ReportExportError::Internal("invalid_command_def"))
}
