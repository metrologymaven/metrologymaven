//! `journal.undo@1`, `journal.redo@1` and `journal.show_undo_tree@1` on the bus
//! (spec node `CORE.JRN.UNDO.MOVE`, bus half; the undo tree, planners, barrier checks and
//! registry definitions live in `fid_journal::undo`, layer model).

use super::{invariant, schema};
use fid_bus::error::{BusError, Detail, InternalReason, NotFoundReason, PreconditionReason, ValidationReason};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_canon::CValue;
use fid_journal::undo::{command_defs, UndoError, UndoTree, REDO, SHOW_UNDO_TREE, UNDO};
use fid_types::Digest;
use std::fmt;
use std::sync::Arc;

/// Where the undo handlers read the projected undo tree and default branch.
pub trait UndoSource: Send + Sync {
    /// Current undo tree projected from the verified journal.
    fn tree(&self) -> Result<UndoTree, String>;
    /// Default branch for `ctx` when the caller omits `branch` (e.g. `"main"` or an agent branch).
    fn default_branch(&self, ctx: &HandlerCtx) -> Result<String, String>;
}

/// Maps every [`UndoError`] to the bus error of the same code and reason (catalogue v3).
pub fn undo_bus_error(e: &UndoError) -> BusError {
    let d = Detail::new(&e.detail());
    match e {
        UndoError::ReleasedRecord { .. } => BusError::Precondition(PreconditionReason::ReleasedRecord, d),
        UndoError::NotUndoable { .. } => BusError::Precondition(PreconditionReason::NotUndoable, d),
        UndoError::NothingToRedo(_) => BusError::Precondition(PreconditionReason::NothingToRedo, d),
        UndoError::NothingToUndo(_) => BusError::Precondition(PreconditionReason::False, d),
        UndoError::UnknownBranch(_) => BusError::NotFound(NotFoundReason::UnknownBranch, d),
        UndoError::UnknownState(_) => BusError::NotFound(NotFoundReason::UnknownEntity, d),
        UndoError::BadRange(_) => BusError::Validation(ValidationReason::BadRange, d),
        UndoError::Schema(_) => BusError::Validation(ValidationReason::Schema, d),
        UndoError::Storage(_) => BusError::Internal(InternalReason::Storage, d),
        UndoError::Tampered(_) | UndoError::Corrupt(_) => BusError::Internal(InternalReason::InvariantViolated, d),
    }
}

/// Which undo command is being executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoOp {
    Undo,
    Redo,
    ShowUndoTree,
}

impl UndoOp {
    pub const ALL: [UndoOp; 3] = [UndoOp::Undo, UndoOp::Redo, UndoOp::ShowUndoTree];

    pub const fn command(self) -> &'static str {
        match self {
            UndoOp::Undo => UNDO,
            UndoOp::Redo => REDO,
            UndoOp::ShowUndoTree => SHOW_UNDO_TREE,
        }
    }
}

fn opt_str<'a>(params: &'a CValue, key: &str) -> Result<Option<&'a str>, BusError> {
    let CValue::Obj(o) = params else { return Err(schema("parameters are not an object")) };
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        _ => Err(schema(&format!("{key} is not a string"))),
    }
}

fn opt_u32(params: &CValue, key: &str) -> Result<Option<u32>, BusError> {
    let CValue::Obj(o) = params else { return Err(schema("parameters are not an object")) };
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Num(n)) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX) => {
            Ok(Some(*n as u32))
        }
        Some(CValue::Num(_)) => {
            Err(BusError::Validation(ValidationReason::BadRange, Detail::new(&format!("{key} is out of range"))))
        }
        _ => Err(schema(&format!("{key} is not an integer"))),
    }
}

fn opt_digest(params: &CValue, key: &str) -> Result<Option<Digest>, BusError> {
    match opt_str(params, key)? {
        None => Ok(None),
        Some(s) => Digest::parse(s).map(Some).map_err(|_| schema(&format!("{key} is not a sha256: digest"))),
    }
}

fn check_allowed_keys(params: &CValue, allowed: &[&str]) -> Result<(), BusError> {
    let CValue::Obj(o) = params else { return Err(schema("parameters are not an object")) };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(schema(&format!("unknown parameter {:?}", k.chars().take(64).collect::<String>())));
        }
    }
    Ok(())
}

/// Runs one undo/redo/show-undo-tree call against `source`, returning the events to commit
/// (one `HeadMoved` event for undo/redo, empty for `show_undo_tree`) and the command result value.
pub fn run_undo(
    op: UndoOp,
    source: &dyn UndoSource,
    params: &CValue,
    ctx: &HandlerCtx,
) -> Result<(Vec<Event>, CValue), BusError> {
    match op {
        UndoOp::Undo => check_allowed_keys(params, &["branch", "steps"])?,
        UndoOp::Redo => check_allowed_keys(params, &["branch", "child", "steps"])?,
        UndoOp::ShowUndoTree => check_allowed_keys(params, &["branch", "depth", "from"])?,
    }
    let tree = source.tree().map_err(|e| invariant(&format!("undo tree: {e}")))?;
    let branch = match opt_str(params, "branch")? {
        Some(b) => b.to_string(),
        None => source.default_branch(ctx).map_err(|e| invariant(&format!("default branch: {e}")))?,
    };

    match op {
        UndoOp::Undo => {
            let steps = opt_u32(params, "steps")?.unwrap_or(1);
            let moved = tree.plan_undo(&branch, steps).map_err(|e| undo_bus_error(&e))?;
            let ev = Event::new(EventType::new(moved.type_id())?, moved.body());
            Ok((vec![ev], moved.result_value()))
        }
        UndoOp::Redo => {
            let child = opt_digest(params, "child")?;
            let steps = opt_u32(params, "steps")?.unwrap_or(1);
            let moved = tree.plan_redo(&branch, child, steps).map_err(|e| undo_bus_error(&e))?;
            let ev = Event::new(EventType::new(moved.type_id())?, moved.body());
            Ok((vec![ev], moved.result_value()))
        }
        UndoOp::ShowUndoTree => {
            let from = opt_digest(params, "from")?;
            let depth = opt_u32(params, "depth")?;
            let view = tree.show_tree(&branch, from, depth).map_err(|e| undo_bus_error(&e))?;
            Ok((Vec::new(), view.to_value()))
        }
    }
}

/// Bus handler for `journal.undo@1`, `journal.redo@1` or `journal.show_undo_tree@1`.
pub struct UndoHandler {
    op: UndoOp,
    source: Arc<dyn UndoSource>,
}

impl UndoHandler {
    pub fn new(op: UndoOp, source: Arc<dyn UndoSource>) -> Self {
        Self { op, source }
    }
}

impl fmt::Debug for UndoHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UndoHandler").field("op", &self.op).finish_non_exhaustive()
    }
}

impl Handler for UndoHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        Ok(run_undo(self.op, self.source.as_ref(), params.value(), ctx)?.0)
    }
}

/// Registers `journal.undo@1`, `journal.redo@1` and `journal.show_undo_tree@1` in `set`.
pub fn register_undo(set: &mut HandlerSet, source: &Arc<dyn UndoSource>) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| invariant(&format!("undo definitions: {e}")))?;
    if defs.len() != UndoOp::ALL.len() {
        return Err(invariant("unexpected number of undo definitions"));
    }
    for (def, op) in defs.into_iter().zip(UndoOp::ALL) {
        if def.id != op.command() {
            return Err(invariant("undo definitions out of order"));
        }
        set.register(def, Box::new(UndoHandler::new(op, Arc::clone(source))))?;
    }
    Ok(())
}
