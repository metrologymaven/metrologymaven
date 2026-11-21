//! Journal branch commands on the bus. Spec node `CORE.JRN.BR.NAME` (the command half; the
//! branch table, name rules and planners are in `fid_journal::branch`, layer model).
//!
//! - [`BranchHandler`] and [`register`]: the handlers of `journal.create_branch@1`,
//!   `journal.list_branches@1`, `journal.protect_branch@1` and
//!   `journal.set_branch_coediting@1`. Each reads the branch table from a [`BranchSource`],
//!   takes the caller from the call's principal ([`HandlerCtx::principal`], derived by the bus
//!   from the channel and nothing else), runs the planner, and returns the event to commit
//!   (none for a no-op and for the R0 list).
//! - [`run`] does the same for a call's [`HandlerCtx`] and also returns the command's result
//!   value (`{branch}` or `{branches}`) as its `result_schema` declares it.
//! - [`to_bus_error`]: every planner refusal becomes the bus error of the same code and
//!   reason (catalogue v3): `validation/bad_branch_name`, `validation/schema`,
//!   `not_found/unknown_branch`, `not_found/unknown_entity`, `policy/class_forbidden`,
//!   `policy/human_only`, `internal/invariant_violated`.
//!
//! - [`DiffHandler`], [`run_diff`] and [`register_diff`]: `journal.diff_branch@1` (spec node
//!   `CORE.JRN.BR.DIFF`, R0). The handler reads the branch table from a [`BranchSource`] and
//!   the diff from a [`DiffSource`] (the journal's verified headers, `fid_journal::diff`),
//!   passes the call's cancel token through, and returns no events; [`diff_bus_error`] maps
//!   every refusal to the bus error of the same code and reason (catalogue v3). Nothing in the
//!   result comes from the caller's principal or from an event body: principal kinds are the
//!   verified entry headers', owners the origin-bound branch table's.
//!
//! - [`PromoteHandler`], [`run_promote`] and [`register_promote`] (module `promote`):
//!   `journal.promote_branch@1`, `journal.promote_protected_branch@1` and
//!   `journal.request_promote_branch@1` (spec node `CORE.JRN.BR.PROMO`), the person/agent
//!   gate; [`promote_bus_error`] maps their refusals (catalogue v3).
//!
//! Agent branches (`agent/<client>/<n>`) are not created here: the routing module
//! (`CORE.BUS.ROUTE`) adds them through the journal's crate-private route planner.

use fid_bus::error::{
    BusError, CancelledReason, Detail, InternalReason, NotFoundReason, PolicyReason, ValidationReason,
};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_canon::CValue;
use fid_journal::branch::{
    command_defs, plan_coediting, plan_create, plan_protect, Actor, BranchError, BranchEvent, Branches, EntryOrigin,
    EventSource, CREATE_BRANCH, LIST_BRANCHES, MAIN, PROTECT_BRANCH, SET_BRANCH_COEDITING,
};
use fid_journal::diff::{BranchDiff, CancelFlag, DiffError, DiffOptions};
use fid_journal::verify::ReadError;
use fid_types::Digest;
use fid_types::SideEffect;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

mod promote;

pub use promote::{parse_params, promote_bus_error, register_promote, run_promote, PromoteHandler, PromoteWiring};

/// Where the handlers read the branch table. The bus wiring implements it over the journal.
///
/// Contract: [`BranchSource::branches`] is `Branches::replay` over the branch events of every
/// committed entry, in append order, each with its entry's origin (`EntryOrigin`: the entry's
/// principal kind, command, class, branch and adding stage, as the entry records them), and
/// the heads from the head cache. It covers every committed entry whatever later head moves
/// or undo did: a branch, once created or protected, stays.
///
/// Every replayed event must be one of its entry's committed events: the bus wiring must check
/// the payload's events against the verified header's `events_commit` (`CORE.JRN.ENTRY`)
/// before folding them, and must refuse (`internal/invariant_violated`) an entry whose payload
/// does not reproduce it. No wiring exists yet; this is a requirement on it. An entry whose payload was removed contributes no events, so after
/// payload removal the table keeps only what the wiring can prove (`CORE.JRN.BR.DIFF` AC-3 is
/// met for the diff's own reads, not yet for this table; follow-up with the bus wiring).
pub trait BranchSource: Send + Sync {
    /// The current branch table.
    fn branches(&self) -> Result<Branches, String>;
    /// Whether `head` is an entry hash of this project.
    fn is_head(&self, head: &Digest) -> bool;
}

/// The bus error of a planner refusal: same code, same reason (catalogue v3).
pub fn to_bus_error(e: &BranchError) -> BusError {
    let d = Detail::new(&e.detail());
    match e {
        BranchError::BadName(_) => BusError::Validation(ValidationReason::BadBranchName, d),
        BranchError::Schema(_) => BusError::Validation(ValidationReason::Schema, d),
        BranchError::UnknownBranch(_) => BusError::NotFound(NotFoundReason::UnknownBranch, d),
        BranchError::UnknownHead(_) => BusError::NotFound(NotFoundReason::UnknownEntity, d),
        BranchError::ClassForbidden(_) => BusError::Policy(PolicyReason::ClassForbidden, d),
        BranchError::HumanOnly(_) => BusError::Policy(PolicyReason::HumanOnly, d),
        BranchError::Tampered(_) => BusError::Internal(InternalReason::InvariantViolated, d),
    }
}

fn invariant(what: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(what))
}

fn schema(what: &str) -> BusError {
    BusError::Validation(ValidationReason::Schema, Detail::new(what))
}

/// The four commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Create,
    List,
    Protect,
    Coediting,
}

impl Op {
    pub const ALL: [Op; 4] = [Op::Create, Op::List, Op::Protect, Op::Coediting];

    pub fn command(self) -> &'static str {
        match self {
            Op::Create => CREATE_BRANCH,
            Op::List => LIST_BRANCHES,
            Op::Protect => PROTECT_BRANCH,
            Op::Coediting => SET_BRANCH_COEDITING,
        }
    }
}

fn field<'a>(params: &'a CValue, key: &str) -> Result<&'a CValue, BusError> {
    match params {
        CValue::Obj(o) => o.get(key).ok_or_else(|| schema(&format!("missing {key}"))),
        _ => Err(schema("parameters are not an object")),
    }
}

fn text<'a>(params: &'a CValue, key: &str) -> Result<&'a str, BusError> {
    match field(params, key)? {
        CValue::Str(s) => Ok(s),
        _ => Err(schema(&format!("{key} is not a string"))),
    }
}

fn event_of(e: &BranchEvent) -> Result<Event, BusError> {
    Ok(Event::new(EventType::new(e.type_id())?, e.body()))
}

fn one(b: &Branches, name: &str) -> Result<CValue, BusError> {
    let branch = b.get(name).ok_or_else(|| invariant("branch missing after its event"))?;
    Ok(CValue::Obj(BTreeMap::from([("branch".to_string(), branch.to_value())])))
}

/// Runs one command for the call `ctx` against `source`: the events to commit and the result
/// value. The caller is the principal the bus derived for the call; the result reflects the
/// table after the events.
pub fn run(
    op: Op,
    source: &dyn BranchSource,
    params: &CValue,
    ctx: &HandlerCtx,
) -> Result<(Vec<Event>, CValue), BusError> {
    let kind = ctx.principal().kind();
    let actor = Actor::of(kind);
    let mut b = source.branches().map_err(|e| invariant(&format!("branch table: {e}")))?;
    let is_head = |d: &Digest| source.is_head(d);
    let (event, name) = match op {
        Op::List => return Ok((Vec::new(), b.to_value())),
        Op::Create => {
            let name = text(params, "name")?;
            (Some(plan_create(&b, actor, name, text(params, "from")?, &is_head).map_err(|e| to_bus_error(&e))?), name)
        }
        Op::Protect => {
            let name = text(params, "branch")?;
            (plan_protect(&b, actor, name).map_err(|e| to_bus_error(&e))?, name)
        }
        Op::Coediting => {
            let name = text(params, "branch")?;
            let allow = match field(params, "allow")? {
                CValue::Bool(a) => *a,
                _ => return Err(schema("allow is not a boolean")),
            };
            let session = text(params, "agent_session")?;
            (plan_coediting(&b, actor, name, session, allow).map_err(|e| to_bus_error(&e))?, name)
        }
    };
    let mut events = Vec::new();
    if let Some(e) = event {
        // The origin this entry will record; the fold checks the event against it.
        let class = match op {
            Op::Protect => SideEffect::W3,
            Op::Create | Op::Coediting | Op::List => SideEffect::W2,
        };
        let origin = EntryOrigin {
            kind,
            command: op.command().to_string(),
            major: 1,
            class,
            branch: MAIN.to_string(),
            source: EventSource::Handler,
            human_assertion: None,
        };
        b.apply(&origin, &e).map_err(|e| to_bus_error(&e))?;
        events.push(event_of(&e)?);
    }
    Ok((events, one(&b, name)?))
}

/// The handler of one branch command.
pub struct BranchHandler {
    op: Op,
    source: Arc<dyn BranchSource>,
}

impl BranchHandler {
    pub fn new(op: Op, source: Arc<dyn BranchSource>) -> BranchHandler {
        BranchHandler { op, source }
    }
}

impl fmt::Debug for BranchHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BranchHandler").field("op", &self.op).finish_non_exhaustive()
    }
}

impl Handler for BranchHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        Ok(run(self.op, self.source.as_ref(), params.value(), ctx)?.0)
    }
}

/// Registers the four handlers, each with its definition from
/// `fid_journal::branch::command_defs`.
pub fn register(set: &mut HandlerSet, source: &Arc<dyn BranchSource>) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| invariant(&format!("branch definitions: {e}")))?;
    for (op, def) in Op::ALL.into_iter().zip(defs) {
        if def.id != op.command() {
            return Err(invariant("branch definitions out of order"));
        }
        set.register(def, Box::new(BranchHandler::new(op, Arc::clone(source))))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ journal.diff_branch@1

/// Where the diff handler reads the diff. The bus wiring implements it over the journal.
///
/// Contract: [`DiffSource::diff`] is `fid_journal::diff::diff_store` (or
/// `Journal::diff_branch`) over the project's journal stream, with `branches` and `opts`
/// passed through unchanged: computed from verified entry headers only. The `branches` it is
/// given come from a [`BranchSource`], whose replayed events must be checked against each
/// header's `events_commit` (see [`BranchSource`]).
pub trait DiffSource: Send + Sync {
    fn diff(
        &self,
        branches: &Branches,
        source: &str,
        target: &str,
        opts: &DiffOptions,
    ) -> Result<BranchDiff, DiffError>;
}

/// The bus error of a diff refusal: same code, same reason (catalogue v3).
pub fn diff_bus_error(e: &DiffError) -> BusError {
    let d = Detail::new(&e.detail());
    match e {
        DiffError::UnknownBranch(_) => BusError::NotFound(NotFoundReason::UnknownBranch, d),
        DiffError::Corrupt(_) | DiffError::Tampered(_) => BusError::Internal(InternalReason::InvariantViolated, d),
        DiffError::InputLimit(_) => BusError::Validation(ValidationReason::InputLimit, d),
        DiffError::Cancelled => BusError::Cancelled(CancelledReason::Requested, d),
        DiffError::Read(r) => match r {
            ReadError::UnknownStream(_) => BusError::NotFound(NotFoundReason::UnknownStream, d),
            ReadError::UnknownBranch(_) => BusError::NotFound(NotFoundReason::UnknownBranch, d),
            ReadError::BadRange(_) => BusError::Validation(ValidationReason::BadRange, d),
            ReadError::BadParam(_) => BusError::Validation(ValidationReason::Schema, d),
            ReadError::BadToken => BusError::Validation(ValidationReason::BadToken, d),
            ReadError::StaleToken => BusError::Validation(ValidationReason::StaleToken, d),
            ReadError::InputLimit(_) => BusError::Validation(ValidationReason::InputLimit, d),
            ReadError::Cancelled => BusError::Cancelled(CancelledReason::Requested, d),
            ReadError::Storage(_) => BusError::Internal(InternalReason::Storage, d),
            ReadError::Corrupt(_) => BusError::Internal(InternalReason::InvariantViolated, d),
        },
    }
}

/// Runs `journal.diff_branch@1` for the call `ctx`: the result value
/// (`branch-diff.schema.json`). The call's cancel token stops the walk.
pub fn run_diff(
    branches: &dyn BranchSource,
    journal: &dyn DiffSource,
    params: &CValue,
    ctx: &HandlerCtx,
) -> Result<CValue, BusError> {
    let source = text(params, "source")?;
    let target = text(params, "target")?;
    let table = branches.branches().map_err(|e| invariant(&format!("branch table: {e}")))?;
    let token = ctx.cancel().clone();
    let cancel: CancelFlag = Arc::new(move || token.is_cancelled());
    // Agents (MCP) get lower bounds; the kind is the principal the bus derived for the call.
    let opts = DiffOptions { cancel: Some(cancel), ..DiffOptions::for_caller(ctx.principal().kind()) };
    journal.diff(&table, source, target, &opts).map(|d| d.to_value()).map_err(|e| diff_bus_error(&e))
}

/// The handler of `journal.diff_branch@1` (R0: no events).
pub struct DiffHandler {
    branches: Arc<dyn BranchSource>,
    journal: Arc<dyn DiffSource>,
}

impl DiffHandler {
    pub fn new(branches: Arc<dyn BranchSource>, journal: Arc<dyn DiffSource>) -> DiffHandler {
        DiffHandler { branches, journal }
    }
}

impl fmt::Debug for DiffHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiffHandler").finish_non_exhaustive()
    }
}

impl Handler for DiffHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        run_diff(self.branches.as_ref(), self.journal.as_ref(), params.value(), ctx)?;
        Ok(Vec::new())
    }
}

/// Registers the handler of `journal.diff_branch@1` with its definition from
/// `fid_journal::diff::command_defs`.
pub fn register_diff(
    set: &mut HandlerSet,
    branches: &Arc<dyn BranchSource>,
    journal: &Arc<dyn DiffSource>,
) -> Result<(), BusError> {
    let defs = fid_journal::diff::command_defs().map_err(|e| invariant(&format!("diff definition: {e}")))?;
    for def in defs {
        if def.id != fid_journal::diff::DIFF_BRANCH {
            return Err(invariant("unexpected diff definition"));
        }
        set.register(def, Box::new(DiffHandler::new(Arc::clone(branches), Arc::clone(journal))))?;
    }
    Ok(())
}
