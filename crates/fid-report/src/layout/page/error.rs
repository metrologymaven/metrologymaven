//! Errors of page setup (`RPT.DESIGN.PAGE`, `Design: Errors`). A failed command changes
//! nothing and journals nothing.

use crate::doc::ReportError;
use fid_project::Uuid;
use std::fmt;

/// Typed errors of `report.set_page_layout@1` and `report.show_page_layout@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// `report.layout_invalid { field, reason }`: a length that is not exact, too large or
    /// not a length (REQ-004), a custom size outside 50 mm to 2000 mm (REQ-002), or a body
    /// area narrower or shorter than 20 mm (REQ-005).
    LayoutInvalid { field: &'static str, reason: &'static str },
    /// `report.not_found`: the report, or the page within it, does not exist.
    NotFound(Uuid),
    /// `report.bad_params`: parameters of the wrong shape (unknown key, wrong type, missing
    /// report or size, unknown size or orientation name, a non-canonical ID).
    BadParams(&'static str),
    /// Any other error of the stored report (`RPT.MODEL.DOC`), with its own code.
    Report(ReportError),
    /// `report.store_invalid`: a stored layout or journal event that does not read back, or
    /// a commit whose event does not match the state.
    StoreInvalid(&'static str),
}

impl LayoutError {
    /// Stable error code (CLI, MCP and tests).
    pub fn code(&self) -> &'static str {
        match self {
            LayoutError::LayoutInvalid { .. } => "report.layout_invalid",
            LayoutError::NotFound(_) => "report.not_found",
            LayoutError::BadParams(_) => "report.bad_params",
            LayoutError::Report(e) => e.code(),
            LayoutError::StoreInvalid(_) => "report.store_invalid",
        }
    }

    /// The offending field of `report.layout_invalid`.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            LayoutError::LayoutInvalid { field, .. } => Some(field),
            _ => None,
        }
    }
}

impl From<ReportError> for LayoutError {
    fn from(e: ReportError) -> LayoutError {
        match e {
            ReportError::NotFound(id) => LayoutError::NotFound(id),
            other => LayoutError::Report(other),
        }
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::LayoutInvalid { field, reason } => write!(f, "report.layout_invalid: {field}: {reason}"),
            LayoutError::NotFound(id) => write!(f, "report.not_found: {id}"),
            LayoutError::BadParams(why) => write!(f, "report.bad_params: {why}"),
            LayoutError::Report(e) => write!(f, "{e}"),
            LayoutError::StoreInvalid(why) => write!(f, "report.store_invalid: {why}"),
        }
    }
}

impl std::error::Error for LayoutError {}
