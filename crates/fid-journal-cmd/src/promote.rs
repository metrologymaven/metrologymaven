//! The promotion commands on the bus (spec node `CORE.JRN.BR.PROMO`, bus half; the planner,
//! fold and definitions are in `fid_journal::promote`, layer model).
//!
//! [`run_promote`] parses a call, takes the caller from the call's principal (derived by the
//! bus from the channel, never from the parameters), reads the branch table from a
//! [`BranchSource`] and the verified diff from a [`DiffSource`], runs
//! `fid_journal::promote::plan` with the wired [`PromoteWiring`], checks the event against
//! the fold with the origin its entry will record, and returns the one event and the result
//! value. [`promote_bus_error`] maps every refusal to the bus error of the same code and
//! reason (catalogue v3).

use super::{diff_bus_error, field, invariant, schema, text, BranchSource, DiffSource};
use fid_bus::error::{
    BusError, CancelledReason, ConflictReason, Detail, InternalReason, NotFoundReason, PolicyReason,
    PreconditionReason, ValidationReason,
};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_canon::CValue;
use fid_journal::branch::{Actor, EntryOrigin, EventSource};
use fid_journal::diff::{CancelFlag, DiffOptions};
use fid_journal::promote::{
    command_defs, plan, AssertionCheck, PromoEvent, PromoteError, PromoteOp, PromoteParams, Promotions, Rebaser,
};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// What the app wires: `CORE.RPL.RETGT`'s rebaser and `TRUST`'s (or `CORE.BUS.PRIN`'s) check.
/// Either may be absent; a call that needs it is `precondition/capability_missing`.
#[derive(Clone, Default)]
pub struct PromoteWiring {
    pub rebaser: Option<Arc<dyn Rebaser>>,
    pub assertion: Option<Arc<dyn AssertionCheck>>,
}

impl fmt::Debug for PromoteWiring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromoteWiring")
            .field("rebaser", &self.rebaser.is_some())
            .field("assertion", &self.assertion.is_some())
            .finish()
    }
}

/// The bus error of a promotion refusal: same code, same reason (catalogue v3).
pub fn promote_bus_error(e: &PromoteError) -> BusError {
    let d = Detail::new(&e.detail());
    match e {
        PromoteError::HumanOnly => BusError::Policy(PolicyReason::HumanOnly, d),
        PromoteError::ClassForbidden => BusError::Policy(PolicyReason::ClassForbidden, d),
        PromoteError::Schema(_) => BusError::Validation(ValidationReason::Schema, d),
        PromoteError::UnknownBranch(_) => BusError::NotFound(NotFoundReason::UnknownBranch, d),
        PromoteError::NotPromotable(_) => BusError::Precondition(PreconditionReason::False, d),
        PromoteError::TargetProtected(_) => BusError::Policy(PolicyReason::TargetProtected, d),
        PromoteError::HeadMismatch(_) => BusError::Conflict(ConflictReason::ExpectedHeadMismatch, d),
        PromoteError::DigestMismatch => BusError::Conflict(ConflictReason::ExpectedDigestMismatch, d),
        PromoteError::CapabilityMissing(_) => BusError::Precondition(PreconditionReason::CapabilityMissing, d),
        PromoteError::NoAssertion => BusError::Policy(PolicyReason::NoAssertion, d),
        PromoteError::DivergentStep { .. } => BusError::Precondition(PreconditionReason::DivergentStep, d),
        PromoteError::InputLimit(_) => BusError::Validation(ValidationReason::InputLimit, d),
        PromoteError::Rebase(_) => BusError::Internal(InternalReason::HandlerFailed, d),
        PromoteError::Cancelled => BusError::Cancelled(CancelledReason::Requested, d),
        PromoteError::Diff(e) => diff_bus_error(e),
    }
}

fn digest_param(params: &CValue, key: &str) -> Result<Digest, BusError> {
    Digest::parse(text(params, key)?).map_err(|_| schema(&format!("{key} is not a sha256: digest")))
}

fn opt_text(params: &CValue, key: &str) -> Result<Option<String>, BusError> {
    match params {
        CValue::Obj(o) => match o.get(key) {
            None => Ok(None),
            Some(CValue::Str(s)) => Ok(Some(s.clone())),
            Some(_) => Err(schema(&format!("{key} is not a string"))),
        },
        _ => Err(schema("parameters are not an object")),
    }
}

/// The parameters of one call of `op`.
pub fn parse_params(op: PromoteOp, params: &CValue) -> Result<PromoteParams, BusError> {
    let target_head = match field(params, "target_head")? {
        CValue::Null => None,
        _ => Some(digest_param(params, "target_head")?),
    };
    let assertion = match op {
        PromoteOp::Protected => Some(text(params, "assertion")?.to_string()),
        _ => None,
    };
    let note = match op {
        PromoteOp::Request => opt_text(params, "note")?,
        _ => None,
    };
    Ok(PromoteParams {
        source: text(params, "source")?.to_string(),
        source_head: digest_param(params, "source_head")?,
        target: text(params, "target")?.to_string(),
        target_head,
        diff: digest_param(params, "diff")?,
        assertion,
        note,
    })
}

/// Runs one promotion command for the call `ctx`: the event to commit and the result value
/// (`{promotion}` or `{request}`, as the `result_schema` declares it).
pub fn run_promote(
    op: PromoteOp,
    branches: &dyn BranchSource,
    journal: &dyn DiffSource,
    wiring: &PromoteWiring,
    params: &CValue,
    ctx: &HandlerCtx,
) -> Result<(Vec<Event>, CValue), BusError> {
    let kind = ctx.principal().kind();
    let p = parse_params(op, params)?;
    let table = branches.branches().map_err(|e| invariant(&format!("branch table: {e}")))?;
    let token = ctx.cancel().clone();
    let cancel_flag: CancelFlag = Arc::new(move || token.is_cancelled());
    let cancel = {
        let f = Arc::clone(&cancel_flag);
        move || f()
    };
    let opts = DiffOptions { cancel: Some(cancel_flag), ..DiffOptions::for_caller(kind) };
    let mut diff_of = || journal.diff(&table, &p.source, &p.target, &opts);
    let event = plan(
        op,
        Actor::of(kind),
        &table,
        &p,
        &mut diff_of,
        wiring.rebaser.as_deref(),
        wiring.assertion.as_deref(),
        &cancel,
    )
    .map_err(|e| promote_bus_error(&e))?;
    // The origin its entry will record: the fold must accept the event from it.
    let target = match &event {
        PromoEvent::Promoted(pr) => pr.binding.target.as_str().to_string(),
        PromoEvent::Requested(r) => r.binding.target.as_str().to_string(),
    };
    let origin = EntryOrigin {
        kind,
        command: op.command().to_string(),
        major: 1,
        class: op.class(),
        branch: if op == PromoteOp::Request { p.source.clone() } else { target },
        source: EventSource::Handler,
    };
    Promotions::default()
        .apply_entry(&table, &origin, op == PromoteOp::Protected, std::slice::from_ref(&event))
        .map_err(|e| invariant(&e.detail()))?;
    let key = if op == PromoteOp::Request { "request" } else { "promotion" };
    let result = CValue::Obj(BTreeMap::from([(key.to_string(), event.body())]));
    let ev = Event::new(EventType::new(event.type_id())?, event.body());
    Ok((vec![ev], result))
}

/// The handler of one promotion command.
pub struct PromoteHandler {
    op: PromoteOp,
    branches: Arc<dyn BranchSource>,
    journal: Arc<dyn DiffSource>,
    wiring: PromoteWiring,
}

impl PromoteHandler {
    pub fn new(
        op: PromoteOp,
        branches: Arc<dyn BranchSource>,
        journal: Arc<dyn DiffSource>,
        wiring: PromoteWiring,
    ) -> PromoteHandler {
        PromoteHandler { op, branches, journal, wiring }
    }
}

impl fmt::Debug for PromoteHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PromoteHandler").field("op", &self.op).field("wiring", &self.wiring).finish_non_exhaustive()
    }
}

impl Handler for PromoteHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        Ok(run_promote(self.op, self.branches.as_ref(), self.journal.as_ref(), &self.wiring, params.value(), ctx)?.0)
    }
}

/// Registers the three handlers, each with its definition from
/// `fid_journal::promote::command_defs`.
pub fn register_promote(
    set: &mut HandlerSet,
    branches: &Arc<dyn BranchSource>,
    journal: &Arc<dyn DiffSource>,
    wiring: &PromoteWiring,
) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| invariant(&format!("promotion definitions: {e}")))?;
    if defs.len() != PromoteOp::ALL.len() {
        return Err(invariant("promotion definitions out of order"));
    }
    for (op, def) in PromoteOp::ALL.into_iter().zip(defs) {
        if def.id != op.command() {
            return Err(invariant("promotion definitions out of order"));
        }
        let h = PromoteHandler::new(op, Arc::clone(branches), Arc::clone(journal), wiring.clone());
        set.register(def, Box::new(h))?;
    }
    Ok(())
}
