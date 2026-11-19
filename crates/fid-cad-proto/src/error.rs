//! Protocol errors (REQ-IO.CAD.WORKER.PROTO-002, -003, -004).

use crate::msg::ProtoVersion;
use fid_io_reg::IoError;
use std::fmt;

/// The `reason` of a `reader_crashed` protocol failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CrashReason {
    /// A frame's declared length is above the per-frame cap, or the session total would
    /// pass the session cap.
    FrameTooLarge,
    /// A frame does not decode to a known kind, breaks the schema or fails validation.
    InvalidMessage,
    /// The worker speaks a different protocol major version.
    ProtocolMismatch,
    /// The stream ended inside a frame, or before the session's final message.
    UnexpectedEof,
}

impl CrashReason {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            CrashReason::FrameTooLarge => "frame_too_large",
            CrashReason::InvalidMessage => "invalid_message",
            CrashReason::ProtocolMismatch => "protocol_mismatch",
            CrashReason::UnexpectedEof => "unexpected_eof",
        }
    }
}

impl fmt::Display for CrashReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which byte cap a [`ProtoError::FrameTooLarge`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cap {
    /// The per-frame cap ([`crate::ProtoLimits::frame_max`]).
    Frame,
    /// The per-session cap ([`crate::ProtoLimits::session_max`]).
    Session,
}

impl Cap {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Cap::Frame => "frame",
            Cap::Session => "session",
        }
    }
}

/// Every way receiving a frame can fail. All variants except [`ProtoError::Harness`]
/// have the code `reader_crashed`; `Harness` carries a harness error (`limit_exceeded`
/// for a record-count, allocation, byte or wall-time limit, or `io` for a failed pipe).
///
/// Frame numbers are 0-based positions in the stream being read. No variant carries
/// text received from the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    /// `reader_crashed{reason: frame_too_large}`.
    FrameTooLarge {
        /// Frame number.
        frame: u64,
        /// The declared payload length.
        declared: u64,
        /// The cap that was hit.
        cap: Cap,
        /// The cap's value in bytes.
        limit: u64,
    },
    /// `reader_crashed{reason: invalid_message}`.
    InvalidMessage {
        /// Frame number.
        frame: u64,
        /// Static, decoder-chosen reason (never worker text).
        detail: &'static str,
    },
    /// `reader_crashed{reason: protocol_mismatch}`.
    ProtocolMismatch {
        /// The host's version.
        host: ProtoVersion,
        /// The version the worker reported.
        worker: ProtoVersion,
    },
    /// `reader_crashed{reason: unexpected_eof}`.
    UnexpectedEof {
        /// The frame that was being read or awaited.
        frame: u64,
    },
    /// A harness error (`limit_exceeded`, `io`).
    Harness(IoError),
}

impl ProtoError {
    /// The stable error code: `reader_crashed`, or the harness error's code.
    pub fn code(&self) -> &'static str {
        match self {
            ProtoError::Harness(e) => e.code(),
            _ => "reader_crashed",
        }
    }

    /// The crash reason, for `reader_crashed` errors.
    pub fn reason(&self) -> Option<CrashReason> {
        match self {
            ProtoError::FrameTooLarge { .. } => Some(CrashReason::FrameTooLarge),
            ProtoError::InvalidMessage { .. } => Some(CrashReason::InvalidMessage),
            ProtoError::ProtocolMismatch { .. } => Some(CrashReason::ProtocolMismatch),
            ProtoError::UnexpectedEof { .. } => Some(CrashReason::UnexpectedEof),
            ProtoError::Harness(_) => None,
        }
    }

    /// The frame number, where the error has one.
    pub fn frame(&self) -> Option<u64> {
        match self {
            ProtoError::FrameTooLarge { frame, .. }
            | ProtoError::InvalidMessage { frame, .. }
            | ProtoError::UnexpectedEof { frame } => Some(*frame),
            _ => None,
        }
    }

    /// The error in the shared reader taxonomy: `reader_crashed`, or the harness error.
    pub fn to_io_error(&self) -> IoError {
        match self {
            ProtoError::Harness(e) => e.clone(),
            _ => IoError::ReaderCrashed,
        }
    }
}

impl From<IoError> for ProtoError {
    fn from(e: IoError) -> Self {
        ProtoError::Harness(e)
    }
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::FrameTooLarge { frame, declared, cap, limit } => write!(
                f,
                "reader_crashed{{reason: frame_too_large, frame: {frame}, declared: {declared}, cap: {}, limit: {limit}}}",
                cap.as_str()
            ),
            ProtoError::InvalidMessage { frame, detail } => {
                write!(f, "reader_crashed{{reason: invalid_message, frame: {frame}, detail: {detail}}}")
            }
            ProtoError::ProtocolMismatch { host, worker } => write!(
                f,
                "reader_crashed{{reason: protocol_mismatch, host: \"{}\", worker: \"{}\"}}",
                host.major_label(),
                worker.major_label()
            ),
            ProtoError::UnexpectedEof { frame } => write!(f, "reader_crashed{{reason: unexpected_eof, frame: {frame}}}"),
            ProtoError::Harness(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProtoError {}

/// Why a message could not be encoded. The decoder would reject such a message, so the
/// encoder refuses it instead of writing it (REQ-IO.CAD.WORKER.PROTO-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// A length or count does not fit the wire's `u32`.
    TooLong,
    /// A float is NaN or infinite.
    NonFinite,
    /// `Progress` with `done` above `total`.
    ProgressPastTotal,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EncodeError::TooLong => "too_long",
            EncodeError::NonFinite => "non_finite_float",
            EncodeError::ProgressPastTotal => "progress_past_total",
        })
    }
}

impl std::error::Error for EncodeError {}
