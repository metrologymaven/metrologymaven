//! `journal.supersede_record@1` on the bus (spec node `CORE.JRN.UNDO.SUPER`, bus half; the
//! planner, fold and definition are in `fid_journal::supersede`, layer model).
//!
//! [`run_supersede`] parses a call, takes the caller from the call's principal (derived by the
//! bus from the channel, never from the parameters), reads the folded journal, the call's
//! branch and that branch's logical head from a [`SupersedeSource`], runs
//! `fid_journal::supersede::plan`, checks the event with `Supersessions::precommit` under the
//! record its entry will have (principal, parameters commitment of the call as the bus computes
//! it, reason commitment and its opening), and returns the one `RecordSuperseded` event and the
//! result `{supersession}`.
//! [`supersede_bus_error`] maps every refusal to the bus error of the same code and reason
//! (catalogue v3).

use super::{invariant, schema, text};
use fid_bus::error::{
    BusError, ConflictReason, Detail, InternalReason, PolicyReason, PreconditionReason, ValidationReason,
};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_bus::policy::projection::params_commitment;
use fid_canon::CValue;
use fid_journal::branch::{EntryOrigin, EventSource};
use fid_journal::supersede::{
    command_defs, plan, reason_commitment, tombstone, ReasonOpening, SupersedeEntry, SupersedeError, SupersedeParams,
    Supersessions, RECORD_SUPERSEDED, SUPERSEDE_RECORD,
};
use fid_journal::{PrincipalKind, Salt, SALT_BYTES};
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Where the handler reads the journal. The bus wiring implements it.
///
/// Contract:
///
/// - [`SupersedeSource::state`] is `Supersessions::apply_entry` over every committed entry of
///   **every branch** in commit order (lookups walk a head's logical ancestry, so forks and
///   fast-forwards see the supersessions before them), each with the `SupersedeEntry` its
///   verified header and payload record (the journal `EntryOrigin` with its branch and adding
///   stage, the principal's identity commitment, the bus `EntryOrigin` parameters commitment,
///   the entry hash, `promoted_from`, `reason_commit` and, while the payload holds it, its
///   opening, the output digests, and the logical head of its branch before it), and the
///   entry's events checked against the header's `events_commit`.
/// - The commit stage records the call's `reason` parameter as the entry's `reason` (a salted
///   commitment, `CORE.JRN.ENTRY`), and commits the entry only if
///   `Supersessions::precommit` with the entry's real record and the call's parameters accepts
///   it on the state as of the commit, atomically with the commit (compare-and-swap on the
///   event's `head`; `conflict/head_moved` otherwise). A committed entry the fold refuses is
///   then a commit-stage bug or a forged journal.
/// - [`SupersedeSource::call_branch`] is the branch the commit stage would commit the call's
///   entry to, and [`SupersedeSource::head`] a branch's logical head (`None`: empty); after a
///   fork or a fast-forward that is the head the branch now points at.
pub trait SupersedeSource: Send + Sync {
    /// The folded journal.
    fn state(&self) -> Result<Supersessions, String>;
    /// The branch the entry of the call `ctx` would commit to.
    fn call_branch(&self, ctx: &HandlerCtx) -> Result<String, String>;
    /// The logical head of `branch`.
    fn head(&self, branch: &str) -> Result<Option<Digest>, String>;
}

/// The bus error of a refusal: same code, same reason (catalogue v3).
pub fn supersede_bus_error(e: &SupersedeError) -> BusError {
    let d = Detail::new(&e.detail());
    match e {
        SupersedeError::ClassForbidden => BusError::Policy(PolicyReason::ClassForbidden, d),
        SupersedeError::Schema(_) => BusError::Validation(ValidationReason::Schema, d),
        SupersedeError::NotSupersedable(_) => BusError::Precondition(PreconditionReason::NotSupersedable, d),
        SupersedeError::HeadMoved(_) => BusError::Conflict(ConflictReason::HeadMoved, d),
        SupersedeError::Tampered(_) => BusError::Internal(InternalReason::InvariantViolated, d),
    }
}

fn digest_param(params: &CValue, key: &str) -> Result<Digest, BusError> {
    Digest::parse(text(params, key)?).map_err(|_| schema(&format!("{key} is not a sha256: digest")))
}

/// The parameters of one call.
pub fn parse_supersede(params: &CValue) -> Result<SupersedeParams, BusError> {
    if let CValue::Obj(o) = params {
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "new" | "old" | "reason")) {
            return Err(schema(&format!("unknown parameter {:?}", k.chars().take(64).collect::<String>())));
        }
    }
    Ok(SupersedeParams {
        old: digest_param(params, "old")?,
        new: digest_param(params, "new")?,
        reason: text(params, "reason")?.to_string(),
    })
}

/// The record the commit stage gives the entry of call `params` (as the bus received it) by a
/// principal of `kind` on `branch` decided against `head`: the handler's own events, W3, the
/// bus parameters commitment of `params`, the reason committed under a placeholder salt with
/// its opening (the real salt, principal commitment and entry hash are the commit stage's;
/// placeholders here), no outputs and no `promoted_from`.
pub fn entry_for_call(
    kind: PrincipalKind,
    branch: &str,
    head: Option<Digest>,
    params: &CValue,
    reason: &str,
) -> Result<SupersedeEntry, BusError> {
    let salt = Salt([0; SALT_BYTES]);
    Ok(SupersedeEntry {
        origin: EntryOrigin {
            kind,
            command: SUPERSEDE_RECORD.to_string(),
            major: 1,
            class: SideEffect::W3,
            branch: branch.to_string(),
            source: EventSource::Handler,
            human_assertion: None,
        },
        principal: tombstone(),
        params_commitment: Some(params_commitment(params)?),
        hash: tombstone(),
        promoted_from: None,
        reason_commit: Some(reason_commitment(&salt, reason).map_err(|e| supersede_bus_error(&e))?),
        reason_opening: Some(ReasonOpening { salt, reason: reason.to_string() }),
        outputs: Vec::new(),
        head_before: head,
    })
}

/// Runs one call `ctx`: the event to commit and the result value `{supersession}`.
pub fn run_supersede(
    source: &dyn SupersedeSource,
    params: &CValue,
    ctx: &HandlerCtx,
) -> Result<(Vec<Event>, CValue), BusError> {
    let kind = ctx.principal().kind();
    let actor = fid_journal::branch::Actor::of(kind);
    // W3: the bus's class check refuses anyone but a human earlier; this is defence in depth,
    // before anything is read.
    if actor != fid_journal::branch::Actor::Human {
        return Err(supersede_bus_error(&SupersedeError::ClassForbidden));
    }
    let p = parse_supersede(params)?;
    let state = source.state().map_err(|e| invariant(&format!("supersessions: {e}")))?;
    let branch = source.call_branch(ctx).map_err(|e| invariant(&format!("call branch: {e}")))?;
    let head = source.head(&branch).map_err(|e| invariant(&format!("branch head: {e}")))?;
    let s = plan(actor, &state, &branch, head, &p).map_err(|e| supersede_bus_error(&e))?;
    // Precommit equals fold: the event must pass the commit stage's check under the record its
    // entry will have, bound to the call's parameters.
    let events = [(RECORD_SUPERSEDED.to_string(), s.body())];
    let entry = entry_for_call(kind, &branch, head, params, &p.reason)?;
    state.precommit(&entry, &events, &p).map_err(|e| supersede_bus_error(&e))?;
    let result = CValue::Obj(BTreeMap::from([("supersession".to_string(), s.body())]));
    let ev = Event::new(EventType::new(s.type_id())?, s.body());
    Ok((vec![ev], result))
}

/// The handler of `journal.supersede_record@1`.
pub struct SupersedeHandler {
    source: Arc<dyn SupersedeSource>,
}

impl SupersedeHandler {
    pub fn new(source: Arc<dyn SupersedeSource>) -> SupersedeHandler {
        SupersedeHandler { source }
    }
}

impl fmt::Debug for SupersedeHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SupersedeHandler").finish_non_exhaustive()
    }
}

impl Handler for SupersedeHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        Ok(run_supersede(self.source.as_ref(), params.value(), ctx)?.0)
    }
}

/// Registers the handler with its definition from `fid_journal::supersede::command_defs`.
pub fn register_supersede(set: &mut HandlerSet, source: &Arc<dyn SupersedeSource>) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| invariant(&format!("supersede definition: {e}")))?;
    let [def] = <[_; 1]>::try_from(defs).map_err(|_| invariant("expected one supersede definition"))?;
    if def.id != SUPERSEDE_RECORD {
        return Err(invariant("supersede definition out of order"));
    }
    set.register(def, Box::new(SupersedeHandler::new(Arc::clone(source))))?;
    Ok(())
}
