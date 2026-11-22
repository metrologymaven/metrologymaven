//! Transcribed-status marking for views and exports (spec node `IO.RES.TRANS.VIEW`).
//!
//! Provides the single canonical marking of a transcribed result (`record_status:
//! "transcribed"` plus `source_software` and `source_version`) across JSON views (CLI/MCP),
//! table columns, and one-line text labels, together with the conformance kit
//! ([`assert_marked`] and [`MarkExtractor`]) that every results exporter and view runs on its
//! own output so a transcribed value can never be presented as re-evaluated.
//!
//! Nothing in this module panics or uses `unsafe`.

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{to_canonical_bytes, CValue};
use fid_io_reg::IoError;
use fid_project::Uuid;

use crate::trans::model::{SourceText, TranscribedResult, TranscriptionRecord, STATUS_TRANSCRIBED};

/// Stable entity identifier of a result (transcribed or evaluated).
pub type ResultId = Uuid;

/// Maximum number of Unicode scalar values kept for each escaped part (`software` and
/// `version`) of the one-line label (REQ-IO.RES.TRANS.VIEW-002).
pub const MAX_LABEL_PART_CHARS: usize = 64;

/// Canonical table column headers for transcribed-status marking.
pub const TABLE_COLUMN_HEADERS: [&str; 3] = ["record_status", "source_software", "source_version"];

/// Errors returned by [`TranscriptionMark`] construction, rendering, and [`assert_marked`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewError {
    /// A transcribed result in an exporter or view output lacked its required marking
    /// (REQ-IO.RES.TRANS.VIEW-003).
    TranscriptionUnmarked { result: ResultId },
    /// A non-transcribed result was marked as `transcribed` or had a mismatched status
    /// (REQ-IO.RES.TRANS.VIEW-004).
    UnexpectedTranscriptionMark { result: ResultId },
    /// A [`TranscribedResult`] did not reference the supplied [`TranscriptionRecord`].
    RecordMismatch { result: ResultId, expected_record: Uuid, found_record: Uuid },
    /// A non-transcribed `record_status` was empty, contained control characters, or
    /// contained the reserved word `transcribed`.
    InvalidRecordStatus(&'static str),
    /// A [`MarkExtractor`] failed to parse the exporter output.
    Extractor(IoError),
    /// Canonical JSON encoding failed.
    Internal(&'static str),
}

impl ViewError {
    /// Stable snake_case error code.
    pub fn code(&self) -> &'static str {
        match self {
            ViewError::TranscriptionUnmarked { .. } => "transcription_unmarked",
            ViewError::UnexpectedTranscriptionMark { .. } => "unexpected_transcription_mark",
            ViewError::RecordMismatch { .. } => "record_mismatch",
            ViewError::InvalidRecordStatus(_) => "invalid_record_status",
            ViewError::Extractor(e) => e.code(),
            ViewError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for ViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViewError::TranscriptionUnmarked { result } => write!(f, "transcription_unmarked{{{result}}}"),
            ViewError::UnexpectedTranscriptionMark { result } => {
                write!(f, "unexpected_transcription_mark{{{result}}}")
            }
            ViewError::RecordMismatch { result, expected_record, found_record } => {
                write!(f, "record_mismatch{{result={result}, expected={expected_record}, found={found_record}}}")
            }
            ViewError::InvalidRecordStatus(reason) => write!(f, "invalid_record_status{{{reason}}}"),
            ViewError::Extractor(e) => write!(f, "extractor{{{e}}}"),
            ViewError::Internal(reason) => write!(f, "internal{{{reason}}}"),
        }
    }
}

impl std::error::Error for ViewError {}

/// Table-column values rendered from a [`TranscriptionMark`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableColumns {
    /// Record status column (`"transcribed"` or the non-transcribed result status).
    pub record_status: String,
    /// Escaped source software name (`Some` for transcribed results, `None` otherwise).
    pub source_software: Option<String>,
    /// Escaped source software version (`Some` for transcribed results, `None` otherwise).
    pub source_version: Option<String>,
}

/// Canonical transcribed-status marking for one result (REQ-IO.RES.TRANS.VIEW-001..004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionMark {
    record_status: String,
    source_software: Option<SourceText>,
    source_version: Option<SourceText>,
}

impl TranscriptionMark {
    /// Derives the canonical mark for a [`TranscribedResult`] from its [`TranscriptionRecord`].
    /// Refuses if `result.record() != record.id()`.
    pub fn from_transcribed(result: &TranscribedResult, record: &TranscriptionRecord) -> Result<Self, ViewError> {
        if result.record() != record.id() {
            return Err(ViewError::RecordMismatch {
                result: result.id(),
                expected_record: result.record(),
                found_record: record.id(),
            });
        }
        Ok(Self::transcribed(record.source_software().clone(), record.source_version().clone()))
    }

    /// Constructs a transcribed mark directly from untrusted `source_software` and
    /// `source_version` text.
    pub fn transcribed(source_software: SourceText, source_version: SourceText) -> Self {
        Self {
            record_status: STATUS_TRANSCRIBED.to_string(),
            source_software: Some(source_software),
            source_version: Some(source_version),
        }
    }

    /// Constructs a mark for a non-transcribed (e.g. `evaluated` / `measured`) result.
    /// Emits `record_status` unchanged and refuses any status containing `transcribed`
    /// (REQ-IO.RES.TRANS.VIEW-004).
    pub fn non_transcribed(record_status: &str) -> Result<Self, ViewError> {
        if record_status.is_empty() || record_status.len() > 64 {
            return Err(ViewError::InvalidRecordStatus("status must be 1..=64 bytes"));
        }
        if record_status.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(ViewError::InvalidRecordStatus("status must not contain whitespace or control characters"));
        }
        if record_status.to_ascii_lowercase().contains(STATUS_TRANSCRIBED) {
            return Err(ViewError::InvalidRecordStatus("non-transcribed status may not contain 'transcribed'"));
        }
        Ok(Self { record_status: record_status.to_string(), source_software: None, source_version: None })
    }

    /// The `record_status` value (`"transcribed"` or the non-transcribed status unchanged).
    pub fn record_status(&self) -> &str {
        &self.record_status
    }

    /// True if this mark represents a transcribed result.
    pub fn is_transcribed(&self) -> bool {
        self.record_status == STATUS_TRANSCRIBED
    }

    /// Untrusted source software name (`Some` for transcribed results, `None` otherwise).
    pub fn source_software(&self) -> Option<&SourceText> {
        self.source_software.as_ref()
    }

    /// Untrusted source software version (`Some` for transcribed results, `None` otherwise).
    pub fn source_version(&self) -> Option<&SourceText> {
        self.source_version.as_ref()
    }

    /// Renders the JSON fragment as a [`CValue`] (REQ-IO.RES.TRANS.VIEW-001, -004).
    ///
    /// Transcribed results emit `{"record_status": "transcribed", "source_software": ...,
    /// "source_version": ...}`. Non-transcribed results emit only `{"record_status": ...}`
    /// and never emit the word `transcribed`.
    pub fn to_json_value(&self) -> CValue {
        let mut map = BTreeMap::new();
        map.insert("record_status".to_string(), CValue::Str(self.record_status.clone()));
        if let (Some(sw), Some(ver)) = (&self.source_software, &self.source_version) {
            map.insert("source_software".to_string(), CValue::Str(sw.raw().to_string()));
            map.insert("source_version".to_string(), CValue::Str(ver.raw().to_string()));
        }
        CValue::Obj(map)
    }

    /// Renders the JSON fragment as RFC 8785 canonical JSON bytes.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, ViewError> {
        to_canonical_bytes(&self.to_json_value()).map_err(|_| ViewError::Internal("canonical json"))
    }

    /// Renders safe table-column values (REQ-IO.RES.TRANS.VIEW-001, -004).
    pub fn table_columns(&self) -> TableColumns {
        TableColumns {
            record_status: self.record_status.clone(),
            source_software: self.source_software.as_ref().map(SourceText::escaped),
            source_version: self.source_version.as_ref().map(SourceText::escaped),
        }
    }

    /// Renders the one-line text label (REQ-IO.RES.TRANS.VIEW-002, -004).
    ///
    /// For transcribed results: `transcribed from <software> <version>` where `<software>`
    /// and `<version>` are escaped as untrusted text via [`SourceText::escaped`] and
    /// truncated to at most [`MAX_LABEL_PART_CHARS`] (64) Unicode scalar values each.
    /// For non-transcribed results: `record_status` unchanged (never contains `transcribed`).
    pub fn one_line_label(&self) -> String {
        match (&self.source_software, &self.source_version) {
            (Some(sw), Some(ver)) => {
                format!("transcribed from {} {}", escape_and_truncate(sw), escape_and_truncate(ver))
            }
            _ => self.record_status.clone(),
        }
    }
}

fn escape_and_truncate(text: &SourceText) -> String {
    text.escaped().chars().take(MAX_LABEL_PART_CHARS).collect()
}

/// Extracted marking observed by a [`MarkExtractor`] for one result in an exporter or view
/// output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// Observed `record_status` field/column, if present and non-empty.
    pub record_status: Option<String>,
    /// Observed `source_software` field/column (raw or display-escaped), if present.
    pub source_software: Option<String>,
    /// Observed `source_version` field/column (raw or display-escaped), if present.
    pub source_version: Option<String>,
}

impl Mark {
    /// Builds an expected [`Mark`] from a [`TranscriptionMark`] using its raw source fields.
    pub fn from_transcription_mark(m: &TranscriptionMark) -> Self {
        Self {
            record_status: Some(m.record_status.clone()),
            source_software: m.source_software.as_ref().map(|s| s.raw().to_string()),
            source_version: m.source_version.as_ref().map(|s| s.raw().to_string()),
        }
    }
}

/// Format-specific extractor implemented by each exporter and view to run the conformance kit
/// [`assert_marked`] against its own output (REQ-IO.RES.TRANS.VIEW-003).
pub trait MarkExtractor {
    /// Parses `output` and returns `(ResultId, Mark)` for every result present in `output`.
    fn marks(&self, output: &[u8]) -> Result<Vec<(ResultId, Mark)>, IoError>;
}

/// Helper building the expected `(ResultId, TranscriptionMark)` slice for a transcription
/// batch produced by `IO.RES.TRANS.MODEL`.
pub fn expected_from_transcription(
    record: &TranscriptionRecord,
    results: &[TranscribedResult],
) -> Result<Vec<(ResultId, TranscriptionMark)>, ViewError> {
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        out.push((r.id(), TranscriptionMark::from_transcribed(r, record)?));
    }
    Ok(out)
}

fn matches_source_text(observed: Option<&str>, expected: &SourceText) -> bool {
    let Some(obs) = observed else {
        return false;
    };
    if obs.is_empty() {
        return false;
    }
    obs == expected.raw() || obs == expected.escaped() || obs == escape_and_truncate(expected)
}

/// Conformance kit check run by every exporter and view on its own output
/// (REQ-IO.RES.TRANS.VIEW-003, -004).
///
/// Fails with [`ViewError::TranscriptionUnmarked`] naming the first offending [`ResultId`]
/// when any transcribed result in `results` is absent from `output` or lacks
/// `record_status == "transcribed"`, `source_software`, or `source_version`. Fails with
/// [`ViewError::UnexpectedTranscriptionMark`] when a non-transcribed result in `output`
/// carries `"transcribed"` or a mismatched `record_status`.
pub fn assert_marked<E: MarkExtractor>(
    extractor: &E,
    output: &[u8],
    results: &[(ResultId, TranscriptionMark)],
) -> Result<(), ViewError> {
    let extracted = extractor.marks(output).map_err(ViewError::Extractor)?;
    let mut by_id: BTreeMap<ResultId, Vec<Mark>> = BTreeMap::new();
    for (id, mark) in extracted {
        by_id.entry(id).or_default().push(mark);
    }

    for (result_id, expected) in results {
        let Some(observed_list) = by_id.get(result_id) else {
            if expected.is_transcribed() {
                return Err(ViewError::TranscriptionUnmarked { result: *result_id });
            }
            continue;
        };

        if expected.is_transcribed() {
            let (Some(exp_sw), Some(exp_ver)) = (expected.source_software(), expected.source_version()) else {
                return Err(ViewError::Internal("transcribed mark missing source metadata"));
            };
            // Every occurrence of a transcribed result in the output must carry the full mark.
            for obs in observed_list {
                let status_ok = obs.record_status.as_deref() == Some(STATUS_TRANSCRIBED);
                let sw_ok = matches_source_text(obs.source_software.as_deref(), exp_sw);
                let ver_ok = matches_source_text(obs.source_version.as_deref(), exp_ver);
                if !(status_ok && sw_ok && ver_ok) {
                    return Err(ViewError::TranscriptionUnmarked { result: *result_id });
                }
            }
        } else {
            for obs in observed_list {
                if let Some(status) = obs.record_status.as_deref() {
                    if status.to_ascii_lowercase().contains(STATUS_TRANSCRIBED) || status != expected.record_status() {
                        return Err(ViewError::UnexpectedTranscriptionMark { result: *result_id });
                    }
                }
                if obs.source_software.as_deref().is_some_and(|s| s.to_ascii_lowercase().contains(STATUS_TRANSCRIBED))
                    || obs
                        .source_version
                        .as_deref()
                        .is_some_and(|s| s.to_ascii_lowercase().contains(STATUS_TRANSCRIBED))
                {
                    return Err(ViewError::UnexpectedTranscriptionMark { result: *result_id });
                }
            }
        }
    }

    Ok(())
}
