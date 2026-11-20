//! Handler contract, state view and effects check. Spec node `CORE.BUS.PIPE.HANDLER`.
//!
//! A command handler gets an immutable [`StateView`], [`ValidParams`] and a [`HandlerCtx`]
//! (principal, seed, cancel token) and returns only a list of [`Event`]s or a
//! [`HandlerError`] (REQ-001). [`StateView`] has read methods only and every reference it
//! hands out is shared, so no handler can reach a mutable reference to project state
//! (REQ-002); `tests/compile_fail/mut_state.rs` is built as a `compile_fail` doctest on
//! [`Handler`], next to a control that differs only in `&mut`, to prove it (AC-1).
//!
//! [`run_handler`] looks the command up in a [`HandlerSet`], which stores each handler with
//! its command definition, runs it, checks the events against that definition's declared
//! effects ([`effects_check`]) and hands back [`CheckedEvents`] bound to the command ID and
//! the call's full principal, the only value the commit stage accepts. The checks run in
//! this pinned order, and the first failure is returned with nothing to commit:
//!
//! 1. more than [`MAX_EVENTS`] events: `validation/effect_limit` (REQ-004);
//! 2. in event order, the first event whose type is not in the declared effects:
//!    `undeclared_effect/undeclared_event_type`, naming the type (REQ-003);
//! 3. in event order, the first body holding a non-finite number anywhere (no RFC 8785
//!    canonical form): `validation/schema`. Every body is scanned in full before any
//!    length is counted, so this beats the size limit;
//! 4. more than [`MAX_EVENT_BODY_BYTES`] of canonical body bytes: `validation/effect_limit`
//!    (REQ-004). Lengths are counted by [`canonical_len`] without building the bytes, and
//!    counting stops at the limit.
//!
//! The [`HandlerCtx`] principal comes only from [`SessionTable::derive_principal`]
//! (REQ-CORE.BUS.PRIN-002): [`HandlerCtx::derive`] is its only constructor, and it reads
//! the channel, never the parameters. The context owns everything it holds, so no session
//! table borrow outlives [`HandlerCtx::derive`]. Every error is from the frozen bus error
//! catalogue; this module adds no code or reason.

use crate::error::{
    BusError, CancelledReason, Code, Detail, InternalReason, NotFoundReason, UndeclaredEffectReason, ValidationReason,
};
use crate::principal::{Channel, Principal, SessionTable};
use fid_canon::{parse_with_limits, CValue, CanonError, Digest, MAX_DEPTH, MAX_INPUT_BYTES};
use fid_registry::CommandDef;
use fid_types::{CommandId, CommandName};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Most events one handler call may return (REQ-004); one more is `validation/effect_limit`.
pub const MAX_EVENTS: usize = 100_000;
/// Most canonical body bytes one handler call may return, summed over its events (REQ-004).
pub const MAX_EVENT_BODY_BYTES: u64 = 256 * 1024 * 1024;
/// Longest [`EventType`], in bytes.
pub const MAX_EVENT_TYPE_BYTES: usize = 128;
/// Longest [`EntityId`], in bytes.
pub const MAX_ENTITY_ID_BYTES: usize = 256;
/// Longest [`SettingKey`], in bytes.
pub const MAX_SETTING_KEY_BYTES: usize = 128;
/// Largest serialised event envelope [`Event::from_json`] reads.
pub const MAX_ENVELOPE_BYTES: usize = MAX_INPUT_BYTES;
/// Deepest nesting [`Event::from_json`] reads.
pub const MAX_ENVELOPE_DEPTH: usize = MAX_DEPTH;
/// Most handlers one [`HandlerSet`] holds.
pub const MAX_HANDLERS: usize = 65_536;

fn schema(detail: &str) -> BusError {
    BusError::Validation(ValidationReason::Schema, Detail::new(detail))
}

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

// ------------------------------------------------------------------ names

/// An event type, e.g. `ElementCreated` or `align.run_321@1`: 1 to
/// [`MAX_EVENT_TYPE_BYTES`] bytes, an ASCII letter first, then ASCII letters, digits,
/// `_`, `.` or `@`. Compared byte for byte (case-sensitive).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventType(String);

impl EventType {
    /// Whether `s` is a valid event type.
    pub fn is_valid(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() <= MAX_EVENT_TYPE_BYTES
            && b.first().is_some_and(u8::is_ascii_alphabetic)
            && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'@'))
    }

    /// Checks and wraps an event type; `validation/schema` if invalid.
    pub fn new(s: &str) -> Result<Self, BusError> {
        if EventType::is_valid(s) {
            Ok(EventType(s.to_string()))
        } else {
            Err(schema("event type must be 1..=128 bytes of [A-Za-z][A-Za-z0-9_.@]*"))
        }
    }

    /// The type name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A stable entity ID (CONSTITUTION §5): imported IDs are kept as they are, so this is
/// text, 1 to [`MAX_ENTITY_ID_BYTES`] bytes with no control character.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(String);

impl EntityId {
    /// Checks and wraps an entity ID; `validation/schema` if invalid.
    pub fn new(s: &str) -> Result<Self, BusError> {
        if s.is_empty() || s.len() > MAX_ENTITY_ID_BYTES || s.chars().any(char::is_control) {
            return Err(schema("entity id must be 1..=256 bytes with no control character"));
        }
        Ok(EntityId(s.to_string()))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A project setting key: same grammar as a command name (`[a-z0-9_.]`, alphanumeric at
/// both ends), at most [`MAX_SETTING_KEY_BYTES`] bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SettingKey(String);

impl SettingKey {
    /// Checks and wraps a setting key; `validation/schema` if invalid.
    pub fn new(s: &str) -> Result<Self, BusError> {
        if s.len() > MAX_SETTING_KEY_BYTES || !CommandName::is_valid(s) {
            return Err(schema("setting key must be 1..=128 bytes of [a-z0-9_.], alphanumeric at both ends"));
        }
        Ok(SettingKey(s.to_string()))
    }

    /// The key text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ------------------------------------------------------------------ state view

/// A read-only view of one entity. Every field is a shared reference, so nothing reached
/// through it can be changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityRef<'a> {
    pub id: &'a EntityId,
    /// The entity's kind, e.g. `feature.plane`.
    pub kind: &'a str,
    /// The entity's canonical body.
    pub body: &'a CValue,
}

/// The read interface a handler sees (REQ-002). The project model implements it, so the bus
/// never depends on the project crate. Read methods only: every method takes `&self` and
/// returns owned values or shared references.
pub trait StateView {
    /// The entity with this ID, if it exists.
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>>;
    /// The hash of the branch head the call runs against.
    fn head(&self) -> Digest;
    /// The value of a project setting, if set.
    fn setting(&self, key: &SettingKey) -> Option<CValue>;
}

// ------------------------------------------------------------------ inputs

/// Parameters that passed schema validation (`CORE.BUS.PIPE.VALID`). Only this crate builds
/// one (the validation stage lands in `fid-bus`); the handler reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidParams(CValue);

impl ValidParams {
    /// Wraps parameters the validation stage accepted.
    #[allow(dead_code)] // first caller: CORE.BUS.PIPE.VALID
    pub(crate) fn from_validated(value: CValue) -> Self {
        ValidParams(value)
    }

    /// Test-only constructor for handler tests outside this crate; not part of the API.
    /// Exists only with the `test-support` feature, which only `fid-bus`'s own
    /// dev-dependency on itself enables.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn for_tests(value: CValue) -> Self {
        ValidParams(value)
    }

    /// The parameters.
    pub fn value(&self) -> &CValue {
        &self.0
    }
}

/// The owner side of a cancel flag; the bus or the job system keeps it.
#[derive(Debug, Default)]
pub struct CancelSource {
    flag: Arc<AtomicBool>,
}

impl CancelSource {
    /// A fresh, not cancelled source.
    pub fn new() -> Self {
        CancelSource::default()
    }

    /// A read-only token for the handler.
    pub fn token(&self) -> CancelToken {
        CancelToken { flag: Arc::clone(&self.flag) }
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }
}

/// What a handler can learn about cancellation: one flag, read only.
#[derive(Clone)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    /// A token that is never cancelled.
    pub fn never() -> Self {
        CancelToken { flag: Arc::new(AtomicBool::new(false)) }
    }

    /// True once cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
}

impl fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CancelToken")
    }
}

/// The context of one handler call: principal, seed and cancel token (REQ-001). Owns all
/// three, so it borrows no session table.
#[derive(Debug, Clone)]
pub struct HandlerCtx {
    principal: Principal,
    seed: u64,
    cancel: CancelToken,
}

impl HandlerCtx {
    /// Builds the context of one call. The principal comes from
    /// [`SessionTable::derive_principal`] on `channel` and nowhere else; a channel without
    /// an authenticated identity gives `unauthenticated`. The table is borrowed only for
    /// the length of this call.
    pub fn derive(
        table: &SessionTable,
        channel: &dyn Channel,
        seed: u64,
        cancel: CancelToken,
    ) -> Result<HandlerCtx, BusError> {
        let principal = table.derive_principal(channel)?;
        Ok(HandlerCtx { principal, seed, cancel })
    }

    /// Who is acting.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// The call's seed (CONSTITUTION §4); recorded so replay gives the same events.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The cancel flag.
    pub fn cancel(&self) -> &CancelToken {
        &self.cancel
    }
}

// ------------------------------------------------------------------ events

/// One event a handler returns: type and canonical body.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    type_id: EventType,
    body: CValue,
}

impl Event {
    /// An event.
    pub fn new(type_id: EventType, body: CValue) -> Self {
        Event { type_id, body }
    }

    /// The event type.
    pub fn type_id(&self) -> &EventType {
        &self.type_id
    }

    /// The body.
    pub fn body(&self) -> &CValue {
        &self.body
    }

    /// The envelope `{"body": ..., "type": "..."}`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("body".to_string(), self.body.clone());
        o.insert("type".to_string(), CValue::Str(self.type_id.0.clone()));
        CValue::Obj(o)
    }

    /// Reads an envelope value: an object with exactly the fields `type` (a valid
    /// [`EventType`]) and `body` (any JSON). Anything else is `validation/schema`.
    pub fn from_value(v: CValue) -> Result<Event, BusError> {
        let CValue::Obj(mut o) = v else {
            return Err(schema("event envelope must be an object"));
        };
        let type_id = match o.remove("type") {
            Some(CValue::Str(s)) => EventType::new(&s)?,
            Some(_) => return Err(schema("event envelope field `type` must be a string")),
            None => return Err(schema("event envelope lacks field `type`")),
        };
        let Some(body) = o.remove("body") else {
            return Err(schema("event envelope lacks field `body`"));
        };
        if !o.is_empty() {
            return Err(schema("event envelope has a field other than `type` and `body`"));
        }
        Ok(Event { type_id, body })
    }

    /// Reads a serialised envelope of at most [`MAX_ENVELOPE_BYTES`] bytes and
    /// [`MAX_ENVELOPE_DEPTH`] levels. Oversized or too deep input is
    /// `validation/input_limit`; bad JSON, duplicate keys at any depth and a wrong shape are
    /// `validation/schema`.
    pub fn from_json(bytes: &[u8]) -> Result<Event, BusError> {
        let v = parse_with_limits(bytes, MAX_ENVELOPE_BYTES, MAX_ENVELOPE_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => {
                BusError::Validation(ValidationReason::InputLimit, Detail::new("event envelope too large or too deep"))
            }
            CanonError::DuplicateKey(_) => schema("event envelope has a duplicate key"),
            CanonError::NonFiniteNumber | CanonError::InvalidUtf8 | CanonError::Syntax(_) => {
                schema(&format!("event envelope is not valid JSON: {}", e.code()))
            }
        })?;
        Event::from_value(v)
    }
}

/// Events that passed the effects check of one command, bound to that command and to the
/// full principal of the call that produced them (kind, identity, `on_behalf_of`, client ID,
/// upgraded session): the only form the commit stage takes. Built only by [`run_handler`].
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedEvents {
    command: CommandId,
    principal: Principal,
    events: Vec<Event>,
    body_bytes: u64,
}

impl CheckedEvents {
    /// The command whose declared effects the events were checked against.
    pub fn command(&self) -> &CommandId {
        &self.command
    }

    /// The principal of the call that produced the events: a clone of `ctx.principal()`.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// The events, in handler order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Total canonical body bytes.
    pub fn body_bytes(&self) -> u64 {
        self.body_bytes
    }

    /// Command, principal and events, for the commit stage.
    pub fn into_parts(self) -> (CommandId, Principal, Vec<Event>) {
        (self.command, self.principal, self.events)
    }
}

// ------------------------------------------------------------------ handler

/// A typed handler failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerError {
    /// A refusal from the bus error catalogue, e.g. `precondition` or
    /// `not_found/unknown_entity`. Codes only the pipeline may raise become
    /// `internal/handler_failed` (see [`HandlerError::into_bus_error`]).
    Refused(BusError),
    /// The handler saw its cancel token set and stopped.
    Cancelled,
    /// A bug or environment fault; the detail is for logs only.
    Failed(Detail),
}

impl HandlerError {
    /// The bus error this handler error becomes. A refusal keeps its code, reason and detail,
    /// except the pipeline-only codes `hidden_command`, `undeclared_effect`,
    /// `unauthenticated`, `idempotency_mismatch` and `upgrade_refused`, which a handler may
    /// not claim: those become `internal/handler_failed`. `Cancelled` becomes
    /// `cancelled/requested`; `Failed` becomes `internal/handler_failed`.
    pub fn into_bus_error(self) -> BusError {
        match self {
            HandlerError::Refused(e) => match e.code() {
                Code::Validation
                | Code::Policy
                | Code::Precondition
                | Code::Conflict
                | Code::NotFound
                | Code::Cancelled
                | Code::Internal => e,
                Code::HiddenCommand
                | Code::UndeclaredEffect
                | Code::Unauthenticated
                | Code::IdempotencyMismatch
                | Code::UpgradeRefused => BusError::Internal(
                    InternalReason::HandlerFailed,
                    Detail::new(&format!("handler returned pipeline-only code {}", e.code())),
                ),
            },
            HandlerError::Cancelled => {
                BusError::Cancelled(CancelledReason::Requested, Detail::new("handler stopped on cancel"))
            }
            HandlerError::Failed(d) => BusError::Internal(InternalReason::HandlerFailed, d),
        }
    }
}

impl From<BusError> for HandlerError {
    fn from(e: BusError) -> Self {
        HandlerError::Refused(e)
    }
}

/// A command handler (REQ-001). It gets the state as `&dyn StateView` only, so it holds no
/// handle through which it could write state, and it returns events or a typed error.
///
/// AC-1: a handler that asks for `&mut` state does not build
/// (`tests/compile_fail/mut_state.rs`). Stable rustdoc does not check the error code of a
/// `compile_fail` block (only nightly does), so the control below is what makes the test
/// specific: `tests/compile_fail/mut_state_control.rs` is the same file with `&mut` replaced
/// by `&`, and it builds. A test asserts the two files differ only in that `&mut`.
///
#[doc = "```compile_fail"]
#[doc = include_str!("../tests/compile_fail/mut_state.rs")]
#[doc = "```"]
///
/// Control (the contract's signature):
///
#[doc = "```"]
#[doc = include_str!("../tests/compile_fail/mut_state_control.rs")]
#[doc = "```"]
///
/// Nor can a shared view be turned into a mutable one:
///
/// ```compile_fail
/// use fid_bus::handler::StateView;
/// fn widen(view: &dyn StateView) -> &mut dyn StateView {
///     view
/// }
/// fn main() {}
/// ```
pub trait Handler: Send + Sync {
    /// Runs the command against `view` and returns its events in order.
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError>;
}

// ------------------------------------------------------------------ canonical length

fn add(n: &mut u64, k: usize) {
    *n = n.saturating_add(u64::try_from(k).unwrap_or(u64::MAX));
}

/// Canonical length of a string, quotes included (RFC 8785 §3.2.2.2).
fn str_len(s: &str) -> u64 {
    let mut n: u64 = 2;
    for c in s.chars() {
        let k = match c {
            '"' | '\\' | '\u{8}' | '\t' | '\n' | '\u{c}' | '\r' => 2,
            c if u32::from(c) < 0x20 => 6,
            c => c.len_utf8(),
        };
        add(&mut n, k);
    }
    n
}

/// Number of decimal digits of `v`.
fn digits(mut v: u64) -> u64 {
    let mut d = 1;
    while v >= 10 {
        v /= 10;
        d += 1;
    }
    d
}

/// The length in bytes of `value`'s RFC 8785 canonical form (as `fid_canon::to_canonical_bytes`
/// would write it), counted without building it. Counting stops as soon as the length
/// exceeds `limit`; the result is then some value above `limit`. A non-finite number is
/// `NonFiniteNumber`. Iterative, so deep values cannot overflow the stack.
pub fn canonical_len(value: &CValue, limit: u64) -> Result<u64, CanonError> {
    let mut n: u64 = 0;
    let mut stack = vec![value];
    while let Some(v) = stack.pop() {
        match v {
            CValue::Null => n = n.saturating_add(4),
            CValue::Bool(b) => n = n.saturating_add(if *b { 4 } else { 5 }),
            CValue::Str(s) => n = n.saturating_add(str_len(s)),
            CValue::I64(i) => n = n.saturating_add(2 + u64::from(*i < 0) + digits(i.unsigned_abs())),
            CValue::U64(u) => n = n.saturating_add(2 + digits(*u)),
            CValue::F64Bits(f) => {
                if !f.is_finite() {
                    return Err(CanonError::NonFiniteNumber);
                }
                // `"0x` + 16 hex digits + `"`.
                n = n.saturating_add(20);
            }
            CValue::Num(f) => add(&mut n, fid_canon::format_number(*f)?.len()),
            CValue::Arr(items) => {
                // Brackets and the commas between items.
                add(&mut n, 2 + items.len().saturating_sub(1));
                stack.extend(items.iter());
            }
            CValue::Obj(map) => {
                // Braces, commas, and per member the key and a colon.
                add(&mut n, 2 + map.len().saturating_sub(1));
                for (k, item) in map {
                    n = n.saturating_add(str_len(k).saturating_add(1));
                    stack.push(item);
                }
            }
        }
        if n > limit {
            return Ok(n);
        }
    }
    Ok(n)
}

/// Whether `value` holds a NaN or infinite number anywhere (no canonical form). Iterative.
fn has_non_finite(value: &CValue) -> bool {
    let mut stack = vec![value];
    while let Some(v) = stack.pop() {
        match v {
            CValue::Num(f) | CValue::F64Bits(f) => {
                if !f.is_finite() {
                    return true;
                }
            }
            CValue::Arr(items) => stack.extend(items.iter()),
            CValue::Obj(map) => stack.extend(map.values()),
            CValue::Null | CValue::Bool(_) | CValue::Str(_) | CValue::I64(_) | CValue::U64(_) => {}
        }
    }
    false
}

/// Checks `events` against `def.effects` (REQ-003, REQ-004), in the order given in the
/// module docs. Returns the total canonical body bytes.
fn check(def: &CommandDef, events: &[Event]) -> Result<u64, BusError> {
    if events.len() > MAX_EVENTS {
        return Err(BusError::Validation(
            ValidationReason::EffectLimit,
            Detail::new(&format!("handler returned {} events; the limit is {MAX_EVENTS}", events.len())),
        ));
    }
    let declared: BTreeSet<&str> = def.effects.iter().map(String::as_str).collect();
    if let Some((i, e)) = events.iter().enumerate().find(|(_, e)| !declared.contains(e.type_id.as_str())) {
        return Err(BusError::UndeclaredEffect(
            UndeclaredEffectReason::UndeclaredEventType,
            Detail::new(&format!("undeclared event type {} at index {i}", e.type_id)),
        ));
    }
    // Every body is scanned in full before any length is counted, so a body with no
    // canonical form is `schema` even where counting would stop early at the limit.
    if let Some(i) = events.iter().position(|e| has_non_finite(&e.body)) {
        return Err(schema(&format!("event {i} body has no canonical form: non_finite_number")));
    }
    let mut total: u64 = 0;
    for (i, e) in events.iter().enumerate() {
        let room = MAX_EVENT_BODY_BYTES - total;
        let len = canonical_len(&e.body, room)
            .map_err(|err| schema(&format!("event {i} body has no canonical form: {}", err.code())))?;
        if len > room {
            return Err(BusError::Validation(
                ValidationReason::EffectLimit,
                Detail::new(&format!("event bodies exceed {MAX_EVENT_BODY_BYTES} bytes at index {i}")),
            ));
        }
        total += len;
    }
    Ok(total)
}

/// The effects check (REQ-003, REQ-004): `Ok` when every event type is declared in
/// `def.effects` and the limits hold; otherwise the first failure, in the pinned order.
pub fn effects_check(def: &CommandDef, events: &[Event]) -> Result<(), BusError> {
    check(def, events).map(|_| ())
}

/// Looks up command `id` in `set`, runs its handler and checks the events against that
/// command's own definition. On success returns [`CheckedEvents`] bound to `id` and to
/// `ctx`'s principal; on any failure returns the error and nothing to commit. Lookup
/// errors are those of [`HandlerSet::get`]. A cancel requested by the time the handler
/// returns gives `cancelled/requested`.
pub fn run_handler(
    set: &HandlerSet,
    id: &str,
    view: &dyn StateView,
    params: &ValidParams,
    ctx: &HandlerCtx,
) -> Result<CheckedEvents, BusError> {
    let entry = set.entry(id)?;
    let events = entry.handler.run(view, params, ctx).map_err(HandlerError::into_bus_error)?;
    if ctx.cancel.is_cancelled() {
        return Err(BusError::Cancelled(CancelledReason::Requested, Detail::new("cancel requested during handler")));
    }
    let body_bytes = check(&entry.def, &events)?;
    Ok(CheckedEvents { command: entry.id.clone(), principal: ctx.principal.clone(), events, body_bytes })
}

// ------------------------------------------------------------------ handler set

struct Entry {
    id: CommandId,
    def: CommandDef,
    handler: Box<dyn Handler>,
}

/// Handlers, each stored with its command definition, by command `name@major`. Keys are
/// compared as (name, major); only the canonical spelling of a major (no leading zero) is
/// accepted.
#[derive(Default)]
pub struct HandlerSet {
    map: BTreeMap<(String, u32), Entry>,
}

impl fmt::Debug for HandlerSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.map.values().map(|e| e.id.as_str())).finish()
    }
}

/// Splits a canonical `name@major`; `None` if malformed or the major has a leading zero.
fn split_id(id: &str) -> Option<(CommandId, String, u32)> {
    let parsed = CommandId::parse(id).ok()?;
    let (name, major) = id.rsplit_once('@')?;
    (major == parsed.major().to_string()).then(|| (parsed.clone(), name.to_string(), parsed.major()))
}

impl HandlerSet {
    /// An empty set.
    pub fn new() -> Self {
        HandlerSet::default()
    }

    /// Number of handlers.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Binds `handler` to the command `def` defines (`def.id@def.major`) and keeps `def`
    /// with it, so its events are checked against that definition only. An ID that is not a
    /// valid `name@major`, a second handler for the same ID, or a set already holding
    /// [`MAX_HANDLERS`] is `internal/invariant_violated`: these are mistakes in building the
    /// bus, and the set is unchanged.
    pub fn register(&mut self, def: CommandDef, handler: Box<dyn Handler>) -> Result<(), BusError> {
        let Some((id, name, major)) = split_id(&format!("{}@{}", def.id, def.major)) else {
            return Err(invariant("command definition has no valid name@major"));
        };
        let key = (name, major);
        if self.map.contains_key(&key) {
            return Err(invariant(&format!("a handler for {id} is already registered")));
        }
        if self.map.len() >= MAX_HANDLERS {
            return Err(invariant("handler set is full"));
        }
        self.map.insert(key, Entry { id, def, handler });
        Ok(())
    }

    fn entry(&self, id: &str) -> Result<&Entry, BusError> {
        let Some((_, name, major)) = split_id(id) else {
            return Err(schema("command id must be a canonical name@major"));
        };
        let lo = (name, 0);
        if let Some(e) = self.map.get(&(lo.0.clone(), major)) {
            return Ok(e);
        }
        let hi = (lo.0.clone(), u32::MAX);
        let detail = format!("no handler for {}", lo.0);
        if self.map.range(lo..=hi).next().is_some() {
            Err(BusError::NotFound(NotFoundReason::UnknownMajor, Detail::new(&format!("{detail}@{major}"))))
        } else {
            Err(BusError::NotFound(NotFoundReason::UnknownCommand, Detail::new(&detail)))
        }
    }

    /// The handler of `id`. A malformed ID is `validation/schema`; a name with handlers for
    /// other majors only is `not_found/unknown_major`; any other miss is
    /// `not_found/unknown_command`.
    pub fn get(&self, id: &str) -> Result<&dyn Handler, BusError> {
        self.entry(id).map(|e| e.handler.as_ref())
    }

    /// The definition stored with the handler of `id`; same errors as [`HandlerSet::get`].
    pub fn def(&self, id: &str) -> Result<&CommandDef, BusError> {
        self.entry(id).map(|e| &e.def)
    }
}
