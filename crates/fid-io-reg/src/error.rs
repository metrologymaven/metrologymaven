//! The closed reader error taxonomy (REQ-IO.REG.LIMITS-002, -004).

use crate::limits::LimitKind;
use fid_types::Determinism;
use std::fmt;
use std::io;

/// Where in the input something happened: a byte offset, plus a 1-based line and
/// column when the reader tracks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position {
    /// Zero-based byte offset from the start of the input.
    pub byte: u64,
    /// 1-based line, if known.
    pub line: Option<u32>,
    /// 1-based column (in bytes from the line start), if known.
    pub col: Option<u32>,
}

impl Position {
    /// A position known only by byte offset.
    pub const fn byte(byte: u64) -> Position {
        Position { byte, line: None, col: None }
    }

    /// A position with byte offset and 1-based line and column.
    pub const fn line_col(byte: u64, line: u32, col: u32) -> Position {
        Position { byte, line: Some(line), col: Some(col) }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "byte {}", self.byte)?;
        match (self.line, self.col) {
            (Some(l), Some(c)) => write!(f, " (line {l}, column {c})"),
            (Some(l), None) => write!(f, " (line {l})"),
            _ => Ok(()),
        }
    }
}

/// Every stable error code, in declaration order of [`IoError`].
pub const CODES: &[&str] = &[
    "limit_exceeded",
    "malformed",
    "truncated_file",
    "unit_required",
    "unit_conflict",
    "format_unknown",
    "format_ambiguous",
    "format_mismatch",
    "format_duplicate",
    "raw_path_forbidden",
    "path_outside_root",
    "root_not_found",
    "target_exists",
    "limit_raise_forbidden",
    "reader_crashed",
    "io",
];

/// The closed error set of every reader. Codes ([`IoError::code`]) are stable
/// snake_case strings shared by CLI, MCP and the journal. Free text never appears in an
/// error: the `reason` of [`IoError::Malformed`] is a static string chosen by the reader,
/// never text copied from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoError {
    /// A budget was charged past its limit. `value` is the configured limit in the
    /// unit of [`LimitKind::unit`]; `at` is the input position reached.
    LimitExceeded {
        /// The limit.
        limit: LimitKind,
        /// Its configured value.
        value: u64,
        /// Input position reached.
        at: Position,
    },
    /// The input breaks the format's grammar.
    Malformed {
        /// Where.
        at: Position,
        /// Static, reader-chosen reason code (never file content).
        reason: &'static str,
    },
    /// The input ends before the format says it may.
    TruncatedFile {
        /// Where the input ended.
        at: Position,
    },
    /// The format declares no length unit and the caller gave none.
    UnitRequired,
    /// The format declares a length unit, the caller gave a different one and did not set
    /// `override_unit` (`IO.REG.PROV`).
    UnitConflict,
    /// No registered format matches the content.
    FormatUnknown,
    /// More than one registered format matches the content.
    FormatAmbiguous,
    /// File extension and content signature name different formats.
    FormatMismatch,
    /// A format ID is registered twice.
    FormatDuplicate,
    /// An agent or script passed a raw path instead of a root handle.
    RawPathForbidden,
    /// A path resolved from a root handle leaves the root.
    PathOutsideRoot,
    /// A root handle names a root that is not configured for the requested direction.
    RootNotFound,
    /// The export target exists and `overwrite` is not set.
    TargetExists,
    /// An agent or script asked to raise a limit above its default.
    LimitRaiseForbidden {
        /// The limit.
        limit: LimitKind,
        /// The requested value.
        requested: u64,
        /// The default it may not exceed.
        default: u64,
    },
    /// An isolated (worker or plugin) reader died.
    ReaderCrashed,
    /// The underlying source failed.
    Io {
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// Input position reached.
        at: Position,
    },
}

impl IoError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            IoError::LimitExceeded { .. } => "limit_exceeded",
            IoError::Malformed { .. } => "malformed",
            IoError::TruncatedFile { .. } => "truncated_file",
            IoError::UnitRequired => "unit_required",
            IoError::UnitConflict => "unit_conflict",
            IoError::FormatUnknown => "format_unknown",
            IoError::FormatAmbiguous => "format_ambiguous",
            IoError::FormatMismatch => "format_mismatch",
            IoError::FormatDuplicate => "format_duplicate",
            IoError::RawPathForbidden => "raw_path_forbidden",
            IoError::PathOutsideRoot => "path_outside_root",
            IoError::RootNotFound => "root_not_found",
            IoError::TargetExists => "target_exists",
            IoError::LimitRaiseForbidden { .. } => "limit_raise_forbidden",
            IoError::ReaderCrashed => "reader_crashed",
            IoError::Io { .. } => "io",
        }
    }

    /// The input position, for the variants that carry one.
    pub fn position(&self) -> Option<Position> {
        match self {
            IoError::LimitExceeded { at, .. }
            | IoError::Malformed { at, .. }
            | IoError::TruncatedFile { at }
            | IoError::Io { at, .. } => Some(*at),
            _ => None,
        }
    }

    /// True for the variants that report malformed input (each carries a [`Position`]).
    pub fn is_malformed_input(&self) -> bool {
        matches!(self, IoError::Malformed { .. } | IoError::TruncatedFile { .. })
    }

    /// The determinism class of this outcome: `N` for wall-time expiry, `D0` otherwise.
    pub fn determinism(&self) -> Determinism {
        match self {
            IoError::LimitExceeded { limit: LimitKind::Wall, .. } => Determinism::N,
            _ => Determinism::D0,
        }
    }

    /// The same error with line and column filled in at its position, if it has one.
    pub fn with_line_col(self, line: u32, col: u32) -> IoError {
        let set = |at: Position| Position::line_col(at.byte, line, col);
        match self {
            IoError::LimitExceeded { limit, value, at } => IoError::LimitExceeded { limit, value, at: set(at) },
            IoError::Malformed { at, reason } => IoError::Malformed { at: set(at), reason },
            IoError::TruncatedFile { at } => IoError::TruncatedFile { at: set(at) },
            IoError::Io { kind, at } => IoError::Io { kind, at: set(at) },
            other => other,
        }
    }

    /// Recovers a reader error from a [`std::io::Error`]: an [`IoError`] carried inside
    /// (as [`IoError::into_io`] makes) comes back unchanged, an unexpected end of input
    /// becomes `truncated_file`, anything else becomes `io{kind}` at byte `at`.
    pub fn from_io(err: io::Error, at: u64) -> IoError {
        if let Some(inner) = err.get_ref().and_then(|e| e.downcast_ref::<IoError>()) {
            return inner.clone();
        }
        match err.kind() {
            io::ErrorKind::UnexpectedEof => IoError::TruncatedFile { at: Position::byte(at) },
            kind => IoError::Io { kind, at: Position::byte(at) },
        }
    }

    /// Wraps this error in a [`std::io::Error`] (for the [`std::io::Read`] impl), so that
    /// [`IoError::from_io`] can recover it.
    pub fn into_io(self) -> io::Error {
        let kind = match &self {
            IoError::Io { kind, .. } => *kind,
            IoError::TruncatedFile { .. } => io::ErrorKind::UnexpectedEof,
            IoError::Malformed { .. } => io::ErrorKind::InvalidData,
            _ => io::ErrorKind::Other,
        };
        io::Error::new(kind, self)
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            IoError::LimitExceeded { limit, value, at } => {
                write!(f, ": limit {limit} = {value} {} reached at {at}", limit.unit())
            }
            IoError::Malformed { at, reason } => write!(f, ": {reason} at {at}"),
            IoError::TruncatedFile { at } => write!(f, ": input ends at {at}"),
            IoError::LimitRaiseForbidden { limit, requested, default } => {
                write!(f, ": {limit} {requested} is above the default {default}; only a human may raise it")
            }
            IoError::Io { kind, at } => write!(f, ": {kind} at {at}"),
            _ => Ok(()),
        }
    }
}

impl std::error::Error for IoError {}
