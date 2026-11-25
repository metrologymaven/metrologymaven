//! The in-process bus transport and the bus-error envelope table. Spec node `AUTO.SURF.INPROC`.
//!
//! [`InProcTransport`] is the [`BusTransport`] a surface uses when the bus runs in its own
//! process. It turns each tracker [`Purpose::Submit`] request into exactly one
//! [`BusPort::call`] and each [`ApiReply`] into the shared [`Envelope`], through the pure
//! [`envelope_for`]. That function also owns the one table from bus error codes to envelope
//! error codes ([`BUS_ERROR_TABLE`]), so the CLI, MCP and Python report the same bus error the
//! same way.
//!
//! **Identity.** The transport adds nothing about who is calling. It reports the port's
//! [`BusPort::principal_kind`] and [`BusPort::channel`] unchanged and forwards
//! [`BusPort::confirm_human`]; there is no operation that sets a principal kind, an identity or
//! a session, and the port itself is never handed out. Whether a call runs as `human` is the
//! bus's own derivation for the port's channel (CORE.BUS.PRIN), never the transport's.
//!
//! **Calls.** A submit runs synchronously inside [`BusTransport::send`] and then queues
//! [`BusEvent::Accepted`] and [`BusEvent::Finished`] under one fresh UUIDv7 call ID from the
//! transport's [`KeySource`]. The call ID names no `CORE.JOB` job, so it is never written into
//! the envelope (`job_id` absent). The tracker's idempotency key is not passed to the bus: an
//! in-process call either runs inside `send` or `send` returns [`SendError::NotSent`], so it is
//! never resent. `Cancel` and `Wait` requests are refused with [`NO_LIVE_JOB`], because no
//! in-process call outlives `send`.
//!
//! **Dry runs.** [`BusRequest`] has no mode, so every call sends `dry_run: false`. An
//! [`ApiReply::DryRun`] always becomes outcome `dry_run` with no journal entry hash, never
//! `committed` or `computed` (D-045).
//!
//! **Hidden commands (D-035).** A failure with bus code `hidden_command`, from any stage, is
//! reported byte for byte as the bus's own refusal of a major its table does not hold:
//! `not_found` with message `"<name>"@<major>: unknown major` (the detail of
//! `fid_bus::validate::CommandTable::resolve`) and details `bus_code: not_found`,
//! `bus_reason: unknown_major`, `bus_origin: pipeline` ([`hidden_as_unknown_major`]). So no
//! surface, filtering or not, can tell a lifecycle-hidden major from a missing one, and
//! `AUTO.CLI.DISPATCH` still counts it as absent (maintainer decision 2026-11-25).
//!
//! Determinism N (call IDs); [`envelope_for`] is D0.

#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]

use crate::jobs::{BusEvent, BusRequest, BusTransport, KeySource, Purpose, SendError};
use crate::{Envelope, ErrorCode, Outcome};
use fid_bus_api::{ApiError, ApiReply, ApiStage, BusCall, BusPort, ChannelInfo, PrincipalKind};
use fid_canon::CValue;
use fid_types::CommandId;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// The refusal reason of a `Cancel` or `Wait` request (REQ-AUTO.SURF.INPROC-008).
pub const NO_LIVE_JOB: &str = "no_live_job";
/// The bus code that becomes outcome `cancelled` with no error (REQ-AUTO.SURF.INPROC-004).
pub const CANCELLED: &str = "cancelled";
/// `bus_origin` for errors the bus pipeline's resolution and validation raised.
pub const ORIGIN_PIPELINE: &str = "pipeline";
/// `bus_origin` for errors a command handler returned.
pub const ORIGIN_HANDLER: &str = "handler";
/// `bus_origin` for every other stage, `session` included.
pub const ORIGIN_BUS: &str = "bus";
/// Events the transport holds before [`BusTransport::send`] refuses with
/// [`SendError::NotSent`] (without calling the port) until the tracker drains them.
pub const MAX_QUEUED_EVENTS: usize = 65_536;

/// The bus-code to envelope-code table of spec node AUTO.SURF.INPROC, as
/// (bus code, bus reason or `None` for any reason, envelope code). The first matching row
/// wins, so each code's specific reasons come before its catch-all. Bus code
/// [`CANCELLED`] is not here: it is an outcome, not an error. A code not in this table becomes
/// `internal_error` (REQ-AUTO.SURF.INPROC-007).
pub const BUS_ERROR_TABLE: &[(&str, Option<&str>, ErrorCode)] = &[
    ("validation", Some("unit_mismatch"), ErrorCode::UnitError),
    ("validation", None, ErrorCode::ValidationError),
    ("policy", Some("raw_path_refused"), ErrorCode::RawPathRefused),
    ("policy", Some("outside_root"), ErrorCode::RootEscape),
    ("policy", Some("bad_handle"), ErrorCode::ValidationError),
    ("policy", None, ErrorCode::PolicyRefused),
    ("precondition", None, ErrorCode::PreconditionFailed),
    ("conflict", None, ErrorCode::Conflict),
    ("idempotency_mismatch", None, ErrorCode::Conflict),
    ("not_found", None, ErrorCode::NotFound),
    // Never reached through `envelope_for`, which reports `hidden_command` as the unknown-major
    // refusal first (D-035); kept so the table covers every bus code.
    ("hidden_command", None, ErrorCode::NotFound),
    ("unauthenticated", None, ErrorCode::PolicyRefused),
    ("upgrade_refused", None, ErrorCode::PolicyRefused),
    ("undeclared_effect", None, ErrorCode::InternalError),
    ("internal", None, ErrorCode::InternalError),
];

/// The envelope error code for a bus failure with `code` and `reason`: the first matching row
/// of [`BUS_ERROR_TABLE`], else `internal_error` (REQ-AUTO.SURF.INPROC-005, -007).
pub fn error_code_for(code: &str, reason: &str) -> ErrorCode {
    BUS_ERROR_TABLE
        .iter()
        .find(|(c, r, _)| {
            *c == code
                && match r {
                    None => true,
                    Some(r) => *r == reason,
                }
        })
        .map_or(ErrorCode::InternalError, |(_, _, e)| e.clone())
}

/// The `bus_origin` of an error raised at `stage` (REQ-AUTO.SURF.INPROC-006): `pipeline` for
/// `resolve` and `validate`, `handler` for `handler`, `bus` for every other stage.
pub const fn bus_origin(stage: ApiStage) -> &'static str {
    match stage {
        ApiStage::Resolve | ApiStage::Validate => ORIGIN_PIPELINE,
        ApiStage::Handler => ORIGIN_HANDLER,
        ApiStage::Session
        | ApiStage::Authorise
        | ApiStage::Precondition
        | ApiStage::Route
        | ApiStage::Effects
        | ApiStage::Commit => ORIGIN_BUS,
    }
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

/// The final envelope for `reply` to a call of `command` (REQ-AUTO.SURF.INPROC-003 .. -007).
/// Pure and D0, so every transport that receives an [`ApiReply`] produces the same bytes.
///
/// - `Committed` is `committed` with `journal_entry_hash`, `Computed` is `computed`,
///   `RequestFiled` is `request_filed` with `request_id`, `DryRun` is `dry_run`; each carries
///   the reply's result, if any;
/// - a failure with bus code `cancelled` is outcome `cancelled` with no error;
/// - a failure with bus code `hidden_command`, from any stage, is [`hidden_as_unknown_major`]
///   (D-035);
/// - any other failure is `failed` with the code [`error_code_for`] gives, the bus detail as
///   the message and the details `bus_code`, `bus_reason` and `bus_origin`.
///
/// `job_id` is never set. A `RequestFiled` reply with an empty request ID (which no bus of this
/// build sends) cannot be a valid `request_filed` envelope; it is reported as `failed` with
/// `internal_error` and no details, so it is never mistaken for a bus error code.
pub fn envelope_for(command: &CommandId, reply: &ApiReply) -> Envelope {
    let id = command.as_str();
    match reply {
        ApiReply::Committed { entry, result } => Envelope {
            journal_entry_hash: Some(entry.as_str().to_string()),
            result: result.clone(),
            ..Envelope::bare(id, Outcome::Committed)
        },
        ApiReply::Computed { result } => Envelope { result: result.clone(), ..Envelope::bare(id, Outcome::Computed) },
        ApiReply::RequestFiled { request_id } if request_id.is_empty() => {
            Envelope::failed(id, ErrorCode::InternalError, "the bus filed a request with no request ID", None)
        }
        ApiReply::RequestFiled { request_id } => {
            Envelope { request_id: Some(request_id.clone()), ..Envelope::bare(id, Outcome::RequestFiled) }
        }
        ApiReply::DryRun { result } => Envelope { result: Some(result.clone()), ..Envelope::bare(id, Outcome::DryRun) },
        ApiReply::Failed(e) => failed_envelope(command, e),
    }
}

/// The bus code that is reported as the unknown-major refusal (D-035).
pub const HIDDEN_COMMAND: &str = "hidden_command";
/// Longest name the bus shows in a resolve refusal (`fid_bus::validate::CommandTable::resolve`).
const SHOWN_NAME_CHARS: usize = 128;

/// The envelope of a `hidden_command` failure for `command` (D-035): exactly the envelope of
/// the bus's `not_found/unknown_major` refusal of `command` at stage `resolve`, whose detail is
/// `"<name>"@<major>: unknown major` with the name shown as a Rust debug string, cut to 128
/// characters, as `fid_bus::validate::CommandTable::resolve` writes it. The hidden error's own
/// reason, detail and stage are dropped.
pub fn hidden_as_unknown_major(command: &CommandId) -> Envelope {
    let shown: String = command.name().chars().take(SHOWN_NAME_CHARS).collect();
    let message = format!("{shown:?}@{}: unknown major", command.major());
    failed_body(command.as_str(), "not_found", "unknown_major", ApiStage::Resolve, &message)
}

fn failed_envelope(command: &CommandId, e: &ApiError) -> Envelope {
    let id = command.as_str();
    if e.code() == CANCELLED {
        return Envelope::bare(id, Outcome::Cancelled);
    }
    if e.code() == HIDDEN_COMMAND {
        return hidden_as_unknown_major(command);
    }
    failed_body(id, e.code(), e.reason(), e.stage(), e.detail())
}

fn failed_body(id: &str, code: &str, reason: &str, stage: ApiStage, message: &str) -> Envelope {
    let mut details = BTreeMap::new();
    details.insert("bus_code".to_string(), text(code));
    details.insert("bus_reason".to_string(), text(reason));
    details.insert("bus_origin".to_string(), text(bus_origin(stage)));
    Envelope::failed(id, error_code_for(code, reason), message, Some(CValue::Obj(details)))
}

/// The [`BusTransport`] over one in-process [`BusPort`] (module docs).
pub struct InProcTransport {
    port: Box<dyn BusPort + Send>,
    queue: VecDeque<BusEvent>,
    keys: Box<dyn KeySource + Send>,
    closed: bool,
}

impl InProcTransport {
    /// A transport over `port` whose call IDs come from `keys` (a UUIDv7 source such as
    /// [`crate::jobs::SystemKeySource`]).
    pub fn new(port: Box<dyn BusPort + Send>, keys: Box<dyn KeySource + Send>) -> InProcTransport {
        InProcTransport { port, queue: VecDeque::new(), keys, closed: false }
    }

    /// The principal kind the bus derives for the port's channel now, unchanged.
    pub fn principal_kind(&self) -> PrincipalKind {
        self.port.principal_kind()
    }

    /// The port's channel, unchanged.
    pub fn channel(&self) -> ChannelInfo {
        self.port.channel()
    }

    /// Asks the bus to confirm a person for the port's session; forwarded to the port once,
    /// with nothing added. Who is asked, and whether anyone is, is the bus's choice.
    pub fn confirm_human(&mut self) -> Result<(), ApiError> {
        self.port.confirm_human()
    }

    /// Closes the transport: every later [`BusTransport::send`] returns
    /// [`SendError::NotSent`] and calls nothing. Events already queued can still be read.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Whether [`InProcTransport::close`] was called.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Events queued and not yet read.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}

impl fmt::Debug for InProcTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcTransport")
            .field("queued", &self.queue.len())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl BusTransport for InProcTransport {
    /// REQ-AUTO.SURF.INPROC-001, -002, -008, -009.
    fn send(&mut self, request: &BusRequest) -> Result<(), SendError> {
        let events = match request.purpose {
            Purpose::Submit => 2,
            Purpose::Cancel | Purpose::Wait => 1,
        };
        if self.closed || self.queue.len().saturating_add(events) > MAX_QUEUED_EVENTS {
            return Err(SendError::NotSent);
        }
        match request.purpose {
            Purpose::Submit => {
                let call = BusCall { command: request.command.clone(), params: request.params.clone(), dry_run: false };
                let reply = self.port.call(&call);
                let envelope = envelope_for(&request.command, &reply);
                let job = self.keys.next_key();
                self.queue.push_back(BusEvent::Accepted { key: request.key, job });
                self.queue.push_back(BusEvent::Finished { key: request.key, job, envelope: Box::new(envelope) });
            }
            Purpose::Cancel | Purpose::Wait => {
                self.queue.push_back(BusEvent::Refused {
                    key: request.key,
                    attempt: request.attempt,
                    reason: NO_LIVE_JOB.to_string(),
                });
            }
        }
        Ok(())
    }

    fn recv(&mut self) -> Option<BusEvent> {
        self.queue.pop_front()
    }
}
