//! Error types for `RPT.BALLOON.REG` (`fid_report::balloon::register`).

use std::fmt;

/// Typed errors of the balloon register (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// `report.balloon_number_taken`: target balloon number is already in use on this drawing revision (`REQ-002`, `REQ-004`).
    BalloonNumberTaken { number: u32 },
    /// `report.char_already_ballooned`: characteristic already has a balloon on this drawing revision (`REQ-003`).
    CharAlreadyBallooned { characteristic: String },
    /// `report.balloon_off_sheet`: anchor or label position lies outside the sheet bounds (`REQ-008`, `AC-6`).
    BalloonOffSheet { x: i64, y: i64, width_cmm: i64, height_cmm: i64 },
    /// `report.not_found`: named balloon, drawing revision, or sheet does not exist (or sheet is not ready).
    NotFound(&'static str),
    /// `report.text_invalid`: zone exceeded 16 bytes or contained NUL (`REQ-010`).
    TextInvalid(&'static str),
    /// Invalid command parameters (e.g. number == 0, sheet belongs to a different drawing revision).
    BadParams(&'static str),
    /// Corrupt event or ID generator fault.
    Store(String),
}

impl RegisterError {
    pub fn code(&self) -> &'static str {
        match self {
            RegisterError::BalloonNumberTaken { .. } => "report.balloon_number_taken",
            RegisterError::CharAlreadyBallooned { .. } => "report.char_already_ballooned",
            RegisterError::BalloonOffSheet { .. } => "report.balloon_off_sheet",
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
                write!(f, "report.balloon_number_taken: number {number} already in use")
            }
            RegisterError::CharAlreadyBallooned { characteristic } => {
                write!(f, "report.char_already_ballooned: characteristic {characteristic}")
            }
            RegisterError::BalloonOffSheet { x, y, width_cmm, height_cmm } => {
                write!(f, "report.balloon_off_sheet: ({x}, {y}) outside 0..={width_cmm} x 0..={height_cmm}")
            }
            RegisterError::NotFound(what) => write!(f, "report.not_found: {what}"),
            RegisterError::TextInvalid(reason) => write!(f, "report.text_invalid: {reason}"),
            RegisterError::BadParams(reason) => write!(f, "report.bad_params: {reason}"),
            RegisterError::Store(detail) => write!(f, "report.store_invalid: {detail}"),
        }
    }
}

impl std::error::Error for RegisterError {}
