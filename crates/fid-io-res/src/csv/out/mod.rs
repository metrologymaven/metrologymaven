//! Result table export through a column map (spec node `IO.RES.CSV.OUT`), the library
//! behind `export.results_csv@1` (W1, `fs_effect: write_root`; CLI
//! `fid export results-csv`, MCP `export_results_csv`).
//!
//! [`export_results_csv`] renders result rows through a [`ColumnMap`] ([`render`]),
//! checks the transcription marking of the bytes it is about to write ([`check_marked`]),
//! writes them atomically ([`write_atomic`]) and returns the [`ExportReport`] (with the
//! map's `lossy` fields) and the journal event [`ExportEvent`]. [`replay`] re-runs an
//! event and classifies the result (`IDENTICAL` when the bytes match).
//!
//! - **Authored decimals (REQ-001):** decimal fields are written from their authored text;
//!   only the decimal mark is swapped for the map's. No number is ever parsed or formatted
//!   through the platform, so the output does not depend on locale.
//! - **Lossy (REQ-002):** the report and the event list the lossy-check fields the map
//!   omits ([`ColumnMap::lossy`]).
//! - **Formula injection (REQ-003):** text and enumeration cells (and header cells)
//!   starting with `=`, `+`, `-`, `@`, tab or CR get a leading `'` ([`neutralise`]);
//!   decimal cells never do.
//! - **Transcription marking (REQ-004):** with a map holding `record_status` and
//!   `source_software`, the export fails `transcription_unmarked` rather than write a
//!   table in which a transcribed result is not marked.
//! - **Determinism (REQ-005, D0):** rows are sorted by characteristic, part and result
//!   ID (byte-wise), and the file bytes depend only on the rows and the map.

mod event;
mod kit;
mod records;
mod render;
mod write;

pub use event::ExportEvent;
pub use kit::{check_marked, read_table, split_records};
pub use records::{
    check_rows, load_records, records_digest, ResultRow, MAX_RECORDS_BYTES, MAX_RECORDS_DEPTH, RECORDS_SCHEMA,
    RESULT_ID_KEY, TRANSCRIBED,
};
pub use render::{neutralise, render, unneutralise, Rendered, FORMULA_TRIGGERS, NEUTRALISER};
pub use write::{precheck, write_atomic, WriteReceipt, TMP_PREFIX};

use crate::csv::map::{ColMapError, ColumnMap, Field};
use fid_io_reg::{ExportReport, FormatId, IoError, Location, ReportBuilder, ReportError, RootHandle, TypeName};
use fid_replay::ReplayOutcome;
use std::fmt;
use std::path::Path;

/// The registry command.
pub const COMMAND: &str = "export.results_csv@1";
/// The format ID in the export report.
pub const FORMAT_ID: &str = "results-csv";

/// Errors of the export. [`OutError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutError {
    /// `colmap_invalid` (or a reader-harness code) from the map.
    ColMap(ColMapError),
    /// `no_results`: nothing to export.
    NoResults,
    /// `records_invalid`: the input rows break a rule.
    Records {
        /// Static reason code.
        reason: &'static str,
        /// Zero-based input position of the offending row, if any.
        record: Option<usize>,
    },
    /// A dialect error while encoding or reading back the table.
    Encode(IoError),
    /// An envelope or file-system error: `target_exists`, `io`, `raw_path_forbidden`,
    /// `path_outside_root`, `root_not_found`.
    Io(IoError),
    /// `target_is_link`: the target, or a folder on its way, is a symbolic link.
    TargetIsLink,
    /// `transcription_unmarked`: a transcribed result is not marked in the output.
    TranscriptionUnmarked {
        /// The result ID.
        result: String,
    },
    /// A report could not be built.
    Report(ReportError),
    /// `event_invalid`: a journal event could not be read back.
    Event {
        /// Static reason code.
        reason: &'static str,
    },
}

impl OutError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            OutError::ColMap(e) => e.code(),
            OutError::NoResults => "no_results",
            OutError::Records { .. } => "records_invalid",
            OutError::Encode(e) | OutError::Io(e) => e.code(),
            OutError::TargetIsLink => "target_is_link",
            OutError::TranscriptionUnmarked { .. } => "transcription_unmarked",
            OutError::Report(e) => e.code(),
            OutError::Event { .. } => "event_invalid",
        }
    }

    /// The static reason, where there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            OutError::ColMap(e) => e.reason(),
            OutError::Records { reason, .. } | OutError::Event { reason } => Some(reason),
            _ => None,
        }
    }
}

impl fmt::Display for OutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutError::ColMap(e) => write!(f, "{e}"),
            OutError::Records { reason, record: Some(n) } => write!(f, "records_invalid: {reason} (record {n})"),
            OutError::Records { reason, record: None } => write!(f, "records_invalid: {reason}"),
            OutError::Encode(e) | OutError::Io(e) => write!(f, "{e}"),
            OutError::Event { reason } => write!(f, "event_invalid: {reason}"),
            // Result IDs come from input files: shown escaped, never raw.
            OutError::TranscriptionUnmarked { result } => {
                write!(f, "transcription_unmarked: {}", fid_io_reg::UntrustedText::new(result).as_display())
            }
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for OutError {}

impl From<ColMapError> for OutError {
    fn from(e: ColMapError) -> Self {
        OutError::ColMap(e)
    }
}

impl From<IoError> for OutError {
    fn from(e: IoError) -> Self {
        OutError::Io(e)
    }
}

impl From<ReportError> for OutError {
    fn from(e: ReportError) -> Self {
        OutError::Report(e)
    }
}

/// The parameters of one export (the registry command's `map`, `target`,
/// `characteristics`, `overwrite` and `dry_run`).
#[derive(Debug, Clone)]
pub struct ExportRequest<'a> {
    /// The column map.
    pub map: &'a ColumnMap,
    /// The target as a root handle, `root://<root>/<path>`.
    pub target: &'a str,
    /// Only results of these characteristic IDs; `None` for all.
    pub characteristics: Option<Vec<String>>,
    /// Replace an existing target.
    pub overwrite: bool,
    /// Render and report, but write nothing.
    pub dry_run: bool,
}

/// What a successful export returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The export report (`--json` output, MCP result).
    pub report: ExportReport,
    /// The journal event.
    pub event: ExportEvent,
    /// The file bytes (also for a dry run).
    pub bytes: Vec<u8>,
}

/// Runs `export.results_csv@1`: `rows` through `req.map` into `req.target`, a handle
/// whose root is the folder `root_dir`. Checks the target before rendering
/// (`target_exists` without `overwrite`, also for a dry run).
pub fn export_results_csv(rows: &[ResultRow], req: &ExportRequest<'_>, root_dir: &Path) -> Result<Exported, OutError> {
    let handle = RootHandle::parse(req.target)?;
    precheck(root_dir, &handle.rel, req.overwrite)?;
    let selected = select(rows, req.characteristics.as_deref());
    let rendered = render(&selected, req.map)?;
    if req.map.has_field(Field::RecordStatus) && req.map.has_field(Field::SourceSoftware) {
        check_marked(&rendered.bytes, req.map, &selected)?;
    }
    let receipt = if req.dry_run {
        WriteReceipt::of(&rendered.bytes)
    } else {
        write_atomic(root_dir, &handle.rel, &rendered.bytes, req.overwrite)?
    };
    let lossy = req.map.lossy();
    let mut builder = ReportBuilder::new();
    for f in &lossy {
        builder.lossy(&TypeName::new(f.as_str())?);
    }
    let report = builder.finish_export(
        FormatId::new(FORMAT_ID)?,
        Location::handle(handle.clone())?,
        receipt.sha256,
        receipt.bytes,
        req.dry_run,
    );
    let event = ExportEvent {
        map: req.map.identity(),
        records_sha256: records_digest(&selected)?,
        target: handle.to_string(),
        characteristics: req.characteristics.clone(),
        overwrite: req.overwrite,
        dry_run: req.dry_run,
        lossy: lossy.iter().map(|f| f.as_str().to_owned()).collect(),
        rows: rendered.rows,
        output: receipt,
        report_sha256: report.digest()?,
    };
    Ok(Exported { report, event, bytes: rendered.bytes })
}

/// The rows of the given characteristic IDs, in input order (all for `None`).
fn select(rows: &[ResultRow], characteristics: Option<&[String]>) -> Vec<ResultRow> {
    match characteristics {
        None => rows.to_vec(),
        Some(ids) => rows.iter().filter(|r| ids.iter().any(|c| c == r.characteristic())).cloned().collect(),
    }
}

/// Replays a journaled export with the same rows and map into the root folder
/// `root_dir` (CORE.RPL rules): `NOT_REPLAYABLE` when the map identity or the selected
/// rows' digest differ from the event's, `IDENTICAL` when the written file, report and
/// lossy list match, `DIVERGENT` otherwise.
pub fn replay(
    event: &ExportEvent,
    rows: &[ResultRow],
    map: &ColumnMap,
    root_dir: &Path,
) -> Result<ReplayOutcome, OutError> {
    let selected = select(rows, event.characteristics.as_deref());
    if map.identity() != event.map || records_digest(&selected)? != event.records_sha256 {
        return Ok(ReplayOutcome::NotReplayable);
    }
    let req = ExportRequest {
        map,
        target: &event.target,
        characteristics: event.characteristics.clone(),
        overwrite: event.overwrite,
        dry_run: event.dry_run,
    };
    let out = export_results_csv(&selected, &req, root_dir)?;
    Ok(if out.event == *event { ReplayOutcome::Identical } else { ReplayOutcome::Divergent })
}
