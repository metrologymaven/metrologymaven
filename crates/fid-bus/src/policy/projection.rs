//! MCP projection, pending requests and approval. Spec node `CORE.BUS.POL.PROJ`.
//!
//! The projection check of the authorise stage, behind the generated MCP surface (defence in
//! depth). On an MCP channel ([`is_mcp_channel`]), or for any `agent` principal wherever it
//! arrives, a command runs only when it is projected `exposed`; a `request_only` or
//! `human_only` command becomes a pending request for a person; a `never` command, or one
//! without `mcp` among its surfaces, is refused with `policy/not_on_mcp` (REQ-001, REQ-002).
//! The projection used is the stricter of the declared one (`human_only` clamps it to
//! `request_only`, `CommandDef::effective_projection`) and the ceiling of the command's
//! class (`fid_registry::max_projection`), so a definition that slipped past the registry
//! checks still cannot widen what agents reach.
//!
//! The person/agent rule (CONSTITUTION §3, D-031): over MCP an agent can only *file* a
//! request. It can never approve one: [`authorise`] refuses `approval_of` from any
//! principal that is not `human` with `policy/bad_approval` (REQ-004), and the approval is
//! bound to the exact pending record: the request must be `pending`, name the same command
//! and major, and its parameters commitment must equal the digest of the parameters the
//! person invokes with. A request that was already approved, rejected or withdrawn cannot be
//! approved again, so an approval can be neither replayed nor substituted. The approval's
//! `RequestApproved` event carries the request's record digest, and [`RequestBook`] refuses
//! to fold one whose digest, requester or command differs from the record.
//!
//! The principal is always derived by the bus from the channel
//! ([`SessionTable::derive_principal`]); nothing here reads a principal, a requester or an
//! approver from the parameters. Parameters with extra fields are `validation/schema`.
//!
//! Pending requests live in the Record Journal. Filing appends one W1
//! `request.file_request@1` entry ([`Filing::commit`]) holding the command ID and major, the
//! parameters commitment, the requester and the branch head at filing, and returns the
//! request ID (a UUIDv7 recorded as an observed input). [`RequestBook`] is the fold of the
//! four request events (`RequestFiled`, `RequestApproved`, `RequestWithdrawn`,
//! `RequestRejected`) in journal order; it numbers requests by filing sequence and exposes
//! each record to handlers as an entity of kind [`REQUEST_KIND`] through [`StateView`].
//! Nothing in this module changes the book: it changes only when a committed entry is folded,
//! so a refused or failed call leaves every request as it was (REQ-008).
//!
//! Commands ([`command_defs`]): `request.list_requests@1` (R0, [`list_requests`]),
//! `request.withdraw_request@1` (W1, [`WithdrawHandler`]), `request.reject_request@1` (W1,
//! `human_only`, projection `never`, [`RejectHandler`]) and the internal
//! `request.file_request@1` (W1, no surface, written only by [`Filing::commit`]). Every error
//! is from the bus error catalogue: `policy` (`not_on_mcp`, `bad_approval`,
//! `not_requester`, `human_only`), `not_found/unknown_request`, `conflict/id_collision`,
//! `precondition/invalid_transition` (withdrawing or rejecting a request that is no longer
//! pending), `validation/schema`, and `internal/invariant_violated` only for a journal that
//! contradicts itself. Determinism D0.

#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]

use crate::error::{
    BusError, ConflictReason, Detail, InternalReason, NotFoundReason, PolicyReason, PreconditionReason, Untrusted,
    ValidationReason,
};
use crate::handler::{
    EntityId, EntityRef, Event, EventType, Handler, HandlerCtx, HandlerError, SettingKey, StateView, ValidParams,
};
use crate::principal::{
    Channel, ChannelKind, JournalAppend, Principal, PrincipalId, PrincipalKind, PrincipalRef, SessionTable,
};
use fid_canon::{CValue, Digest as CanonDigest};
use fid_journal::{
    AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, OutputDigest, TaggedDigest,
};
use fid_registry::{max_projection, CommandDef, DefError, Projection, Surface};
use fid_types::{CommandId, Determinism, Digest, SideEffect};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Entity kind under which [`RequestBook`] exposes a request record to handlers.
pub const REQUEST_KIND: &str = "request.pending_request";
/// Event of a filed request (`request.file_request@1`).
pub const REQUEST_FILED: &str = "RequestFiled";
/// Event added to the approved command's commit.
pub const REQUEST_APPROVED: &str = "RequestApproved";
/// Event of `request.withdraw_request@1`.
pub const REQUEST_WITHDRAWN: &str = "RequestWithdrawn";
/// Event of `request.reject_request@1`.
pub const REQUEST_REJECTED: &str = "RequestRejected";
/// `request.list_requests@1`.
pub const LIST_REQUESTS: &str = "request.list_requests";
/// `request.withdraw_request@1`.
pub const WITHDRAW_REQUEST: &str = "request.withdraw_request";
/// `request.reject_request@1`.
pub const REJECT_REQUEST: &str = "request.reject_request";
/// `request.file_request@1` (internal).
pub const FILE_REQUEST: &str = "request.file_request";
/// Longest rejection reason, in characters (Unicode scalar values).
pub const MAX_REASON_CHARS: usize = 2000;
/// Input tag of the parameters commitment in the filing entry.
pub const PARAMS_TAG: &str = "param";

/// Largest filing sequence; a record body carries it exactly as a JSON number (2^53).
pub const MAX_SEQ: u64 = 1 << 53;
const MAX_EXACT_SEQ: f64 = 9_007_199_254_740_992.0;
const LIFECYCLE_SINCE: &str = "2026-11-20";
const UUID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

// ------------------------------------------------------------------ errors

fn schema(detail: &str) -> BusError {
    BusError::Validation(ValidationReason::Schema, Detail::new(detail))
}

fn policy(reason: PolicyReason, detail: &str) -> BusError {
    BusError::Policy(reason, Detail::new(detail))
}

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

fn unknown_request(id: &RequestId) -> BusError {
    BusError::NotFound(NotFoundReason::UnknownRequest, Detail::new(&format!("no request {id}")))
}

fn not_pending(id: &RequestId, state: RequestState) -> BusError {
    BusError::Precondition(
        PreconditionReason::InvalidTransition,
        Detail::new(&format!("request {id} is {}, not pending", state.as_str())),
    )
}

// ------------------------------------------------------------------ JSON helpers

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

/// Reads an object with exactly the keys in `required` plus any of `optional`; anything
/// else is `validation/schema`.
fn fields<'a>(
    v: &'a CValue,
    what: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a BTreeMap<String, CValue>, BusError> {
    let CValue::Obj(o) = v else { return Err(schema(&format!("{what} must be an object"))) };
    if let Some(k) = o.keys().find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str())) {
        let k: String = k.chars().take(64).collect();
        return Err(schema(&format!("{what} has unknown field {k:?}")));
    }
    if let Some(k) = required.iter().find(|k| !o.contains_key(**k)) {
        return Err(schema(&format!("{what} lacks field {k}")));
    }
    Ok(o)
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, key: &str, what: &str) -> Result<&'a str, BusError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(schema(&format!("{what}.{key} must be a string"))),
    }
}

fn digest_field(o: &BTreeMap<String, CValue>, key: &str, what: &str) -> Result<Digest, BusError> {
    Digest::parse(str_field(o, key, what)?).map_err(|_| schema(&format!("{what}.{key} must be a sha256: digest")))
}

fn principal_value(p: &PrincipalRef) -> CValue {
    p.to_value()
}

fn principal_from(v: Option<&CValue>, what: &str) -> Result<PrincipalRef, BusError> {
    let v = v.ok_or_else(|| schema(&format!("{what} is missing")))?;
    let o = fields(v, what, &["id", "kind"], &[])?;
    let kind = PrincipalKind::parse(str_field(o, "kind", what)?).map_err(|_| schema(&format!("{what}.kind")))?;
    let id = PrincipalId::new(str_field(o, "id", what)?).map_err(|e| schema(&format!("{what}.id: {e}")))?;
    Ok(PrincipalRef { kind, id })
}

fn to_digest(d: CanonDigest) -> Digest {
    Digest::from_bytes(d.0)
}

fn canon_digest(v: &CValue, what: &str) -> Result<Digest, BusError> {
    fid_canon::digest(v).map(to_digest).map_err(|e| schema(&format!("{what} has no canonical form: {}", e.code())))
}

/// The parameters commitment of an invocation: the `sha256:` digest of the parameters'
/// RFC 8785 canonical bytes. `validation/schema` if they have no canonical form.
pub fn params_commitment(params: &CValue) -> Result<Digest, BusError> {
    canon_digest(params, "parameters")
}

/// The commitment of an untrusted text: the digest of the text as a canonical JSON string.
pub fn text_commitment(t: &Untrusted<String>) -> Result<Digest, BusError> {
    canon_digest(&CValue::Str(t.as_untrusted().clone()), "text")
}

fn command_id(def: &CommandDef) -> Result<CommandId, BusError> {
    CommandId::parse(&format!("{}@{}", def.id, def.major)).map_err(|_| invariant("command definition has no valid id"))
}

// ------------------------------------------------------------------ request IDs and states

/// A request ID: a UUIDv7 (RFC 9562 version 7, variant `10`), written in lowercase
/// hyphenated form. Issued by the pipeline as an observed input and recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(Uuid);

impl RequestId {
    /// A request ID from a UUID; `validation/schema` unless it is version 7 and variant `10`.
    pub fn from_uuid(u: Uuid) -> Result<RequestId, BusError> {
        if u.version() == 7 && u.variant_bits() == 0b10 {
            Ok(RequestId(u))
        } else {
            Err(schema("request id must be a UUIDv7"))
        }
    }

    /// Parses the lowercase hyphenated form; anything else is `validation/schema`.
    pub fn parse(s: &str) -> Result<RequestId, BusError> {
        let u = Uuid::parse(s).map_err(|_| schema("request id must be a lowercase hyphenated UUIDv7"))?;
        if u.to_string() != s {
            return Err(schema("request id must be a lowercase hyphenated UUIDv7"));
        }
        RequestId::from_uuid(u)
    }

    /// The UUID.
    pub fn uuid(&self) -> Uuid {
        self.0
    }

    fn entity_id(&self) -> Result<EntityId, BusError> {
        EntityId::new(&self.to_string())
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// The state of a pending-request record. Only `pending` moves, once, to one of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RequestState {
    Pending,
    Approved,
    Rejected,
    Withdrawn,
}

impl RequestState {
    /// Every state, in declaration order.
    pub const ALL: [RequestState; 4] =
        [RequestState::Pending, RequestState::Approved, RequestState::Rejected, RequestState::Withdrawn];

    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            RequestState::Pending => "pending",
            RequestState::Approved => "approved",
            RequestState::Rejected => "rejected",
            RequestState::Withdrawn => "withdrawn",
        }
    }

    /// Decodes a wire string.
    pub fn from_wire(s: &str) -> Option<RequestState> {
        RequestState::ALL.into_iter().find(|x| x.as_str() == s)
    }
}

// ------------------------------------------------------------------ projection

/// Whether calls on this channel kind come through an MCP surface. Total, no wildcard.
pub const fn is_mcp_channel(kind: ChannelKind) -> bool {
    match kind {
        ChannelKind::Mcp | ChannelKind::ServerMcp => true,
        ChannelKind::Gui
        | ChannelKind::Cli
        | ChannelKind::Python
        | ChannelKind::Control
        | ChannelKind::ServerSession
        | ChannelKind::ServerService
        | ChannelKind::Service
        | ChannelKind::Device
        | ChannelKind::Internal => false,
    }
}

/// The outcome of the projection check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projected {
    /// This check passes; the command may run.
    Run,
    /// The command must not run; the call becomes a pending request for a person.
    PendingRequest,
}

/// The projection MCP callers get for `def`: the stricter of the declared projection
/// (`human_only` clamped to `request_only`) and the ceiling of its class, and `never` when
/// `mcp` is not among its surfaces.
pub fn mcp_projection(def: &CommandDef) -> Projection {
    let declared = def.effective_projection();
    let ceiling = max_projection(def.side_effect);
    let p = if declared < ceiling { declared } else { ceiling };
    if def.exposure.contains(&Surface::Mcp) {
        p
    } else {
        Projection::Never
    }
}

/// The projection check (REQ-001, REQ-002). Off MCP channels it always passes. On MCP:
/// `never` is `policy/not_on_mcp`; `request_only` or `human_only` is
/// [`Projected::PendingRequest`]; `exposed` is [`Projected::Run`]. Pure.
pub fn authorise_projection(channel: ChannelKind, def: &CommandDef) -> Result<Projected, BusError> {
    if !is_mcp_channel(channel) {
        return Ok(Projected::Run);
    }
    match mcp_projection(def) {
        Projection::Never => {
            Err(policy(PolicyReason::NotOnMcp, &format!("{}@{} is not available over MCP", def.id, def.major)))
        }
        Projection::RequestOnly => Ok(Projected::PendingRequest),
        Projection::Exposed if def.human_only => Ok(Projected::PendingRequest),
        Projection::Exposed => Ok(Projected::Run),
    }
}

/// The decision for one call.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Run the command with the derived principal.
    Run,
    /// Run the command as the approval of a pending request; the commit must carry
    /// [`Approval::event`].
    RunApproved(Approval),
    /// Do not run: file a pending request ([`Filing::commit`]).
    File(Filing),
}

/// What [`authorise`] decided, with the principal it derived.
#[derive(Debug, Clone, PartialEq)]
pub struct Authorised {
    principal: Principal,
    decision: Decision,
}

impl Authorised {
    /// The principal the bus derived from the channel.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// The decision.
    pub fn decision(&self) -> &Decision {
        &self.decision
    }

    /// Principal and decision.
    pub fn into_parts(self) -> (Principal, Decision) {
        (self.principal, self.decision)
    }
}

/// The projection and approval step for one call. The principal is derived from `channel`
/// by `table` and nowhere else. `params` are the call's parameters; `approval_of` the request
/// a person approves, taken from the invocation, never from the parameters; `view` the
/// state the call runs against (the request records and the branch head).
///
/// With `approval_of`: a principal that is not `human`, or a call on an MCP channel, is
/// `policy/bad_approval`; an unknown request is `not_found/unknown_request`; a request that is
/// not pending, names another command or major, or whose parameters commitment differs from
/// `params` is `policy/bad_approval`. Otherwise the projection check applies, for an MCP
/// channel or any `agent` principal. Pure: commits nothing and changes no request.
pub fn authorise(
    table: &SessionTable,
    channel: &dyn Channel,
    def: &CommandDef,
    params: &CValue,
    approval_of: Option<&RequestId>,
    view: &dyn StateView,
) -> Result<Authorised, BusError> {
    let principal = table.derive_principal(channel)?;
    let kind = channel.kind();
    let on_mcp = is_mcp_channel(kind) || principal.kind() == PrincipalKind::Agent;
    let decision = match approval_of {
        Some(request) => Decision::RunApproved(approve(&principal, on_mcp, def, params, request, view)?),
        None => {
            let as_channel = if on_mcp { ChannelKind::Mcp } else { kind };
            match authorise_projection(as_channel, def)? {
                Projected::Run => Decision::Run,
                Projected::PendingRequest => Decision::File(Filing::new(&principal, def, params, view)?),
            }
        }
    };
    Ok(Authorised { principal, decision })
}

fn bad_approval(detail: &str) -> BusError {
    policy(PolicyReason::BadApproval, detail)
}

fn approve(
    principal: &Principal,
    on_mcp: bool,
    def: &CommandDef,
    params: &CValue,
    request: &RequestId,
    view: &dyn StateView,
) -> Result<Approval, BusError> {
    if principal.kind() != PrincipalKind::Human || on_mcp {
        return Err(bad_approval(&format!(
            "only a human outside MCP may approve a request; caller is {}",
            principal.kind().as_str()
        )));
    }
    let record = RequestRecord::read(view, request)?.ok_or_else(|| unknown_request(request))?;
    if record.state != RequestState::Pending {
        return Err(bad_approval(&format!("request {request} is {}, not pending", record.state.as_str())));
    }
    let command = command_id(def)?;
    if record.command != command {
        return Err(bad_approval(&format!("request {request} is for another command than {command}")));
    }
    if record.params_commitment != params_commitment(params)? {
        return Err(bad_approval(&format!("parameters differ from those of request {request}")));
    }
    Ok(Approval {
        request: *request,
        record_digest: record.record_digest()?,
        command,
        params_commitment: record.params_commitment,
        requested_by: record.requester.clone(),
        approved_by: principal.reference(),
    })
}

/// A checked approval of one pending request (REQ-003). Built only by [`authorise`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    request: RequestId,
    record_digest: Digest,
    command: CommandId,
    params_commitment: Digest,
    requested_by: PrincipalRef,
    approved_by: PrincipalRef,
}

impl Approval {
    /// The approved request.
    pub fn request(&self) -> RequestId {
        self.request
    }

    /// Who filed the request.
    pub fn requested_by(&self) -> &PrincipalRef {
        &self.requested_by
    }

    /// The approving person.
    pub fn approved_by(&self) -> &PrincipalRef {
        &self.approved_by
    }

    /// The `RequestApproved` event the approved command's commit carries:
    /// `{approved_by, command, params_commitment, record_digest, request, requested_by}`.
    pub fn event(&self) -> Result<Event, BusError> {
        let body = obj([
            ("approved_by", principal_value(&self.approved_by)),
            ("command", text(self.command.as_str())),
            ("params_commitment", text(self.params_commitment.as_str())),
            ("record_digest", text(self.record_digest.as_str())),
            ("request", text(&self.request.to_string())),
            ("requested_by", principal_value(&self.requested_by)),
        ]);
        Ok(Event::new(EventType::new(REQUEST_APPROVED)?, body))
    }
}

// ------------------------------------------------------------------ filing

/// Where a filing entry goes in the Record Journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordContext {
    pub stream: String,
    pub branch: String,
    pub manifests: Manifests,
}

/// A pending request to be filed: the command did not run. Built only by [`authorise`].
#[derive(Debug, Clone, PartialEq)]
pub struct Filing {
    command: CommandId,
    params: CValue,
    params_commitment: Digest,
    requester: Principal,
    filed_at_head: Digest,
}

impl Filing {
    fn new(principal: &Principal, def: &CommandDef, params: &CValue, view: &dyn StateView) -> Result<Filing, BusError> {
        Ok(Filing {
            command: command_id(def)?,
            params: params.clone(),
            params_commitment: params_commitment(params)?,
            requester: principal.clone(),
            filed_at_head: to_digest(view.head()),
        })
    }

    /// The command the request is for.
    pub fn command(&self) -> &CommandId {
        &self.command
    }

    /// The parameters commitment.
    pub fn params_commitment(&self) -> Digest {
        self.params_commitment
    }

    /// The `RequestFiled` event of this filing under `id`.
    pub fn event(&self, id: RequestId) -> Result<Event, BusError> {
        Ok(Event::new(EventType::new(REQUEST_FILED)?, self.filed_body(id)))
    }

    fn filed_body(&self, id: RequestId) -> CValue {
        filed_body(id, &self.command, self.params_commitment, &self.requester.reference(), self.filed_at_head)
    }

    /// Appends the one W1 `request.file_request@1` entry of this filing under the observed
    /// request ID `id` and returns `id` (REQ-001). An `id` already in `view` is
    /// `conflict/id_collision`. The entry's principal is the requester; its arguments are
    /// `{command, params}`; its one event is `RequestFiled`; its inputs carry the
    /// parameters commitment (tag [`PARAMS_TAG`]) and its output the record digest. On any
    /// error nothing was appended.
    pub fn commit(
        &self,
        id: RequestId,
        view: &dyn StateView,
        time: EntryTime,
        ctx: &RecordContext,
        journal: &mut dyn JournalAppend,
    ) -> Result<RequestId, BusError> {
        if view.entity(&id.entity_id()?).is_some() {
            return Err(BusError::Conflict(ConflictReason::IdCollision, Detail::new(&format!("request {id} exists"))));
        }
        let event = self.event(id)?;
        let record_digest = canon_digest(event.body(), "request record")?;
        let ai = match self.requester.kind() {
            PrincipalKind::Agent => AiAssistance::Executed,
            PrincipalKind::Human
            | PrincipalKind::Script
            | PrincipalKind::Service
            | PrincipalKind::Device
            | PrincipalKind::System => AiAssistance::None,
        };
        let draft = EntryDraft {
            stream: ctx.stream.clone(),
            branch: ctx.branch.clone(),
            logical_parent: None,
            time,
            principal: self.requester.to_draft(),
            ai_assistance: ai,
            manifests: ctx.manifests,
            reason: None,
            human_assertion: None,
            body: DraftBody::Command(CommandDraft {
                id: FILE_REQUEST.to_string(),
                major: 1,
                class: SideEffect::W1,
                determinism: Determinism::D0,
                seed: 0,
                args: obj([("command", text(self.command.as_str())), ("params", self.params.clone())]),
                events: vec![event.to_value()],
                inputs: vec![TaggedDigest {
                    tag: PARAMS_TAG.to_string(),
                    digest: CanonDigest(self.params_commitment.to_bytes()),
                }],
                consumed_r1: Vec::new(),
                outputs: vec![OutputDigest {
                    entity: Some(id.to_string()),
                    digest: CanonDigest(record_digest.to_bytes()),
                }],
                algorithms: Vec::new(),
            }),
        };
        journal.append(&draft)?;
        Ok(id)
    }
}

fn filed_body(
    id: RequestId,
    command: &CommandId,
    params_commitment: Digest,
    requester: &PrincipalRef,
    head: Digest,
) -> CValue {
    obj([
        ("command", text(command.as_str())),
        ("filed_at_head", text(head.as_str())),
        ("params_commitment", text(params_commitment.as_str())),
        ("request", text(&id.to_string())),
        ("requester", principal_value(requester)),
    ])
}

// ------------------------------------------------------------------ records

/// One pending-request record, as folded from the journal.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestRecord {
    id: RequestId,
    seq: u64,
    command: CommandId,
    params_commitment: Digest,
    requester: PrincipalRef,
    filed_at_head: Digest,
    state: RequestState,
    resolved_by: Option<PrincipalRef>,
    reason: Option<Untrusted<String>>,
}

impl RequestRecord {
    pub fn id(&self) -> RequestId {
        self.id
    }

    /// Filing sequence, from 1, in journal order.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn command(&self) -> &CommandId {
        &self.command
    }

    pub fn params_commitment(&self) -> Digest {
        self.params_commitment
    }

    pub fn requester(&self) -> &PrincipalRef {
        &self.requester
    }

    pub fn filed_at_head(&self) -> Digest {
        self.filed_at_head
    }

    pub fn state(&self) -> RequestState {
        self.state
    }

    /// The person who approved or rejected it, or the requester who withdrew it.
    pub fn resolved_by(&self) -> Option<&PrincipalRef> {
        self.resolved_by.as_ref()
    }

    /// The rejection reason, untrusted.
    pub fn reason(&self) -> Option<&Untrusted<String>> {
        self.reason.as_ref()
    }

    /// The digest of the record as filed (the `RequestFiled` body).
    pub fn record_digest(&self) -> Result<Digest, BusError> {
        canon_digest(&self.filed_body(), "request record")
    }

    fn filed_body(&self) -> CValue {
        filed_body(self.id, &self.command, self.params_commitment, &self.requester, self.filed_at_head)
    }

    /// The record as published by `request.list_requests@1` and as its entity body.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("request".to_string(), text(&self.id.to_string()));
        // Exact below 2^53 filings.
        o.insert("seq".to_string(), CValue::Num(self.seq as f64));
        o.insert("command".to_string(), text(self.command.as_str()));
        o.insert("params_commitment".to_string(), text(self.params_commitment.as_str()));
        o.insert("requester".to_string(), principal_value(&self.requester));
        o.insert("filed_at_head".to_string(), text(self.filed_at_head.as_str()));
        o.insert("state".to_string(), text(self.state.as_str()));
        if let Some(p) = &self.resolved_by {
            o.insert("resolved_by".to_string(), principal_value(p));
        }
        if let Some(r) = &self.reason {
            o.insert("reason".to_string(), text(r.as_untrusted()));
        }
        CValue::Obj(o)
    }

    /// Reads a record from its entity body; `validation/schema` if malformed.
    pub fn from_value(v: &CValue) -> Result<RequestRecord, BusError> {
        let what = "request record";
        let o = fields(
            v,
            what,
            &["command", "filed_at_head", "params_commitment", "request", "requester", "seq", "state"],
            &["reason", "resolved_by"],
        )?;
        let seq = match o.get("seq") {
            Some(CValue::Num(n)) if n.fract() == 0.0 && (1.0..=MAX_EXACT_SEQ).contains(n) => *n as u64,
            _ => return Err(schema("request record seq must be an integer from 1")),
        };
        let state = RequestState::from_wire(str_field(o, "state", what)?).ok_or_else(|| schema("bad request state"))?;
        let reason = match o.get("reason") {
            None => None,
            Some(CValue::Str(s)) => Some(Untrusted::new(s.clone())),
            Some(_) => return Err(schema("request record reason must be a string")),
        };
        let resolved_by = match o.get("resolved_by") {
            None => None,
            some => Some(principal_from(some, "request record resolved_by")?),
        };
        Ok(RequestRecord {
            id: RequestId::parse(str_field(o, "request", what)?)?,
            seq,
            command: CommandId::parse(str_field(o, "command", what)?).map_err(|_| schema("bad command id"))?,
            params_commitment: digest_field(o, "params_commitment", what)?,
            requester: principal_from(o.get("requester"), "request record requester")?,
            filed_at_head: digest_field(o, "filed_at_head", what)?,
            state,
            resolved_by,
            reason,
        })
    }

    /// The record of `id` in `view`, if any; an entity of another kind under that ID, or a
    /// malformed body, is `internal/invariant_violated`.
    pub fn read(view: &dyn StateView, id: &RequestId) -> Result<Option<RequestRecord>, BusError> {
        let Some(e) = view.entity(&id.entity_id()?) else { return Ok(None) };
        if e.kind != REQUEST_KIND {
            return Err(invariant(&format!("entity {id} is not a request")));
        }
        let r = RequestRecord::from_value(e.body).map_err(|_| invariant(&format!("request {id} is malformed")))?;
        if r.id != *id {
            return Err(invariant(&format!("request {id} is stored under another id")));
        }
        Ok(Some(r))
    }
}

struct Stored {
    entity: EntityId,
    record: RequestRecord,
    body: CValue,
}

/// The pending-request records, folded from committed entries in journal order. Changed
/// only by [`RequestBook::apply_entry`].
#[derive(Default)]
pub struct RequestBook {
    map: BTreeMap<RequestId, Stored>,
    filed: u64,
}

impl fmt::Debug for RequestBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestBook").field("requests", &self.map.len()).finish()
    }
}

/// One parsed request event.
enum Change {
    Filed(RequestRecord),
    Approved { id: RequestId, record_digest: Digest, command: CommandId, requested_by: PrincipalRef, by: PrincipalRef },
    Withdrawn { id: RequestId, by: PrincipalRef },
    Rejected { id: RequestId, by: PrincipalRef, reason: Untrusted<String> },
}

impl Change {
    fn id(&self) -> RequestId {
        match self {
            Change::Filed(r) => r.id,
            Change::Approved { id, .. } | Change::Withdrawn { id, .. } | Change::Rejected { id, .. } => *id,
        }
    }

    fn parse(e: &Event) -> Result<Option<Change>, BusError> {
        let body = e.body();
        let ty = e.type_id().as_str();
        let change = if ty == REQUEST_FILED {
            let w = "RequestFiled";
            let o = fields(body, w, &["command", "filed_at_head", "params_commitment", "request", "requester"], &[])?;
            Change::Filed(RequestRecord {
                id: RequestId::parse(str_field(o, "request", w)?)?,
                seq: 0,
                command: CommandId::parse(str_field(o, "command", w)?).map_err(|_| schema("bad command id"))?,
                params_commitment: digest_field(o, "params_commitment", w)?,
                requester: principal_from(o.get("requester"), "RequestFiled.requester")?,
                filed_at_head: digest_field(o, "filed_at_head", w)?,
                state: RequestState::Pending,
                resolved_by: None,
                reason: None,
            })
        } else if ty == REQUEST_APPROVED {
            let w = "RequestApproved";
            let keys = ["approved_by", "command", "params_commitment", "record_digest", "request", "requested_by"];
            let o = fields(body, w, &keys, &[])?;
            Change::Approved {
                id: RequestId::parse(str_field(o, "request", w)?)?,
                record_digest: digest_field(o, "record_digest", w)?,
                command: CommandId::parse(str_field(o, "command", w)?).map_err(|_| schema("bad command id"))?,
                requested_by: principal_from(o.get("requested_by"), "RequestApproved.requested_by")?,
                by: principal_from(o.get("approved_by"), "RequestApproved.approved_by")?,
            }
        } else if ty == REQUEST_WITHDRAWN {
            let w = "RequestWithdrawn";
            let o = fields(body, w, &["request", "withdrawn_by"], &[])?;
            Change::Withdrawn {
                id: RequestId::parse(str_field(o, "request", w)?)?,
                by: principal_from(o.get("withdrawn_by"), "RequestWithdrawn.withdrawn_by")?,
            }
        } else if ty == REQUEST_REJECTED {
            let w = "RequestRejected";
            let o = fields(body, w, &["reason", "reason_commitment", "rejected_by", "request"], &[])?;
            let reason = Untrusted::new(str_field(o, "reason", w)?.to_string());
            if text_commitment(&reason)? != digest_field(o, "reason_commitment", w)? {
                return Err(schema("RequestRejected reason does not match its commitment"));
            }
            Change::Rejected {
                id: RequestId::parse(str_field(o, "request", w)?)?,
                by: principal_from(o.get("rejected_by"), "RequestRejected.rejected_by")?,
                reason,
            }
        } else {
            return Ok(None);
        };
        Ok(Some(change))
    }
}

impl RequestBook {
    /// An empty book.
    pub fn new() -> Self {
        RequestBook::default()
    }

    /// Number of records.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the book is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The record of `id`.
    pub fn get(&self, id: &RequestId) -> Option<&RequestRecord> {
        self.map.get(id).map(|s| &s.record)
    }

    /// Every record, by filing sequence.
    pub fn records(&self) -> Vec<&RequestRecord> {
        let mut v: Vec<&RequestRecord> = self.map.values().map(|s| &s.record).collect();
        v.sort_by_key(|r| r.seq);
        v
    }

    /// The book as a [`StateView`] at branch head `head`: each record is an entity of kind
    /// [`REQUEST_KIND`] under its request ID; there are no settings.
    pub fn view(&self, head: CanonDigest) -> BookView<'_> {
        BookView { book: self, head }
    }

    fn pending(&self, id: &RequestId) -> Result<&RequestRecord, BusError> {
        let r = self.get(id).ok_or_else(|| invariant(&format!("event for unknown request {id}")))?;
        if r.state == RequestState::Pending {
            Ok(r)
        } else {
            Err(invariant(&format!("event for request {id}, which is {}", r.state.as_str())))
        }
    }

    fn check(&self, c: &Change) -> Result<(), BusError> {
        match c {
            Change::Filed(r) => {
                if self.map.contains_key(&r.id) {
                    return Err(invariant(&format!("request {} filed twice", r.id)));
                }
            }
            Change::Approved { id, record_digest, command, requested_by, by } => {
                let r = self.pending(id)?;
                let same =
                    r.record_digest()? == *record_digest && r.command == *command && r.requester == *requested_by;
                if !same || by.kind != PrincipalKind::Human {
                    return Err(invariant(&format!("approval of request {id} does not match its record")));
                }
            }
            Change::Withdrawn { id, by } => {
                if self.pending(id)?.requester != *by {
                    return Err(invariant(&format!("request {id} withdrawn by another principal")));
                }
            }
            Change::Rejected { id, by, .. } => {
                self.pending(id)?;
                if by.kind != PrincipalKind::Human {
                    return Err(invariant(&format!("request {id} rejected by a non-human principal")));
                }
            }
        }
        Ok(())
    }

    /// Folds the events of one committed entry, in order. Events of other types are ignored.
    /// Each request may be touched by at most one event of the entry. A malformed request
    /// event, or one the records contradict (unknown or not pending request, a second filing,
    /// an approval whose digest, command or requester differs, a withdrawal by another
    /// principal, a non-human approver or rejecter), is `internal/invariant_violated`, and
    /// then the book is unchanged.
    pub fn apply_entry(&mut self, events: &[Event]) -> Result<(), BusError> {
        let mut changes = Vec::new();
        let mut touched = BTreeSet::new();
        for e in events {
            let Some(c) = Change::parse(e).map_err(|err| invariant(&format!("malformed request event: {err}")))? else {
                continue;
            };
            if !touched.insert(c.id()) {
                return Err(invariant(&format!("one entry touches request {} twice", c.id())));
            }
            self.check(&c)?;
            changes.push(c);
        }
        let mut next = self.filed;
        let mut stored = Vec::with_capacity(changes.len());
        for c in changes {
            let record = match c {
                Change::Filed(mut r) => {
                    next = next
                        .checked_add(1)
                        .filter(|n| *n <= MAX_SEQ)
                        .ok_or_else(|| invariant("filing sequence exhausted"))?;
                    r.seq = next;
                    r
                }
                Change::Approved { id, by, .. } => self.resolved(&id, RequestState::Approved, by, None)?,
                Change::Withdrawn { id, by } => self.resolved(&id, RequestState::Withdrawn, by, None)?,
                Change::Rejected { id, by, reason } => self.resolved(&id, RequestState::Rejected, by, Some(reason))?,
            };
            stored.push(Stored { entity: record.id.entity_id()?, body: record.to_value(), record });
        }
        self.filed = next;
        for s in stored {
            self.map.insert(s.record.id, s);
        }
        Ok(())
    }

    fn resolved(
        &self,
        id: &RequestId,
        state: RequestState,
        by: PrincipalRef,
        reason: Option<Untrusted<String>>,
    ) -> Result<RequestRecord, BusError> {
        let mut r = self.get(id).cloned().ok_or_else(|| invariant(&format!("request {id} vanished")))?;
        r.state = state;
        r.resolved_by = Some(by);
        r.reason = reason;
        Ok(r)
    }
}

/// A [`RequestBook`] as a read-only [`StateView`].
#[derive(Debug, Clone, Copy)]
pub struct BookView<'a> {
    book: &'a RequestBook,
    head: CanonDigest,
}

impl StateView for BookView<'_> {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        let rid = RequestId::parse(id.as_str()).ok()?;
        let s = self.book.map.get(&rid)?;
        Some(EntityRef { id: &s.entity, kind: REQUEST_KIND, body: &s.body })
    }

    fn head(&self) -> CanonDigest {
        self.head
    }

    fn setting(&self, _key: &SettingKey) -> Option<CValue> {
        None
    }
}

// ------------------------------------------------------------------ commands

fn request_param(o: &BTreeMap<String, CValue>, what: &str) -> Result<RequestId, BusError> {
    RequestId::parse(str_field(o, "request", what)?)
}

/// `request.list_requests@1` (REQ-007): the caller's own requests (same principal kind and
/// identity), or every request for a `human` caller, by filing sequence, optionally only
/// those in `state`. Parameters `{state?}`; result `{requests: [record]}`.
pub fn list_requests(book: &RequestBook, caller: &Principal, params: &CValue) -> Result<CValue, BusError> {
    let o = fields(params, "parameters", &[], &["state"])?;
    let state = match o.get("state") {
        None => None,
        Some(CValue::Str(s)) => Some(RequestState::from_wire(s).ok_or_else(|| schema("unknown request state"))?),
        Some(_) => return Err(schema("parameters.state must be a string")),
    };
    let all = caller.kind() == PrincipalKind::Human;
    let me = caller.reference();
    let requests = book
        .records()
        .into_iter()
        .filter(|r| all || r.requester == me)
        .filter(|r| state.is_none_or(|s| r.state == s))
        .map(RequestRecord::to_value)
        .collect();
    Ok(obj([("requests", CValue::Arr(requests))]))
}

/// The handler of `request.withdraw_request@1` (REQ-005): parameters `{request}`; only the
/// requester (same kind and identity) may withdraw (`policy/not_requester`); the request must
/// be pending (`precondition/invalid_transition`); an unknown request is
/// `not_found/unknown_request`. Returns one `RequestWithdrawn` event.
#[derive(Debug, Clone, Copy, Default)]
pub struct WithdrawHandler;

impl Handler for WithdrawHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let o = fields(params.value(), "parameters", &["request"], &[])?;
        let id = request_param(o, "parameters")?;
        let record = RequestRecord::read(view, &id)?.ok_or_else(|| unknown_request(&id))?;
        let me = ctx.principal().reference();
        if record.requester != me {
            return Err(policy(PolicyReason::NotRequester, &format!("only the requester may withdraw {id}")).into());
        }
        if record.state != RequestState::Pending {
            return Err(not_pending(&id, record.state).into());
        }
        let body = obj([("request", text(&id.to_string())), ("withdrawn_by", principal_value(&me))]);
        Ok(vec![Event::new(EventType::new(REQUEST_WITHDRAWN)?, body)])
    }
}

/// The handler of `request.reject_request@1` (REQ-006): parameters `{request, reason}`, the
/// reason 1 to [`MAX_REASON_CHARS`] characters of untrusted text (else `validation/schema`);
/// only a `human` may reject (`policy/human_only`, also enforced by the class check); the
/// request must be pending (`precondition/invalid_transition`); an unknown request is
/// `not_found/unknown_request`. Returns one `RequestRejected` event holding the reason and
/// its commitment ([`text_commitment`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct RejectHandler;

impl Handler for RejectHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let o = fields(params.value(), "parameters", &["reason", "request"], &[])?;
        let id = request_param(o, "parameters")?;
        let reason = str_field(o, "reason", "parameters")?;
        let n = reason.chars().count();
        if n == 0 || n > MAX_REASON_CHARS {
            return Err(schema(&format!("reason must be 1 to {MAX_REASON_CHARS} characters")).into());
        }
        let p = ctx.principal();
        if p.kind() != PrincipalKind::Human {
            return Err(policy(PolicyReason::HumanOnly, "only a human may reject a request").into());
        }
        let record = RequestRecord::read(view, &id)?.ok_or_else(|| unknown_request(&id))?;
        if record.state != RequestState::Pending {
            return Err(not_pending(&id, record.state).into());
        }
        let reason = Untrusted::new(reason.to_string());
        let body = obj([
            ("reason", text(reason.as_untrusted())),
            ("reason_commitment", text(text_commitment(&reason)?.as_str())),
            ("rejected_by", principal_value(&p.reference())),
            ("request", text(&id.to_string())),
        ]);
        Ok(vec![Event::new(EventType::new(REQUEST_REJECTED)?, body)])
    }
}

// ------------------------------------------------------------------ definitions

fn request_id_schema() -> CValue {
    obj([
        ("type", text("string")),
        ("pattern", text(UUID_PATTERN)),
        ("description", text("Request ID, a lowercase UUIDv7.")),
    ])
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn digest_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(Digest::PATTERN)), ("description", text(description))])
}

fn principal_schema(description: &str) -> CValue {
    let kinds: Vec<&str> = PrincipalKind::ALL.iter().map(|k| k.as_str()).collect();
    let mut o = BTreeMap::new();
    o.insert("type".to_string(), text("object"));
    o.insert("additionalProperties".to_string(), CValue::Bool(false));
    o.insert("required".to_string(), strs(&["id", "kind"]));
    o.insert(
        "properties".to_string(),
        obj([("id", obj([("type", text("string"))])), ("kind", obj([("enum", strs(&kinds))]))]),
    );
    o.insert("description".to_string(), text(description));
    CValue::Obj(o)
}

fn state_schema() -> CValue {
    let states: Vec<&str> = RequestState::ALL.iter().map(|s| s.as_str()).collect();
    obj([("enum", strs(&states))])
}

fn record_schema() -> CValue {
    object(
        &["command", "filed_at_head", "params_commitment", "request", "requester", "seq", "state"],
        obj([
            ("request", request_id_schema()),
            ("seq", obj([("type", text("integer")), ("minimum", CValue::Num(1.0))])),
            ("command", obj([("type", text("string")), ("pattern", text(CommandId::PATTERN))])),
            ("params_commitment", digest_schema("Digest of the request's canonical parameters.")),
            ("requester", principal_schema("Who filed the request.")),
            ("filed_at_head", digest_schema("Branch head when the request was filed.")),
            ("state", state_schema()),
            ("resolved_by", principal_schema("Who approved, rejected or withdrew it.")),
            (
                "reason",
                obj([
                    ("type", text("string")),
                    ("x-untrusted", CValue::Bool(true)),
                    ("description", text("Rejection reason, untrusted text.")),
                ]),
            ),
        ]),
    )
}

fn empty_object() -> CValue {
    object(&[], CValue::Obj(BTreeMap::new()))
}

struct Spec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    side_effect: &'static str,
    projection: &'static str,
    idempotency: &'static str,
    effects: &'static [&'static str],
    exposure: &'static [&'static str],
    human_only: bool,
}

fn def_value(d: Spec) -> CValue {
    obj([
        ("id", text(d.name)),
        ("major", CValue::Num(1.0)),
        ("title", text(d.title)),
        ("description", text(d.description)),
        ("params_schema", d.params),
        ("result_schema", d.result),
        ("side_effect", text(d.side_effect)),
        ("mcp_projection", text(d.projection)),
        ("determinism", text("D0")),
        ("idempotency", text(d.idempotency)),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(d.effects)),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(d.exposure)),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(d.human_only)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ])
}

/// The four request commands, in the order list, withdraw, reject, file. Built through
/// `CommandDef::from_value`, so every field rule of the registry applies.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let request_only = || object(&["request"], obj([("request", request_id_schema())]));
    let reason = obj([
        ("type", text("string")),
        ("minLength", CValue::Num(1.0)),
        ("maxLength", CValue::Num(2000.0)),
        ("x-untrusted", CValue::Bool(true)),
        ("description", text("Why the request is rejected; untrusted text, 1 to 2000 characters.")),
    ]);
    let filed_result = object(&["request"], obj([("request", request_id_schema())]));
    let specs = [
        Spec {
            name: LIST_REQUESTS,
            title: "List pending requests",
            description: "Lists the caller's own requests, or every request for a human, by filing sequence, \
                          optionally only those in one state.",
            params: object(&[], obj([("state", state_schema())])),
            result: object(
                &["requests"],
                obj([("requests", obj([("type", text("array")), ("items", record_schema())]))]),
            ),
            side_effect: "R0",
            projection: "exposed",
            idempotency: "natural",
            effects: &[],
            exposure: &["cli", "mcp"],
            human_only: false,
        },
        Spec {
            name: WITHDRAW_REQUEST,
            title: "Withdraw a request",
            description: "Withdraws a pending request; only the principal that filed it may.",
            params: request_only(),
            result: empty_object(),
            side_effect: "W1",
            projection: "exposed",
            idempotency: "none",
            effects: &[REQUEST_WITHDRAWN],
            exposure: &["cli", "mcp"],
            human_only: false,
        },
        Spec {
            name: REJECT_REQUEST,
            title: "Reject a request",
            description: "Rejects a pending request with a reason, kept as untrusted text. Human only; not on MCP.",
            params: object(&["reason", "request"], obj([("reason", reason), ("request", request_id_schema())])),
            result: empty_object(),
            side_effect: "W1",
            projection: "never",
            idempotency: "none",
            effects: &[REQUEST_REJECTED],
            exposure: &["cli", "gui"],
            human_only: true,
        },
        Spec {
            name: FILE_REQUEST,
            title: "File a pending request",
            description: "Internal: written by the bus when an MCP call becomes a pending request. \
                          Never invoked by a surface.",
            params: object(
                &["command", "params"],
                obj([
                    ("command", obj([("type", text("string")), ("pattern", text(CommandId::PATTERN))])),
                    ("params", obj([("type", text("object"))])),
                ]),
            ),
            result: filed_result,
            side_effect: "W1",
            projection: "never",
            idempotency: "none",
            effects: &[REQUEST_FILED],
            exposure: &[],
            human_only: false,
        },
    ];
    specs.into_iter().map(|d| CommandDef::from_value(def_value(d))).collect()
}
