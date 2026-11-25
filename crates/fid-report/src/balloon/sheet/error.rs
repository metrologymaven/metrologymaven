//! Error types for `RPT.BALLOON.SHEET.IMPORT` (`fid_report::balloon::sheet`).

use crate::doc::ReportError;
use std::fmt;

/// Typed errors of the drawing-revision import and sheet-size commands (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SheetError {
    /// `report.drawing_unreadable`: `fid-io-draw` rejected the drawing bytes (`REQ-005`, `AC-3`).
    DrawingUnreadable { code: String, offset: Option<u64>, detail: String },
    /// `report.drawing_revision_exists`: duplicate `(part_rev, drawing_number, revision_label)` (`REQ-008`, `AC-4`).
    DrawingRevisionExists { part_rev: String, drawing_number: String, revision_label: String },
    /// `report.text_invalid`: untrusted text exceeded 256 bytes, was empty where required, or contained NUL (`REQ-004`).
    TextInvalid(&'static str),
    /// `report.not_found`: named drawing revision, sheet, or part revision does not exist.
    NotFound(&'static str),
    /// `report.size_invalid`: invalid physical width or DPI, or target sheet is not a raster sheet.
    SizeInvalid(&'static str),
    /// Approved import-root resolution or I/O failure (`REQ-006`, `AC-4`, e.g. `raw_path_forbidden`).
    File(&'static str),
    /// Invalid command parameters or malformed stored event/record.
    BadParams(&'static str),
    /// Content-addressed blob store or integrity failure.
    Store(String),
}

impl SheetError {
    /// Stable error code reported on CLI/MCP and checked in tests (`Design: Errors`).
    pub fn code(&self) -> &'static str {
        match self {
            SheetError::DrawingUnreadable { .. } => "report.drawing_unreadable",
            SheetError::DrawingRevisionExists { .. } => "report.drawing_revision_exists",
            SheetError::TextInvalid(_) => "report.text_invalid",
            SheetError::NotFound(_) => "report.not_found",
            SheetError::SizeInvalid(_) => "report.size_invalid",
            SheetError::File(code) => code,
            SheetError::BadParams(_) => "report.bad_params",
            SheetError::Store(_) => "report.store_invalid",
        }
    }
}

impl From<ReportError> for SheetError {
    fn from(e: ReportError) -> Self {
        match e {
            ReportError::TextInvalid(_) => SheetError::TextInvalid("text_invalid"),
            other => SheetError::Store(other.to_string()),
        }
    }
}

impl fmt::Display for SheetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SheetError::DrawingUnreadable { code, offset: Some(at), detail } => {
                write!(f, "report.drawing_unreadable ({code} at byte {at}): {detail}")
            }
            SheetError::DrawingUnreadable { code, offset: None, detail } => {
                write!(f, "report.drawing_unreadable ({code}): {detail}")
            }
            SheetError::DrawingRevisionExists { part_rev, drawing_number, revision_label } => write!(
                f,
                "report.drawing_revision_exists: part_rev={part_rev} number={drawing_number} rev={revision_label}"
            ),
            SheetError::TextInvalid(reason) => write!(f, "report.text_invalid: {reason}"),
            SheetError::NotFound(what) => write!(f, "report.not_found: {what}"),
            SheetError::SizeInvalid(reason) => write!(f, "report.size_invalid: {reason}"),
            SheetError::File(code) => write!(f, "{code}"),
            SheetError::BadParams(reason) => write!(f, "report.bad_params: {reason}"),
            SheetError::Store(detail) => write!(f, "report.store_invalid: {detail}"),
        }
    }
}

impl std::error::Error for SheetError {}
