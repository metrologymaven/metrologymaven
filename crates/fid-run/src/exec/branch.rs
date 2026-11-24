//! The run's own branch and Run entity (spec node `AUTO.RUN.EXEC.BRANCH`).
//!
//! **Opening a run** ([`open_run_target`]), first failure returned, nothing later attempted:
//!
//! 1. **Plan.** The calling principal (kind and identity) must be the record's principal, and
//!    the record must be fresh: outcome `pending`, no branch named yet. A run is set up only
//!    under the principal it was prepared for.
//! 2. **Fork point** (REQ-001). The principal's current working branch comes from the bus
//!    ([`RunBus::working_branch`]: the session's open branch for a person, the agent's own
//!    branch for an agent), and its head is the fork point. A working branch with no head is
//!    refused.
//! 3. **Branch.** `journal.create_branch@1` `{"from": <fork point>, "name": "run/<run_id>"}`
//!    ([`crate::run_branch`]) is committed on the working branch under the principal, with
//!    `expected_head` set to the fork point, so the branch starts at exactly the head that was
//!    read: if the working branch moved meanwhile, the bus refuses the call and no branch
//!    exists. The new branch must then have the fork point as its head (nothing landed on it).
//! 4. **Run entity** (REQ-002). `project.create_run@1` `{"record": <run record>}`, the
//!    plan's record with its branch named ([`RunRecord::with_branch`]), is committed on the run
//!    branch under the principal with `expected_head` set to the fork point, so it is the
//!    branch's first entry or nothing.
//!
//! Any failure in 1 to 4 is `run_setup_failed` naming the stage and the cause
//! ([`ExecError::RunSetupFailed`]). The steps run in this order, so a Run entity never exists
//! without its branch (REQ-004). A failure in step 4 leaves the empty branch (branches are
//! append-only, never deleted), and the error names it.
//!
//! **Routing** (REQ-003). The returned [`RunTarget`] commits only to `run/<run_id>`, always
//! under the run's principal ([`RunTarget::commit`]); a commit addressed to any other branch is
//! refused with `wrong_branch` before the bus is called.
//!
//! **Finalising** (REQ-005, [`finalise`]). The Run entity is read back from the run branch; if
//! its outcome is no longer `pending` the call is `already_final` and nothing is written.
//! Otherwise `project.finalise_run@1` `{"outcome": <outcome>, "run": "<run_id>"}` is committed
//! on the run branch with `expected_head` set to the head that was read, so two racing
//! finalisations cannot both pass the check: the second is refused by the bus.
//!
//! **The bus** is reached through [`RunBus`], which the host wires to the command bus
//! (`fid_bus::Bus` with a router that commits each call on the branch named here;
//! `AUTO.RUN.EXEC.CMD`). Nothing here writes to a journal directly.
//!
//! **Not settled by this leaf** (NODE.md, Open questions): the parameter documents of
//! `project.create_run@1` and `project.finalise_run@1` are this module's; CORE.PRJ.CONT.HIER's
//! built `project.create_run@1` takes `{id, label, plan, revision}` and no command yet sets a
//! Run entity's outcome. Against today's bus an agent cannot create a branch with
//! `journal.create_branch@1` nor commit outside `agent/`, so an agent's run setup fails closed
//! with `run_setup_failed`.
//!
//! Determinism D0 apart from the run ID and the branch name derived from it. No floating
//! point, no clock, no randomness.

use crate::prepare::RunPlan;
use crate::record::{run_branch, Outcome, RunRecord};
use fid_bus::error::{BusError, Detail, InternalReason, NotFoundReason};
use fid_bus::handler::EntityId;
use fid_bus::principal::{Principal, PrincipalRef};
use fid_bus::{CommandResult, Invocation};
use fid_canon::{CValue, Digest as EntryHash};
use fid_journal::branch::BranchName;
use fid_program::Uuid7;
use fid_types::CommandId;
use std::collections::BTreeMap;
use std::fmt;

/// The command that creates the run branch.
pub const CREATE_BRANCH_COMMAND: &str = "journal.create_branch@1";
/// The command that writes the Run entity as the run branch's first entry.
pub const CREATE_RUN_COMMAND: &str = "project.create_run@1";
/// The command that writes the Run entity's final outcome.
pub const FINALISE_RUN_COMMAND: &str = "project.finalise_run@1";

/// The command bus as a run's setup and finalisation use it (module docs).
pub trait RunBus {
    /// The branch `principal` currently works on (its session's open branch; for an agent,
    /// its own branch).
    fn working_branch(&self, principal: &Principal) -> Result<String, BusError>;
    /// The head of `branch`; for a branch with no entry of its own, the head it was created
    /// at. `None` if there is no such branch or it has no head.
    fn branch_head(&self, branch: &str) -> Result<Option<EntryHash>, BusError>;
    /// The Run entity of run `run_id` as committed on `branch`, if any.
    fn run_record(&self, branch: &str, run_id: Uuid7) -> Result<Option<RunRecord>, BusError>;
    /// Runs `inv` through the bus under `principal`, committed on `branch`.
    fn execute(&self, principal: &Principal, branch: &str, inv: &Invocation) -> Result<CommandResult, BusError>;
}

/// The step of [`open_run_target`] that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SetupStage {
    /// The plan's principal or state.
    Plan,
    /// Reading the working branch and its head.
    WorkingBranch,
    /// `journal.create_branch@1`.
    CreateBranch,
    /// `project.create_run@1`.
    RunEntity,
}

impl SetupStage {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            SetupStage::Plan => "plan",
            SetupStage::WorkingBranch => "working_branch",
            SetupStage::CreateBranch => "create_branch",
            SetupStage::RunEntity => "run_entity",
        }
    }
}

/// Why a stage failed.
#[derive(Debug, Clone, PartialEq)]
pub enum SetupCause {
    /// The bus refused or failed the call.
    Bus(BusError),
    /// A check of this module refused it (static reason).
    Refused(&'static str),
}

impl fmt::Display for SetupCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupCause::Bus(e) => write!(f, "{e}"),
            SetupCause::Refused(r) => f.write_str(r),
        }
    }
}

/// The run's final outcome: `pending` is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FinalOutcome {
    /// Finished without error.
    Succeeded,
    /// Stopped by an error.
    Failed,
    /// Cancelled.
    Cancelled,
}

impl FinalOutcome {
    /// The record's outcome.
    pub fn outcome(self) -> Outcome {
        match self {
            FinalOutcome::Succeeded => Outcome::Succeeded,
            FinalOutcome::Failed => Outcome::Failed,
            FinalOutcome::Cancelled => Outcome::Cancelled,
        }
    }
}

/// Errors of this module. [`ExecError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecError {
    /// `run_setup_failed`: the run branch or the Run entity could not be set up (REQ-004).
    RunSetupFailed {
        /// The failed step.
        stage: SetupStage,
        /// The run branch, if it was created (it is left empty).
        branch: Option<String>,
        /// Why.
        cause: SetupCause,
    },
    /// `wrong_branch`: a commit of the run addressed to another branch (REQ-003).
    WrongBranch {
        /// The run branch.
        expected: String,
        /// The branch the commit was addressed to.
        got: String,
    },
    /// `already_final`: the run's outcome is written already (REQ-005).
    AlreadyFinal {
        /// The outcome on record.
        outcome: Outcome,
    },
    /// A bus error outside setup (a routed commit or a finalisation), passed through.
    Bus(BusError),
}

impl ExecError {
    /// The stable code; a passed-through bus error keeps its own code.
    pub fn code(&self) -> &'static str {
        match self {
            ExecError::RunSetupFailed { .. } => "run_setup_failed",
            ExecError::WrongBranch { .. } => "wrong_branch",
            ExecError::AlreadyFinal { .. } => "already_final",
            ExecError::Bus(e) => e.code().as_str(),
        }
    }
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecError::RunSetupFailed { stage, branch: Some(b), cause } => {
                write!(f, "run_setup_failed at {}: {cause} (branch {b} was created and is left empty)", stage.as_str())
            }
            ExecError::RunSetupFailed { stage, branch: None, cause } => {
                write!(f, "run_setup_failed at {}: {cause}", stage.as_str())
            }
            ExecError::WrongBranch { expected, got } => {
                write!(f, "wrong_branch: the run commits only to {expected}, not {got}")
            }
            ExecError::AlreadyFinal { outcome } => write!(f, "already_final: the run is {}", outcome.as_str()),
            ExecError::Bus(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExecError {}

/// Where a run's commits go (module docs). Built only by [`open_run_target`].
#[derive(Debug, Clone, PartialEq)]
pub struct RunTarget {
    run_id: Uuid7,
    branch: BranchName,
    fork_point: EntryHash,
    run_entity: EntityId,
    run_entry: EntryHash,
    principal: Principal,
}

impl RunTarget {
    /// The run ID.
    pub fn run_id(&self) -> Uuid7 {
        self.run_id
    }
    /// The run branch `run/<run_id>`.
    pub fn branch(&self) -> &str {
        self.branch.as_str()
    }
    /// The working-branch head the run branch was forked from.
    pub fn fork_point(&self) -> EntryHash {
        self.fork_point
    }
    /// The Run entity's ID (the run ID's canonical text).
    pub fn run_entity(&self) -> &EntityId {
        &self.run_entity
    }
    /// The entry that wrote the Run entity (the run branch's first entry).
    pub fn run_entry(&self) -> EntryHash {
        self.run_entry
    }
    /// The principal every commit of the run is made under.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// `Ok` exactly when `branch` is the run branch; else `wrong_branch` (REQ-003).
    pub fn route(&self, branch: &str) -> Result<(), ExecError> {
        if branch == self.branch.as_str() {
            Ok(())
        } else {
            Err(ExecError::WrongBranch { expected: self.branch.as_str().to_owned(), got: branch.to_owned() })
        }
    }

    /// Commits `inv` of the run, addressed to `branch`, under the run's principal: refused
    /// with `wrong_branch` before the bus is called unless `branch` is the run branch.
    pub fn commit(&self, bus: &dyn RunBus, branch: &str, inv: &Invocation) -> Result<CommandResult, ExecError> {
        self.route(branch)?;
        bus.execute(&self.principal, self.branch.as_str(), inv).map_err(ExecError::Bus)
    }
}

fn command(id: &str) -> Result<CommandId, &'static str> {
    CommandId::parse(id).map_err(|_| "command id")
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<BTreeMap<_, _>>())
}

/// The entry hash of a committed call; anything else (a filed request, a dry run, a read)
/// is not a commit.
fn committed(result: CommandResult) -> Result<EntryHash, &'static str> {
    match result {
        CommandResult::Done { entry: Some(hash), .. } => Ok(hash),
        CommandResult::Done { entry: None, .. } => Err("the call committed no entry"),
        CommandResult::Pending { .. } | CommandResult::WouldFile { .. } => {
            Err("the call filed a pending request instead of committing")
        }
        CommandResult::DryRun { .. } => Err("the call was a dry run"),
    }
}

fn setup(stage: SetupStage, branch: Option<&BranchName>, cause: SetupCause) -> ExecError {
    ExecError::RunSetupFailed { stage, branch: branch.map(|b| b.as_str().to_owned()), cause }
}

fn refused(stage: SetupStage, branch: Option<&BranchName>, why: &'static str) -> ExecError {
    setup(stage, branch, SetupCause::Refused(why))
}

/// Opens the run of `plan` for `principal` (module docs): creates `run/<run_id>` from the head
/// of the principal's working branch, writes the Run entity as its first entry, and returns
/// the run's target.
pub fn open_run_target(bus: &dyn RunBus, plan: &RunPlan, principal: &Principal) -> Result<RunTarget, ExecError> {
    // 1. Plan.
    let record = &plan.record;
    let caller: PrincipalRef = principal.reference();
    if *record.principal() != caller {
        return Err(refused(SetupStage::Plan, None, "the calling principal is not the run's principal"));
    }
    if record.branch().is_some() || record.outcome() != Outcome::Pending {
        return Err(refused(SetupStage::Plan, None, "the plan's record is not fresh (branch named or not pending)"));
    }
    let run_id = record.run_id();
    let name = BranchName::parse(&run_branch(run_id))
        .map_err(|_| refused(SetupStage::Plan, None, "the run branch name is not a branch name"))?;
    let run_entity =
        EntityId::new(&run_id.to_string()).map_err(|e| setup(SetupStage::Plan, None, SetupCause::Bus(e)))?;

    // 2. Fork point.
    let stage = SetupStage::WorkingBranch;
    let working = bus.working_branch(principal).map_err(|e| setup(stage, None, SetupCause::Bus(e)))?;
    let fork_point = bus
        .branch_head(&working)
        .map_err(|e| setup(stage, None, SetupCause::Bus(e)))?
        .ok_or_else(|| refused(stage, None, "the working branch has no head to fork from"))?;

    // 3. Branch, forked at exactly the head read.
    let stage = SetupStage::CreateBranch;
    let create = command(CREATE_BRANCH_COMMAND).map_err(|w| refused(stage, None, w))?;
    let params = obj([("from", CValue::Str(fork_point.to_string())), ("name", CValue::Str(name.as_str().to_owned()))]);
    let mut inv = Invocation::new(create, params);
    inv.expected_head = Some(fork_point);
    let result = bus.execute(principal, &working, &inv).map_err(|e| setup(stage, None, SetupCause::Bus(e)))?;
    committed(result).map_err(|w| refused(stage, None, w))?;
    let head = bus.branch_head(name.as_str()).map_err(|e| setup(stage, Some(&name), SetupCause::Bus(e)))?;
    if head != Some(fork_point) {
        return Err(refused(stage, Some(&name), "the run branch does not start at the fork point"));
    }

    // 4. Run entity, the branch's first entry.
    let stage = SetupStage::RunEntity;
    let entity = record.clone().with_branch(name.as_str());
    let create_run = command(CREATE_RUN_COMMAND).map_err(|w| refused(stage, Some(&name), w))?;
    let mut inv = Invocation::new(create_run, obj([("record", entity.to_value())]));
    inv.expected_head = Some(fork_point);
    let result =
        bus.execute(principal, name.as_str(), &inv).map_err(|e| setup(stage, Some(&name), SetupCause::Bus(e)))?;
    let run_entry = committed(result).map_err(|w| refused(stage, Some(&name), w))?;

    Ok(RunTarget { run_id, branch: name, fork_point, run_entity, run_entry, principal: principal.clone() })
}

/// Writes the run's final `outcome` to its Run entity on the run branch, once (module docs):
/// `already_final` if the entity's outcome is not `pending`. Returns the entry hash.
pub fn finalise(bus: &dyn RunBus, target: &RunTarget, outcome: FinalOutcome) -> Result<EntryHash, ExecError> {
    let branch = target.branch();
    let record = bus.run_record(branch, target.run_id).map_err(ExecError::Bus)?.ok_or_else(|| {
        ExecError::Bus(BusError::NotFound(
            NotFoundReason::UnknownEntity,
            Detail::new(&format!("no Run entity {} on {branch}", target.run_id)),
        ))
    })?;
    if record.outcome() != Outcome::Pending {
        return Err(ExecError::AlreadyFinal { outcome: record.outcome() });
    }
    let head = bus.branch_head(branch).map_err(ExecError::Bus)?;
    let id = command(FINALISE_RUN_COMMAND)
        .map_err(|w| ExecError::Bus(BusError::NotFound(NotFoundReason::UnknownCommand, Detail::new(w))))?;
    let params = obj([
        ("outcome", CValue::Str(outcome.outcome().as_str().to_owned())),
        ("run", CValue::Str(target.run_id.to_string())),
    ]);
    let mut inv = Invocation::new(id, params);
    inv.expected_head = head;
    let result = target.commit(bus, branch, &inv)?;
    committed(result).map_err(|w| ExecError::Bus(BusError::Internal(InternalReason::InvariantViolated, Detail::new(w))))
}
