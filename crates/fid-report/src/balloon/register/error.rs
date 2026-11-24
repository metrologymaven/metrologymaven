//! Errors of the balloon register (`RPT.BALLOON.REG`, `Design: Errors`).

use std::fmt;

/// Typed errors of the six balloon commands. A failed command journals nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// `report.balloon_number_taken`: the number is in use on the drawing revision
    /// (`REQ-002`, `REQ-004`).
    BalloonNumberTaken { number: u32 },
    /// `report.char_already_ballooned`: the characteristic already has another balloon on
    /// the drawing revision (`REQ-003`).
    CharAlreadyBallooned { characteristic: String },
    /// `report.balloon_off_sheet`: an anchor or label position lies outside the sheet
    /// bounds (`REQ-008`).
    BalloonOffSheet { x: i64, y: i64, width: i64, height: i64 },
    /// `report.sheet_not_ready`: the sheet's physical size is not known yet (a raster sheet
    /// with status `needs_size`, `RPT.BALLOON.SHEET.IMPORT` REQ-003).
    SheetNotReady,
    /// `report.not_found`: unknown or removed balloon, unknown characteristic, a sheet that
    /// is unknown or not on the drawing revision, or a revision with no balloon register.
    NotFound(&'static str),
    /// `report.text_invalid`: the zone is longer than 16 bytes or holds NUL (`REQ-010`).
    TextInvalid(&'static str),
    /// `report.bad_params`: malformed parameters (number 0, missing mode fields, ...).
    BadParams(&'static str),
    /// `report.store_invalid`: an ID generator fault or a malformed stored event.
    Store(String),
}

impl RegisterError {
    /// Stable error code (CLI, MCP and tests).
    pub fn code(&self) -> &'static str {
        match self {
            RegisterError::BalloonNumberTaken { .. } => "report.balloon_number_taken",
            RegisterError::CharAlreadyBallooned { .. } => "report.char_already_ballooned",
            RegisterError::BalloonOffSheet { .. } => "report.balloon_off_sheet",
            RegisterError::SheetNotReady => "report.sheet_not_ready",
            RegisterError::NotFound(_) => "report.not_found",
            RegisterError::TextInvalid(_) => "report.text_invalid",
            RegisterError::BadParams(_) => "report.bad_params",
            RegisterError::Store(_) => "report.store_invalid",
        }
    }
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterError::BalloonNumberTaken { number } => {
                write!(f, "report.balloon_number_taken: number {number} is in use")
            }
            RegisterError::CharAlreadyBallooned { characteristic } => {
                write!(f, "report.char_already_ballooned: {characteristic}")
            }
            RegisterError::BalloonOffSheet { x, y, width, height } => {
                write!(f, "report.balloon_off_sheet: ({x}, {y}) outside 0..={width} x 0..={height}")
            }
            RegisterError::SheetNotReady => write!(f, "report.sheet_not_ready"),
            RegisterError::NotFound(what) => write!(f, "report.not_found: {what}"),
            RegisterError::TextInvalid(why) => write!(f, "report.text_invalid: {why}"),
            RegisterError::BadParams(why) => write!(f, "report.bad_params: {why}"),
            RegisterError::Store(detail) => write!(f, "report.store_invalid: {detail}"),
        }
    }
}

impl std::error::Error for RegisterError {}
