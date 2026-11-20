//! Stream routing and the P0 Activity Log. Spec node `CORE.JRN.STREAM.ALOG`.
//!
//! **Routing** ([`route`], D0). A total `match` on the side-effect class:
//!
//! - W1, W2, W3, X1, I1, D1, M1, S1 and A0 commit to the Record Journal (REQ-001; C4 and
//!   D-031 exempt only reads);
//! - R0 and R1 by an external principal (a self-hosted server channel) go to the Access
//!   Ledger when one exists, else to no stream (REQ-003);
//! - R0 and R1 by a local principal go to the Activity Log while a recording session is
//!   active or when the call is a Command Program step, else to no stream (REQ-002). A read
//!   never goes to the Record Journal.
//!
//! **Recording flag** ([`RecordingFlag`]). The recording-session owner (`VIZ.CAPTURE.REC`, a
//! higher layer) sets it with [`RecordingFlag::set_recording`]; the router only reads it.
//!
//! **Activity Log** ([`ActivityLog`]). Record Journal entry headers with stream `activity`,
//! one canonical JSON line per record, without previous-hash links (every `prev` is
//! [`crate::GENESIS_PREV`]), rotating by size (REQ-005, REQ-006). Job lifecycle changes are
//! written whether or not a recording session is active (REQ-004): the `LifecycleSink`
//! implementation lives in `fid-jobs` (layer engine, above this crate) and calls
//! [`ActivityLog::record_lifecycle`]. A write that fails is a warning
//! ([`ActivityWarning`], code `activity_log_unavailable`) and never fails the call (REQ-008).
//!
//! **No rewrite API (REQ-007).** This module, like the rest of the crate, offers no operation
//! that compacts, rewrites, reorders or deletes Record Journal or Access Ledger entries. The
//! only file it ever renames or deletes is one of its own Activity Log files.
//!
//! With feature `store`, [`activity_settings`] declares the two Activity Log settings
//! (`CORE.STO.SET`) and [`ActivityLogConfig::from_resolved`] reads them.

mod activity;
#[cfg(feature = "store")]
mod settings;

pub use activity::{
    format_utc, rotated_file_name, ActivityLog, ActivityLogConfig, ActivityWarning, JobChange, SystemUtc, UtcClock,
    ACTIVITY_STREAM, CURRENT_FILE, DEFAULT_KEEP, DEFAULT_MAX_BYTES, LIFECYCLE_BRANCH, LIFECYCLE_COMMAND,
    LIFECYCLE_IDENTITY, MAX_LINE_BYTES, UNAVAILABLE,
};
#[cfg(feature = "store")]
pub use settings::{activity_settings, KEEP_KEY, MAX_BYTES_KEY};

use crate::entry::{DraftBody, EntryDraft};
use fid_canon::Digest;
use fid_types::SideEffect;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// The three journal streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamId {
    /// The hash-chained Record Journal.
    Record,
    /// The local, unchained Activity Log.
    Activity,
    /// The Access Ledger of external reads (format and implementation P1b).
    AccessLedger,
}

impl StreamId {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            StreamId::Record => "record",
            StreamId::Activity => "activity",
            StreamId::AccessLedger => "access_ledger",
        }
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the calling principal sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Locality {
    /// A principal of this installation (GUI, CLI, local MCP, script, local agent).
    Local,
    /// An external principal reaching a self-hosted server channel.
    External,
}

impl Locality {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Locality::Local => "local",
            Locality::External => "external",
        }
    }
}

impl fmt::Display for Locality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A reference to the Command Program step a call is part of (D-031).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramStepRef {
    /// The program's stable ID.
    pub program: String,
    /// The step's stable ID within the program.
    pub step: String,
}

/// What the router needs to know besides the class and the principal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteContext {
    /// A recording session is active.
    pub recording: bool,
    /// The call is a step of a Command Program.
    pub program_step: Option<ProgramStepRef>,
    /// An Access Ledger exists for this installation.
    pub access_ledger: bool,
}

/// The stream a completed call belongs to, or `None` when it is not stored at all
/// (REQ-001..-003). D0: a pure function of its arguments.
pub fn route(class: SideEffect, principal: Locality, ctx: &RouteContext) -> Option<StreamId> {
    match class {
        SideEffect::W1
        | SideEffect::W2
        | SideEffect::W3
        | SideEffect::X1
        | SideEffect::I1
        | SideEffect::D1
        | SideEffect::M1
        | SideEffect::S1
        | SideEffect::A0 => Some(StreamId::Record),
        SideEffect::R0 | SideEffect::R1 => match principal {
            Locality::External => ctx.access_ledger.then_some(StreamId::AccessLedger),
            Locality::Local => (ctx.recording || ctx.program_step.is_some()).then_some(StreamId::Activity),
        },
    }
}

/// The recording flag, set by the recording-session owner and read by the router.
#[derive(Debug, Default)]
pub struct RecordingFlag {
    active: AtomicBool,
    id: Mutex<Option<String>>,
}

impl RecordingFlag {
    /// A flag that is off.
    pub fn new() -> RecordingFlag {
        RecordingFlag::default()
    }

    /// Turns recording on (with the recording session's ID) or off. Turning it off clears the ID.
    pub fn set_recording(&self, active: bool, recording_id: Option<&str>) {
        let mut id = self.id.lock().unwrap_or_else(PoisonError::into_inner);
        *id = if active { recording_id.map(str::to_string) } else { None };
        self.active.store(active, Ordering::SeqCst);
    }

    /// Whether a recording session is active.
    pub fn is_recording(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// The active recording session's ID, if one was given.
    pub fn recording_id(&self) -> Option<String> {
        if !self.is_recording() {
            return None;
        }
        self.id.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// What happened to one completed call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallOutcome {
    /// The stream the call belongs to. [`StreamId::Record`] and [`StreamId::AccessLedger`] are
    /// returned for the caller to append through their own writers (`CORE.JRN.CHAIN.APPEND`,
    /// `CORE.JRN.STREAM.LEDGER`); this module writes only the Activity Log.
    pub stream: Option<StreamId>,
    /// The Activity Log line's entry hash, when one was written.
    pub written: Option<Digest>,
    /// Set when the Activity Log could not be written; the call itself still succeeds.
    pub warning: Option<ActivityWarning>,
}

/// Why [`Streams::complete_call`] refused a draft (a caller error, not a log failure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The draft is not a `command` entry, so it is not a call.
    NotACommand,
}

impl CallError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            CallError::NotACommand => "not_a_command",
        }
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CallError {}

/// The router wired to the recording flag and the Activity Log.
#[derive(Debug, Clone)]
pub struct Streams {
    recording: Arc<RecordingFlag>,
    activity: Arc<ActivityLog>,
    access_ledger: bool,
}

impl Streams {
    /// Streams with recording off and no Access Ledger.
    pub fn new(activity: Arc<ActivityLog>) -> Streams {
        Streams { recording: Arc::new(RecordingFlag::new()), activity, access_ledger: false }
    }

    /// The same, sharing an existing recording flag.
    pub fn with_flag(activity: Arc<ActivityLog>, recording: Arc<RecordingFlag>) -> Streams {
        Streams { recording, activity, access_ledger: false }
    }

    /// Declares whether an Access Ledger exists (P1b; none in P0).
    pub fn set_access_ledger(&mut self, exists: bool) {
        self.access_ledger = exists;
    }

    /// The recording flag.
    pub fn recording(&self) -> &Arc<RecordingFlag> {
        &self.recording
    }

    /// The Activity Log.
    pub fn activity(&self) -> &Arc<ActivityLog> {
        &self.activity
    }

    /// Shorthand for [`RecordingFlag::set_recording`].
    pub fn set_recording(&self, active: bool, recording_id: Option<&str>) {
        self.recording.set_recording(active, recording_id);
    }

    /// The route context as of now.
    pub fn context(&self, program_step: Option<&ProgramStepRef>) -> RouteContext {
        RouteContext {
            recording: self.recording.is_recording(),
            program_step: program_step.cloned(),
            access_ledger: self.access_ledger,
        }
    }

    /// [`route`] with the current context.
    pub fn route(
        &self,
        class: SideEffect,
        principal: Locality,
        program_step: Option<&ProgramStepRef>,
    ) -> Option<StreamId> {
        route(class, principal, &self.context(program_step))
    }

    /// Routes a completed call by its draft's class and writes it to the Activity Log when it
    /// belongs there. A failed write is reported in [`CallOutcome::warning`], never as an
    /// error (REQ-008). Record Journal and Access Ledger calls are returned unwritten.
    pub fn complete_call(
        &self,
        principal: Locality,
        program_step: Option<&ProgramStepRef>,
        draft: &EntryDraft,
    ) -> Result<CallOutcome, CallError> {
        let DraftBody::Command(c) = &draft.body else { return Err(CallError::NotACommand) };
        let stream = self.route(c.class, principal, program_step);
        let mut out = CallOutcome { stream, written: None, warning: None };
        if stream == Some(StreamId::Activity) {
            match self.activity.append(draft) {
                Ok(hash) => out.written = Some(hash),
                Err(w) => out.warning = Some(w),
            }
        }
        Ok(out)
    }
}
