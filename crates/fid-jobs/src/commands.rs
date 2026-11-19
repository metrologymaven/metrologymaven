//! Job commands and the job-status contract (spec node `CORE.JOB.CMD`).
//!
//! Four registry commands let every surface see and control jobs:
//!
//! | Command | Class | Determinism | Idempotency | Undo |
//! |---|---|---|---|---|
//! | `job.list_jobs@1` | R0 | N | natural | not_undoable |
//! | `job.show_status@1` | R0 | N | natural | not_undoable |
//! | `job.wait_job@1` | R0 | N | natural | not_undoable |
//! | `job.cancel_job@1` | W1 | N | natural | not_undoable |
//!
//! [`command_defs`] declares them as `fid-registry` [`CommandDef`]s; the CLI (`fid job ...`)
//! and MCP tools (`job_*`) are generated from those records. [`JobService`] holds the thin
//! handlers over the [`Jobs`] scheduler:
//!
//! - every result is a [`JobStatus`] in the published `job-status.schema.json` shape
//!   ([`job_status_schema`]);
//! - `list_jobs` returns the caller's own jobs, or every job for a human principal, sorted by
//!   submission time and then job ID, optionally filtered by state (REQ-003);
//! - `wait_job` returns at the terminal state, or at the timeout with `timed_out: true`
//!   (REQ-005), and returns `cancelled/requested` within [`crate::WAIT_CANCEL_POLL`] once the
//!   caller's own call token is cancelled;
//! - `cancel_job` runs only when the caller is the job's own principal or a human: the
//!   precondition is an FPL expression in the definition ([`CANCEL_PRECONDITION`]), compiled
//!   and evaluated with the registry's evaluator, and refused as `policy/not_job_owner`
//!   (REQ-002). It reports a journal commit only when the scheduler says, atomically under
//!   its own lock, that this call newly requested the cancellation
//!   ([`Jobs::cancel_request`]); a job already terminal (REQ-007) or already asked to cancel
//!   is returned unchanged with nothing to commit, so a repeated call has no further effect
//!   (`natural`);
//! - an unknown job ID gives `not_found/unknown_job` for each command (REQ-006);
//! - timestamps are wall-clock UTC in RFC 3339 form with milliseconds; ordering never uses
//!   them, only the scheduler's monotonic submission time and then the job ID.
//!
//! Fail closed: parameters are read from at most [`MAX_PARAMS_BYTES`] bytes, unknown fields
//! and duplicate keys are refused, the wait timeout is bounded by [`MAX_WAIT_TIMEOUT`],
//! principal IDs must already be lowercase (no case folding), a job with no recorded owner is
//! visible and cancellable only by a human, and `show_status` and `wait_job` apply the same
//! owner-or-human rule as `cancel_job`.
//!
//! The handlers never write the Record Journal: [`CancelOutcome::commit`] tells the bus what
//! to commit. Lock order is `owners` -> the scheduler's own lock; nothing here takes
//! `owners` while the scheduler's lock is held.

use crate::progress::CancelToken;
use crate::sched::{Execution, JobErrorCode, JobId, JobOpts, JobRecord, JobState, Jobs, PrincipalKind};
use fid_bus::err::{
    CancelledReason, InternalReason, NotFoundReason, PolicyReason, PreconditionReason, ValidationReason,
};
use fid_bus::{BusError, Detail};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_registry::{
    compile_command, evaluate, CommandDef, ContextTypes, DefError, FieldPath, PredContext, PredError, Predicate, Type,
    Value, Verdict,
};
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// `job.list_jobs@1` (R0).
pub const LIST_JOBS: &str = "job.list_jobs@1";
/// `job.show_status@1` (R0).
pub const SHOW_STATUS: &str = "job.show_status@1";
/// `job.wait_job@1` (R0).
pub const WAIT_JOB: &str = "job.wait_job@1";
/// `job.cancel_job@1` (W1). Also the journal event type it commits.
pub const CANCEL_JOB: &str = "job.cancel_job@1";
/// The four commands, in declaration order.
pub const COMMANDS: [&str; 4] = [LIST_JOBS, SHOW_STATUS, WAIT_JOB, CANCEL_JOB];

/// Default `timeout` of `job.wait_job@1`.
pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest accepted `timeout` of `job.wait_job@1`.
pub const MAX_WAIT_TIMEOUT: Duration = Duration::from_secs(3600);
/// Largest accepted parameter document, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4096;
/// Deepest accepted nesting of a parameter document.
pub const MAX_PARAMS_DEPTH: usize = 4;
/// Longest principal ID, in bytes (same bound as the roster's principal identities, #165).
pub const MAX_PRINCIPAL_ID_BYTES: usize = 128;

/// The cancel precondition, in FPL: the caller is a human, or the caller is the job's own
/// principal (same kind and ID). Same rule as the leaf's
/// `job.principal == caller || caller.kind == "human"`, with the human test first so a job
/// with no recorded owner (a read of `job.principal` fails) is still open to a human.
pub const CANCEL_PRECONDITION: &str =
    "caller.kind == \"human\" || (job.principal.kind == caller.kind && job.principal.id == caller.id)";
/// Step bound of [`CANCEL_PRECONDITION`].
pub const CANCEL_PRECONDITION_BOUND: u32 = 64;
/// Message key of [`CANCEL_PRECONDITION`] when it is false.
pub const NOT_JOB_OWNER_KEY: &str = "job.not_job_owner";

/// JSON Schema pattern of a timestamp: RFC 3339 UTC with exactly three fractional digits.
pub const TIMESTAMP_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}[.][0-9]{3}Z$";
/// Latest instant [`format_rfc3339`] renders, in ms since the epoch: 9999-12-31T23:59:59.999Z.
const MAX_RFC3339_MILLIS: u64 = 253_402_300_799_999;

/// JSON Schema pattern of a job ID: a lowercase hyphenated UUID.
pub const JOB_ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

const LIFECYCLE_SINCE: &str = "2026-11-18";
const JOB_STATES: [JobState; 5] =
    [JobState::Queued, JobState::Running, JobState::Succeeded, JobState::Failed, JobState::Cancelled];
const JOB_ERROR_CODES: [JobErrorCode; 5] = [
    JobErrorCode::Timeout,
    JobErrorCode::Cancelled,
    JobErrorCode::HandlerFailed,
    JobErrorCode::NotFound,
    JobErrorCode::QueueFull,
];
/// Owner-table size below which no sweep runs.
const MIN_OWNER_SWEEP: usize = 1024;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------ errors

fn validation(reason: ValidationReason, detail: &str) -> BusError {
    BusError::Validation(reason, Detail::new(detail))
}

fn unknown_job(id: JobId) -> BusError {
    BusError::NotFound(NotFoundReason::UnknownJob, Detail::new(&format!("no job {id}")))
}

fn not_job_owner(id: JobId) -> BusError {
    BusError::Policy(
        PolicyReason::NotJobOwner,
        Detail::new(&format!("job {id} belongs to another principal; only its own principal or a human may use it")),
    )
}

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

/// Maps a scheduler error on a lookup of `id`.
fn sched_error(id: JobId, e: JobErrorCode) -> BusError {
    match e {
        JobErrorCode::NotFound => unknown_job(id),
        other => invariant(&format!("scheduler refused a lookup of job {id}: {other}")),
    }
}

fn canon_error(e: CanonError) -> BusError {
    match e {
        CanonError::InputLimit => validation(ValidationReason::InputLimit, "parameters exceed the size or depth limit"),
        other => validation(ValidationReason::Schema, &format!("parameters are not valid JSON: {}", other.code())),
    }
}

// ------------------------------------------------------------------ principals

/// A principal ID as the bus derived it from the channel: 1..=[`MAX_PRINCIPAL_ID_BYTES`]
/// printable ASCII characters with no spaces and no upper-case letters. As decided for the
/// roster's principal identities (#165), an ID that is not already lowercase is refused
/// rather than folded, so case variants of one identity can never pass as separate
/// principals; IDs then compare exactly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalId(String);

impl PrincipalId {
    /// Parses a principal ID; anything else is `validation/name`.
    pub fn parse(s: &str) -> Result<PrincipalId, BusError> {
        let ok = !s.is_empty()
            && s.len() <= MAX_PRINCIPAL_ID_BYTES
            && s.bytes().all(|b| b.is_ascii_graphic() && !b.is_ascii_uppercase());
        if !ok {
            return Err(validation(
                ValidationReason::Name,
                "principal ID must be 1..=128 printable ASCII characters, no spaces, no upper case",
            ));
        }
        Ok(PrincipalId(s.to_string()))
    }

    /// The ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The principal calling a job command, or owning a job.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Caller {
    pub kind: PrincipalKind,
    pub id: PrincipalId,
}

impl Caller {
    pub fn new(kind: PrincipalKind, id: PrincipalId) -> Caller {
        Caller { kind, id }
    }
}

/// FPL context for the owner-or-human rule: `caller.kind`, `caller.id`,
/// `job.principal.kind`, `job.principal.id`. A job with no recorded owner has no
/// `job.principal` fields.
struct RuleContext<'a> {
    caller: &'a Caller,
    owner: Option<&'a Caller>,
}

impl PredContext for RuleContext<'_> {
    fn get(&self, path: &FieldPath) -> Result<Value, PredError> {
        let missing = || PredError::MissingField { path: path.to_string() };
        let parts: Vec<&str> = path.0.iter().map(String::as_str).collect();
        match parts.as_slice() {
            ["caller", "kind"] => Ok(Value::Str(self.caller.kind.as_str().to_string())),
            ["caller", "id"] => Ok(Value::Str(self.caller.id.as_str().to_string())),
            ["job", "principal", "kind"] => {
                self.owner.map(|o| Value::Str(o.kind.as_str().to_string())).ok_or_else(missing)
            }
            ["job", "principal", "id"] => self.owner.map(|o| Value::Str(o.id.as_str().to_string())).ok_or_else(missing),
            _ => Err(missing()),
        }
    }
}

/// Types of the non-`params` context roots that job command preconditions read.
pub fn context_types() -> ContextTypes {
    ContextTypes::new()
        .with("caller", Type::Map)
        .with("caller.kind", Type::String)
        .with("caller.id", Type::String)
        .with("job", Type::Map)
        .with("job.principal", Type::Map)
        .with("job.principal.kind", Type::String)
        .with("job.principal.id", Type::String)
}

// ------------------------------------------------------------------ job status

/// The job-status contract every surface maps onto (`job-status.schema.json`).
///
/// Times are wall-clock instants (time since the Unix epoch, UTC), written as RFC 3339 with
/// milliseconds ([`format_rfc3339`]). They are for display only: lists are ordered by the
/// scheduler's monotonic submission time, then job ID.
#[derive(Debug, Clone, PartialEq)]
pub struct JobStatus {
    pub id: JobId,
    pub command: CommandId,
    pub state: JobState,
    /// Fraction complete, in [0, 1].
    pub progress: f64,
    /// Stage label of the latest progress report.
    pub stage: String,
    pub submitted_at: Duration,
    pub started_at: Option<Duration>,
    pub ended_at: Option<Duration>,
    /// Set when the state is `failed` or `cancelled`.
    pub error: Option<JobErrorCode>,
}

/// Formats a time since the Unix epoch as RFC 3339 UTC with milliseconds, e.g.
/// `2023-11-14T22:13:20.500Z`. Sub-millisecond parts are cut off; instants after
/// 9999-12-31T23:59:59.999Z are shown as that instant.
pub fn format_rfc3339(since_epoch: Duration) -> String {
    let ms = u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX).min(MAX_RFC3339_MILLIS);
    let (secs, milli) = (ms / 1000, ms % 1000);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (hh, mm, ss) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // Days since 1970-01-01 to a proleptic Gregorian date (H. Hinnant's civil_from_days),
    // in unsigned arithmetic: every intermediate is non-negative for days >= 0.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}.{milli:03}Z")
}

fn timestamp(d: Duration) -> CValue {
    CValue::Str(format_rfc3339(d))
}

fn opt_timestamp(d: Option<Duration>) -> CValue {
    d.map_or(CValue::Null, timestamp)
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

impl JobStatus {
    /// The status view of a scheduler record. The handler's free-text failure message is
    /// left out: it is untrusted and not part of the contract.
    pub fn from_record(r: &JobRecord) -> JobStatus {
        let f = r.progress.fraction;
        // The progress module only accepts finite fractions in [0, 1]; anything else would
        // break the schema, so it is shown as 0.
        let progress = if (0.0..=1.0).contains(&f) { f } else { 0.0 };
        JobStatus {
            id: r.id,
            command: r.command.clone(),
            state: r.state,
            progress,
            stage: r.progress.stage.clone(),
            submitted_at: r.submitted_wall,
            started_at: r.started_wall,
            ended_at: r.ended_wall,
            error: r.error,
        }
    }

    fn fields(&self) -> BTreeMap<String, CValue> {
        let mut o = BTreeMap::new();
        o.insert("id".to_string(), text(&self.id.to_string()));
        o.insert("command".to_string(), text(self.command.as_str()));
        o.insert("state".to_string(), text(self.state.as_str()));
        o.insert("progress".to_string(), CValue::Num(self.progress));
        o.insert("stage".to_string(), text(&self.stage));
        o.insert("submitted_at".to_string(), timestamp(self.submitted_at));
        o.insert("started_at".to_string(), opt_timestamp(self.started_at));
        o.insert("ended_at".to_string(), opt_timestamp(self.ended_at));
        o.insert("error".to_string(), self.error.map_or(CValue::Null, |e| text(e.code())));
        o
    }

    /// The JSON value, in the `job-status.schema.json` shape.
    pub fn to_value(&self) -> CValue {
        CValue::Obj(self.fields())
    }

    /// RFC 8785 canonical JSON of [`JobStatus::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, BusError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| invariant(&format!("job status not serialisable: {e}")))
    }
}

/// Result of `job.wait_job@1`: the job status plus `timed_out`.
#[derive(Debug, Clone, PartialEq)]
pub struct WaitResult {
    pub status: JobStatus,
    /// True when the timeout elapsed before the job reached a terminal state.
    pub timed_out: bool,
}

impl WaitResult {
    pub fn to_value(&self) -> CValue {
        let mut o = self.status.fields();
        o.insert("timed_out".to_string(), CValue::Bool(self.timed_out));
        CValue::Obj(o)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, BusError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| invariant(&format!("wait result not serialisable: {e}")))
    }
}

/// Result of `job.list_jobs@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct JobList {
    /// Sorted by submission time, then job ID.
    pub jobs: Vec<JobStatus>,
}

impl JobList {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("jobs".to_string(), CValue::Arr(self.jobs.iter().map(JobStatus::to_value).collect()));
        CValue::Obj(o)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, BusError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| invariant(&format!("job list not serialisable: {e}")))
    }
}

/// One Record Journal entry the bus must commit for a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEffect {
    /// Journal event type, e.g. `job.cancel_job@1`.
    pub event_type: &'static str,
    /// The job concerned.
    pub job: JobId,
}

/// Result of `job.cancel_job@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct CancelOutcome {
    /// The job status after the request. A running job stays `running` until its handler
    /// sees the token and returns.
    pub status: JobStatus,
    /// `Some` only when this call newly requested the cancellation; `None` for a job already
    /// terminal or already asked to cancel (nothing to commit).
    pub commit: Option<JournalEffect>,
}

/// A command result as JSON, with what the bus must commit.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub result: CValue,
    pub commit: Option<JournalEffect>,
}

// ------------------------------------------------------------------ parameters

/// Parameters of `job.list_jobs@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListJobsParams {
    /// Only jobs in this state; `None` for all.
    pub state: Option<JobState>,
}

/// Parameters of `job.show_status@1` and `job.cancel_job@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobParams {
    pub job: JobId,
}

/// Parameters of `job.wait_job@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitParams {
    pub job: JobId,
    /// 0..=[`MAX_WAIT_TIMEOUT`]; [`DEFAULT_WAIT_TIMEOUT`] when absent.
    pub timeout: Duration,
}

/// Parses a job state wire name.
pub fn parse_job_state(s: &str) -> Option<JobState> {
    JOB_STATES.iter().copied().find(|st| st.as_str() == s)
}

/// Parses a job ID: exactly the lowercase hyphenated UUID form ([`JOB_ID_PATTERN`]).
pub fn parse_job_id(s: &str) -> Result<JobId, BusError> {
    let bad = || validation(ValidationReason::Schema, "/job must be a lowercase hyphenated UUID");
    let uuid = Uuid::parse(s).map_err(|_| bad())?;
    if uuid.to_string() != s {
        return Err(bad());
    }
    Ok(JobId::from_uuid(uuid))
}

/// A parameter object with every field checked off once; leftovers are refused.
struct Params(BTreeMap<String, CValue>);

impl Params {
    fn new(v: &CValue, allowed: &[&str]) -> Result<Params, BusError> {
        let CValue::Obj(o) = v else {
            return Err(validation(ValidationReason::Schema, "parameters must be a JSON object"));
        };
        if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
            // The key is untrusted; only its length is reported.
            return Err(validation(ValidationReason::Schema, &format!("unknown parameter field ({} bytes)", k.len())));
        }
        Ok(Params(o.clone()))
    }

    fn job(&self) -> Result<JobId, BusError> {
        match self.0.get("job") {
            Some(CValue::Str(s)) => parse_job_id(s),
            Some(_) => Err(validation(ValidationReason::Schema, "/job must be a string")),
            None => Err(validation(ValidationReason::Schema, "/job is required")),
        }
    }
}

impl ListJobsParams {
    pub fn from_value(v: &CValue) -> Result<ListJobsParams, BusError> {
        let p = Params::new(v, &["state"])?;
        let state = match p.0.get("state") {
            None => None,
            Some(CValue::Str(s)) => Some(
                parse_job_state(s).ok_or_else(|| validation(ValidationReason::Schema, "/state is not a job state"))?,
            ),
            Some(_) => return Err(validation(ValidationReason::Schema, "/state must be a string")),
        };
        Ok(ListJobsParams { state })
    }
}

impl JobParams {
    pub fn from_value(v: &CValue) -> Result<JobParams, BusError> {
        let p = Params::new(v, &["job"])?;
        Ok(JobParams { job: p.job()? })
    }
}

impl WaitParams {
    pub fn from_value(v: &CValue) -> Result<WaitParams, BusError> {
        let p = Params::new(v, &["job", "timeout"])?;
        let job = p.job()?;
        let secs = match p.0.get("timeout") {
            None => return Ok(WaitParams { job, timeout: DEFAULT_WAIT_TIMEOUT }),
            Some(CValue::Num(n)) => *n,
            Some(CValue::I64(n)) => *n as f64,
            Some(CValue::U64(n)) => *n as f64,
            Some(_) => return Err(validation(ValidationReason::Schema, "/timeout must be a number of seconds")),
        };
        let range = || {
            validation(
                ValidationReason::LiteralOutOfRange,
                &format!("/timeout must be between 0 and {} s", MAX_WAIT_TIMEOUT.as_secs()),
            )
        };
        if !(0.0..=MAX_WAIT_TIMEOUT.as_secs_f64()).contains(&secs) {
            return Err(range());
        }
        let timeout = Duration::try_from_secs_f64(secs).map_err(|_| range())?;
        Ok(WaitParams { job, timeout: timeout.min(MAX_WAIT_TIMEOUT) })
    }
}

// ------------------------------------------------------------------ schemas

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| text(x)).collect())
}

fn num(n: f64) -> CValue {
    CValue::Num(n)
}

fn time_field(nullable: bool, description: &str) -> CValue {
    let ty = if nullable { strs(&["string", "null"]) } else { text("string") };
    obj([
        ("type", ty),
        ("format", text("date-time")),
        ("pattern", text(TIMESTAMP_PATTERN)),
        ("description", text(description)),
    ])
}

const SUBMITTED_AT_DOC: &str = "Wall-clock time (UTC, RFC 3339, milliseconds) when the job was submitted. \
    For display only: job lists are ordered by the scheduler's monotonic submission time, then job ID, \
    never by this field.";
const STARTED_AT_DOC: &str = "Wall-clock time (UTC, RFC 3339, milliseconds) when the job started running; \
    null while it is queued or if it never ran.";
const ENDED_AT_DOC: &str = "Wall-clock time (UTC, RFC 3339, milliseconds) when the job reached a terminal \
    state; null until then.";

fn status_properties() -> BTreeMap<String, CValue> {
    let states: Vec<&str> = JOB_STATES.iter().map(|s| s.as_str()).collect();
    let mut errors: Vec<CValue> = JOB_ERROR_CODES.iter().map(|e| text(e.code())).collect();
    errors.push(CValue::Null);
    let mut p = BTreeMap::new();
    p.insert("id".to_string(), obj([("type", text("string")), ("pattern", text(JOB_ID_PATTERN))]));
    p.insert("command".to_string(), obj([("type", text("string")), ("pattern", text(CommandId::PATTERN))]));
    p.insert("state".to_string(), obj([("enum", strs(&states))]));
    p.insert("progress".to_string(), obj([("type", text("number")), ("minimum", num(0.0)), ("maximum", num(1.0))]));
    p.insert(
        "stage".to_string(),
        obj([("type", text("string")), ("maxLength", num(crate::progress::MAX_STAGE_CHARS as f64))]),
    );
    p.insert("submitted_at".to_string(), time_field(false, SUBMITTED_AT_DOC));
    p.insert("started_at".to_string(), time_field(true, STARTED_AT_DOC));
    p.insert("ended_at".to_string(), time_field(true, ENDED_AT_DOC));
    p.insert("error".to_string(), obj([("enum", CValue::Arr(errors))]));
    p
}

const STATUS_FIELDS: [&str; 9] =
    ["command", "ended_at", "error", "id", "progress", "stage", "started_at", "state", "submitted_at"];

fn status_body(extra: Option<(&str, CValue)>) -> CValue {
    let mut props = status_properties();
    let mut required: Vec<&str> = STATUS_FIELDS.to_vec();
    if let Some((k, v)) = extra {
        props.insert(k.to_string(), v);
        required.push(k);
        required.sort_unstable();
    }
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&required)),
        ("properties", CValue::Obj(props)),
    ])
}

/// JSON Schema (2020-12) of a job status; checked in as `schema/job-status.schema.json`.
pub fn job_status_schema() -> CValue {
    let CValue::Obj(mut o) = status_body(None) else { return CValue::Null };
    o.insert("$schema".to_string(), text("https://json-schema.org/draft/2020-12/schema"));
    o.insert("$id".to_string(), text("https://metrologymaven.io/schema/job-status.schema.json"));
    o.insert("title".to_string(), text("Metrology Maven job status"));
    CValue::Obj(o)
}

fn wait_result_schema() -> CValue {
    status_body(Some(("timed_out", obj([("type", text("boolean"))]))))
}

fn list_result_schema() -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["jobs"])),
        ("properties", obj([("jobs", obj([("type", text("array")), ("items", status_body(None))]))])),
    ])
}

fn job_param() -> CValue {
    obj([("type", text("string")), ("pattern", text(JOB_ID_PATTERN))])
}

fn job_params_schema() -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["job"])),
        ("properties", obj([("job", job_param())])),
    ])
}

fn list_params_schema() -> CValue {
    let states: Vec<&str> = JOB_STATES.iter().map(|s| s.as_str()).collect();
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("properties", obj([("state", obj([("enum", strs(&states))]))])),
    ])
}

fn wait_params_schema() -> CValue {
    let timeout = obj([
        ("type", text("number")),
        ("minimum", num(0.0)),
        ("maximum", num(MAX_WAIT_TIMEOUT.as_secs_f64())),
        ("default", num(DEFAULT_WAIT_TIMEOUT.as_secs_f64())),
        ("x-quantity", text("time")),
        ("x-unit", text("s")),
    ]);
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["job"])),
        ("properties", obj([("job", job_param()), ("timeout", timeout)])),
    ])
}

// ------------------------------------------------------------------ definitions

struct DefSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    side_effect: &'static str,
    preconditions: CValue,
    effects: &'static [&'static str],
}

fn def_value(d: DefSpec) -> CValue {
    obj([
        ("id", text(d.name)),
        ("major", num(1.0)),
        ("title", text(d.title)),
        ("description", text(d.description)),
        ("params_schema", d.params),
        ("result_schema", d.result),
        ("side_effect", text(d.side_effect)),
        ("mcp_projection", text("exposed")),
        ("determinism", text("N")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", d.preconditions),
        ("effects", strs(d.effects)),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ])
}

/// The registry definitions of the four job commands, in [`COMMANDS`] order. Built through
/// [`CommandDef::from_value`], so every field rule of the registry applies.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let cancel_pre = CValue::Arr(vec![obj([
        ("expr", text(CANCEL_PRECONDITION)),
        ("bound", num(f64::from(CANCEL_PRECONDITION_BOUND))),
        ("message_key", text(NOT_JOB_OWNER_KEY)),
    ])]);
    let specs = [
        DefSpec {
            name: "job.list_jobs",
            title: "List jobs",
            description: "Lists the caller's jobs (every job for a human), by submission time then job ID, \
                          optionally filtered by state.",
            params: list_params_schema(),
            result: list_result_schema(),
            side_effect: "R0",
            preconditions: CValue::Arr(Vec::new()),
            effects: &[],
        },
        DefSpec {
            name: "job.show_status",
            title: "Show job status",
            description: "Returns one job's status: ID, command, state, progress, stage, timestamps and error code.",
            params: job_params_schema(),
            result: status_body(None),
            side_effect: "R0",
            preconditions: CValue::Arr(Vec::new()),
            effects: &[],
        },
        DefSpec {
            name: "job.wait_job",
            title: "Wait for a job",
            description: "Waits until the job ends or the timeout elapses and returns its status; \
                          timed_out is true when the timeout elapsed first.",
            params: wait_params_schema(),
            result: wait_result_schema(),
            side_effect: "R0",
            preconditions: CValue::Arr(Vec::new()),
            effects: &[],
        },
        DefSpec {
            name: "job.cancel_job",
            title: "Cancel a job",
            description: "Requests cancellation of a job. Allowed for the job's own principal or a human; \
                          a job already ended is returned unchanged.",
            params: job_params_schema(),
            result: status_body(None),
            side_effect: "W1",
            preconditions: cancel_pre,
            effects: &[CANCEL_JOB],
        },
    ];
    specs.into_iter().map(|d| CommandDef::from_value(def_value(d))).collect()
}

// ------------------------------------------------------------------ service

struct Owners {
    map: BTreeMap<JobId, Caller>,
    sweep_at: usize,
}

/// The job command handlers over a [`Jobs`] scheduler.
///
/// Jobs are submitted only through [`JobService::submit_as`], which records the owning
/// principal. The scheduler itself is never handed out; [`JobService::record`] and
/// [`JobService::is_cancel_requested`] are read-only views. A job without a recorded owner
/// (only possible inside this crate) is visible and cancellable only by a human.
pub struct JobService<T: Send + 'static> {
    jobs: Jobs<T>,
    owners: Mutex<Owners>,
    rule: Predicate,
}

impl<T: Send + 'static> fmt::Debug for JobService<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobService").field("jobs", &self.jobs).finish_non_exhaustive()
    }
}

impl<T: Send + 'static> JobService<T> {
    /// Wraps `jobs`. Compiles the cancel precondition from the registry definition; fails
    /// (`internal/invariant_violated`) if the definitions do not build.
    pub fn new(jobs: Jobs<T>) -> Result<JobService<T>, BusError> {
        let defs = command_defs().map_err(|e| invariant(&format!("job command definitions: {e}")))?;
        let cancel = defs
            .iter()
            .find(|d| d.id == "job.cancel_job" && d.major == 1)
            .ok_or_else(|| invariant("job.cancel_job@1 is not defined"))?;
        let mut rules = compile_command(cancel, &context_types()).map_err(|e| invariant(&e.to_string()))?;
        let rule = match (rules.pop(), rules.is_empty()) {
            (Some(rule), true) => rule,
            _ => return Err(invariant("job.cancel_job@1 must declare exactly one precondition")),
        };
        Ok(JobService { jobs, owners: Mutex::new(Owners { map: BTreeMap::new(), sweep_at: MIN_OWNER_SWEEP }), rule })
    }

    /// Read-only: the scheduler's record of a job, without any access check (for the bus and
    /// the committer, not a command result).
    pub fn record(&self, id: JobId) -> Result<JobRecord, JobErrorCode> {
        self.jobs.status(id)
    }

    /// Read-only: whether cancellation of the job has been requested.
    pub fn is_cancel_requested(&self, id: JobId) -> Result<bool, JobErrorCode> {
        self.jobs.progress(id).map(|p| p.is_cancel_requested())
    }

    /// Submits `exec` on behalf of `caller` and records `caller` as the job's owner. The
    /// job's principal kind is the caller's, whatever `opts.principal` says.
    pub fn submit_as(&self, caller: &Caller, exec: Execution<T>, opts: JobOpts) -> Result<JobId, JobErrorCode> {
        let mut owners = lock(&self.owners);
        let id = self.jobs.submit(exec, JobOpts { principal: caller.kind, ..opts })?;
        owners.map.insert(id, caller.clone());
        if owners.map.len() >= owners.sweep_at {
            // Drop owners of jobs the scheduler no longer keeps; amortised O(1) per submit.
            let live: BTreeSet<JobId> = self.jobs.list().into_iter().map(|r| r.id).collect();
            owners.map.retain(|id, _| live.contains(id));
            owners.sweep_at = owners.map.len().saturating_mul(2).max(MIN_OWNER_SWEEP);
        }
        Ok(id)
    }

    /// The recorded owner of a job, if any.
    pub fn owner(&self, id: JobId) -> Option<Caller> {
        lock(&self.owners).map.get(&id).cloned()
    }

    /// Evaluates the owner-or-human rule. A job without a recorded owner fails the read of
    /// `job.principal` for a non-human, which is refused like a false rule.
    fn allowed(&self, caller: &Caller, owner: Option<&Caller>) -> Result<bool, BusError> {
        let ctx = RuleContext { caller, owner };
        match evaluate(&self.rule, &ctx).result {
            Ok(Verdict::True) => Ok(true),
            Ok(Verdict::False { .. }) | Err(PredError::MissingField { .. }) => Ok(false),
            Err(PredError::Budget { steps }) => Err(BusError::Precondition(
                PreconditionReason::StepBoundExceeded,
                Detail::new(&format!("{NOT_JOB_OWNER_KEY}: step bound reached after {steps} steps")),
            )),
            Err(e) => Err(invariant(&format!("{NOT_JOB_OWNER_KEY}: {e}"))),
        }
    }

    /// Looks up a job (`not_found` first) and checks the caller may use it (`policy`).
    fn authorised(&self, caller: &Caller, id: JobId) -> Result<JobRecord, BusError> {
        let record = self.jobs.status(id).map_err(|e| sched_error(id, e))?;
        let owner = self.owner(id);
        if self.allowed(caller, owner.as_ref())? {
            Ok(record)
        } else {
            Err(not_job_owner(id))
        }
    }

    /// `job.list_jobs@1` (R0).
    pub fn list_jobs(&self, caller: &Caller, params: &ListJobsParams) -> Result<JobList, BusError> {
        let mut records = self.jobs.list();
        records.retain(|r| params.state.map_or(true, |s| r.state == s));
        let mut visible = Vec::with_capacity(records.len());
        {
            let owners = lock(&self.owners);
            for r in records {
                // Filtering fails closed: any evaluation error hides the job.
                if matches!(self.allowed(caller, owners.map.get(&r.id)), Ok(true)) {
                    visible.push(r);
                }
            }
        }
        visible.sort_by(|a, b| a.submitted_at.cmp(&b.submitted_at).then_with(|| a.id.cmp(&b.id)));
        Ok(JobList { jobs: visible.iter().map(JobStatus::from_record).collect() })
    }

    /// `job.show_status@1` (R0).
    pub fn show_status(&self, caller: &Caller, params: &JobParams) -> Result<JobStatus, BusError> {
        self.authorised(caller, params.job).map(|r| JobStatus::from_record(&r))
    }

    /// `job.wait_job@1` (R0). Blocks the calling thread for at most `params.timeout`, and
    /// returns `cancelled/requested` within [`crate::WAIT_CANCEL_POLL`] once `call` (the bus
    /// token of this call) is cancelled.
    pub fn wait_job(&self, caller: &Caller, params: &WaitParams, call: &CancelToken) -> Result<WaitResult, BusError> {
        self.authorised(caller, params.job)?;
        let record =
            self.jobs.wait_cancellable(params.job, params.timeout, call).map_err(|e| sched_error(params.job, e))?;
        if call.is_cancelled() && !record.state.is_terminal() {
            return Err(BusError::Cancelled(
                CancelledReason::Requested,
                Detail::new(&format!("wait for job {} was cancelled by its caller", params.job)),
            ));
        }
        Ok(WaitResult { timed_out: !record.state.is_terminal(), status: JobStatus::from_record(&record) })
    }

    /// `job.cancel_job@1` (W1).
    pub fn cancel_job(&self, caller: &Caller, params: &JobParams) -> Result<CancelOutcome, BusError> {
        let id = params.job;
        let before = self.authorised(caller, id)?;
        if before.state.is_terminal() {
            return Ok(CancelOutcome { status: JobStatus::from_record(&before), commit: None });
        }
        // The scheduler decides "newly requested" atomically under its own lock; a job that
        // ended in between (on its own or by timeout) reports false and commits nothing.
        let request = self.jobs.cancel_request(id).map_err(|e| sched_error(id, e))?;
        let record = match self.jobs.status(id) {
            Ok(r) => r,
            // Dropped by the retention cap just now: report the state the request returned.
            Err(_) => JobRecord { state: request.state, ..before },
        };
        Ok(CancelOutcome {
            status: JobStatus::from_record(&record),
            commit: request.newly_requested.then_some(JournalEffect { event_type: CANCEL_JOB, job: id }),
        })
    }

    /// Runs one job command from its JSON parameters; `call` is the bus token of this call
    /// (only `job.wait_job@1` blocks on it). An unknown command is `not_found/unknown_command`.
    pub fn invoke(
        &self,
        caller: &Caller,
        command: &str,
        params: &CValue,
        call: &CancelToken,
    ) -> Result<Invocation, BusError> {
        let plain = |result: CValue| Invocation { result, commit: None };
        match command {
            LIST_JOBS => Ok(plain(self.list_jobs(caller, &ListJobsParams::from_value(params)?)?.to_value())),
            SHOW_STATUS => Ok(plain(self.show_status(caller, &JobParams::from_value(params)?)?.to_value())),
            WAIT_JOB => Ok(plain(self.wait_job(caller, &WaitParams::from_value(params)?, call)?.to_value())),
            CANCEL_JOB => {
                let out = self.cancel_job(caller, &JobParams::from_value(params)?)?;
                Ok(Invocation { result: out.status.to_value(), commit: out.commit })
            }
            _ => Err(BusError::NotFound(NotFoundReason::UnknownCommand, Detail::new("not a job command"))),
        }
    }

    /// [`JobService::invoke`] from raw JSON bytes: at most [`MAX_PARAMS_BYTES`] bytes and
    /// [`MAX_PARAMS_DEPTH`] levels; duplicate keys are refused.
    pub fn invoke_json(
        &self,
        caller: &Caller,
        command: &str,
        params: &[u8],
        call: &CancelToken,
    ) -> Result<Invocation, BusError> {
        let value = parse_with_limits(params, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(canon_error)?;
        self.invoke(caller, command, &value, call)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::sched::{JobsConfig, NullLifecycleSink};
    use std::sync::Arc;

    /// A job submitted straight to the scheduler (no recorded owner) is open only to a human.
    #[test]
    fn ownerless_job_is_human_only() {
        let config = JobsConfig { max_running: 1, timer_poll: None, ..JobsConfig::default() };
        let svc: JobService<()> = JobService::new(Jobs::new(config, Arc::new(NullLifecycleSink)).unwrap()).unwrap();
        let handler = |_: &crate::ProgressSink, t: &CancelToken| {
            while !t.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(crate::HandlerError::Cancelled)
        };
        let exec = Execution::new(CommandId::parse("test.sleep@1").unwrap(), handler);
        let id = svc.jobs.submit(exec, JobOpts::default()).unwrap();
        assert_eq!(svc.owner(id), None);
        for kind in [PrincipalKind::Script, PrincipalKind::Agent] {
            let who = Caller::new(kind, PrincipalId::parse("s").unwrap());
            for e in [
                svc.show_status(&who, &JobParams { job: id }).map(|_| ()).unwrap_err(),
                svc.cancel_job(&who, &JobParams { job: id }).map(|_| ()).unwrap_err(),
            ] {
                assert_eq!((e.code().as_str(), e.reason()), ("policy", "not_job_owner"));
            }
        }
        let h = Caller::new(PrincipalKind::Human, PrincipalId::parse("h").unwrap());
        assert!(svc.show_status(&h, &JobParams { job: id }).is_ok());
        let out = svc.cancel_job(&h, &JobParams { job: id }).unwrap();
        assert_eq!(out.commit, Some(JournalEffect { event_type: CANCEL_JOB, job: id }));
    }
}
