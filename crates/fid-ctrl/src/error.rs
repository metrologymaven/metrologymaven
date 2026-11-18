//! Typed control-channel transport errors with stable codes.

use std::fmt;

/// Wire-level error codes (spec node AUTO.CTRL.WIRE, Design: Errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireError {
    /// The configured address is not `127.0.0.1` or `::1` (REQ-AUTO.CTRL.WIRE-001).
    NonLoopback,
    /// A line exceeded its bound (REQ-AUTO.CTRL.WIRE-003).
    MessageTooLarge,
    /// A line is not valid JSON or does not match the schema (REQ-AUTO.CTRL.WIRE-004).
    MalformedMessage,
    /// No common protocol version (REQ-AUTO.CTRL.WIRE-005).
    VersionUnsupported,
    /// The connection limit is reached (REQ-AUTO.CTRL.WIRE-006).
    TooManyConnections,
    /// A line did not complete within its deadline, or the connection sat idle too long.
    /// Not in the spec's error list: the stall bound this crate adds (see the PR).
    ReadTimeout,
}

impl WireError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            WireError::NonLoopback => "ctrl_non_loopback",
            WireError::MessageTooLarge => "message_too_large",
            WireError::MalformedMessage => "malformed_message",
            WireError::VersionUnsupported => "version_unsupported",
            WireError::TooManyConnections => "too_many_connections",
            WireError::ReadTimeout => "read_timeout",
        }
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for WireError {}

/// Why the server could not start.
#[derive(Debug)]
pub enum StartError {
    /// The configured address is not loopback; no socket was bound.
    NonLoopback(String),
    /// Binding or writing the discovery file failed.
    Io(std::io::Error),
}

impl StartError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            StartError::NonLoopback(_) => WireError::NonLoopback.code(),
            StartError::Io(_) => "ctrl_io",
        }
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::NonLoopback(a) => write!(f, "ctrl_non_loopback: {a:?}"),
            StartError::Io(e) => write!(f, "ctrl_io: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

impl From<std::io::Error> for StartError {
    fn from(e: std::io::Error) -> Self {
        StartError::Io(e)
    }
}
