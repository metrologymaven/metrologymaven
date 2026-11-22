//! Branch routing and per-branch writer locks. Spec node `CORE.BUS.ROUTE`, part 1.
//!
//! **Built here (part 1):**
//!
//! - [`SessionRouter`]: the [`Router`] of the pipeline's route stage. A non-agent principal
//!   (`human`, `script`, `service`, `device`, `system`) commits to the branch its session has
//!   open (REQ-003): the project's working branch until the session opens another
//!   ([`SessionRouter::open_branch`]). An `agent` principal's commit-class call is refused with
//!   `policy/agent_write_forbidden` (fail closed until part 2 routes it to its own
//!   `agent/<client>/<n>` branch); the pipeline's own `agent/` check stays as a backstop.
//! - [`check_expected_head`]: the `expected_head` guard (REQ-004). A mismatch is a
//!   [`HeadConflict`] that carries the branch's current head; as a [`BusError`] it is
//!   `conflict/expected_head_mismatch` whose detail names the current head.
//! - [`BranchLocks`]: one FIFO ticket lock per branch (REQ-005). The pipeline takes the
//!   branch's ticket at the start of its route stage, once the branch is chosen, and holds it
//!   through the guards, the handler, the effects check and the commit, so a branch has one
//!   writer at a time and waiting writers are served in the order they took their tickets.
//!
//! **Lock order.** A call of [`crate::pipeline::Bus::execute`] takes, in this order and only
//! in this order:
//!
//! 1. at most one branch ticket ([`BranchLocks::acquire`]), at the route stage;
//! 2. the bus's commit lock, inside the commit stage only, released before the call returns;
//! 3. locks inside the hooks and the journal, under the commit lock.
//!
//! The internal mutexes of [`BranchLocks`] and [`SessionRouter`] are leaf locks: each is held
//! for a few map operations and never while waiting for, or taking, any other lock (the
//! ticket wait releases its mutex). A ticket is only ever waited for by a call that holds no
//! other bus lock (the route stage runs before the commit stage, and no ticket is taken
//! under the commit lock), and the holder of a ticket never waits for a second one. So no
//! cycle of waits can form between tickets and the commit lock. The commit lock stays
//! bus-wide because the commit hooks are bus-wide: their staged states are installed in
//! commit order across all branches. A thread must not take a second ticket of a branch
//! whose ticket it holds (it would wait for itself); the pipeline takes one per call.
//!
//! **Not built (part 2, see NODE.md):** agent branches `agent/<client>/<n>` and the event
//! source that creates them, the co-editing opt-in and `agent_coedited`, the idempotency
//! table, and the agent name bound by `AUTO.MCP.SESS`. [`RouteTarget::coedited`] is always
//! `false` here.
//!
//! Determinism D0: routing reads only the session's recorded open branch and the class.

use crate::error::{BusError, ConflictReason, Detail, InternalReason, PolicyReason};
use crate::pipeline::{is_read, Router};
use crate::principal::{Channel, Principal, PrincipalKind, SessionKey};
use fid_canon::Digest;
use fid_journal::branch::BranchName;
use fid_registry::CommandDef;
use fid_types::SideEffect;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

fn invariant(detail: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(detail))
}

// ------------------------------------------------------------------ target

/// Where a call commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteTarget {
    /// The branch.
    pub branch: BranchName,
    /// Whether an agent commits to a non-agent branch under a recorded human opt-in
    /// (REQ-002). Always `false` in part 1, which routes no agent commit.
    pub coedited: bool,
}

/// The session router: each session's open branch, defaulting to the project's working
/// branch. Sessions are named by their bus-issued key (table and number), never by anything
/// the client sends.
#[derive(Debug)]
pub struct SessionRouter {
    working: BranchName,
    open: Mutex<OpenBranches>,
}

/// Each session's open branch, by (session table, session number); absent means the working
/// branch.
type OpenBranches = BTreeMap<(u64, u64), BranchName>;

fn session_id(session: &SessionKey) -> (u64, u64) {
    (session.table_id(), session.number())
}

impl SessionRouter {
    /// A router whose sessions all have `working` open until they open another branch.
    pub fn new(working: BranchName) -> SessionRouter {
        SessionRouter { working, open: Mutex::new(BTreeMap::new()) }
    }

    fn map(&self) -> Result<MutexGuard<'_, OpenBranches>, BusError> {
        self.open.lock().map_err(|_| invariant("the session router's lock is poisoned"))
    }

    /// The project's working branch.
    pub fn working(&self) -> &BranchName {
        &self.working
    }

    /// Records that `session` now has `branch` open; its later commits go there.
    pub fn open_branch(&self, session: &SessionKey, branch: BranchName) -> Result<(), BusError> {
        let mut map = self.map()?;
        if branch == self.working {
            map.remove(&session_id(session));
        } else {
            map.insert(session_id(session), branch);
        }
        Ok(())
    }

    /// Forgets the branch `session` had open (call when the session ends); a later call of
    /// the same key sees the working branch again.
    pub fn close_session(&self, session: &SessionKey) -> Result<(), BusError> {
        self.map()?.remove(&session_id(session));
        Ok(())
    }

    /// The branch `session` has open.
    pub fn open_branch_of(&self, session: &SessionKey) -> Result<BranchName, BusError> {
        Ok(self.map()?.get(&session_id(session)).cloned().unwrap_or_else(|| self.working.clone()))
    }

    /// `route::target` (REQ-003): where a call of class `class` by `principal` in `session`
    /// commits (for a read: the branch its call record names). A non-agent principal's call
    /// goes to the session's open branch. An agent's commit-class call is
    /// `policy/agent_write_forbidden` until part 2 (agent branches); an agent's read names
    /// the open branch.
    pub fn target(
        &self,
        principal: &Principal,
        session: &SessionKey,
        class: SideEffect,
    ) -> Result<RouteTarget, BusError> {
        if principal.kind() == PrincipalKind::Agent && !is_read(class) {
            return Err(BusError::Policy(
                PolicyReason::AgentWriteForbidden,
                Detail::new(
                    "agent commits go to the agent's own agent/<client>/<n> branch, which CORE.BUS.ROUTE part 2 \
                     builds; refused until then",
                ),
            ));
        }
        Ok(RouteTarget { branch: self.open_branch_of(session)?, coedited: false })
    }
}

impl Router for SessionRouter {
    fn branch(&self, principal: &Principal, channel: &dyn Channel, def: &CommandDef) -> Result<String, BusError> {
        Ok(self.target(principal, channel.session(), def.side_effect)?.branch.as_str().to_string())
    }

    /// The session's open branch, for every principal. An agent's request there is refused by
    /// the pipeline's `agent/` check unless the open branch is an agent branch.
    fn request_branch(&self, _principal: &Principal, channel: &dyn Channel) -> Result<String, BusError> {
        Ok(self.open_branch_of(channel.session())?.as_str().to_string())
    }
}

// ------------------------------------------------------------------ expected_head

/// An `expected_head` that is not the branch head (REQ-004), with the current head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadConflict {
    pub branch: String,
    /// The head the caller expected.
    pub expected: Digest,
    /// The branch's current head (`GENESIS_PREV` for a branch with no entry).
    pub current: Digest,
}

impl fmt::Display for HeadConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "branch {}: expected head {}, current head {}", self.branch, self.expected, self.current)
    }
}

impl From<HeadConflict> for BusError {
    /// `conflict/expected_head_mismatch`; the detail is the [`fmt::Display`] text, which names
    /// the current head.
    fn from(c: HeadConflict) -> BusError {
        BusError::Conflict(ConflictReason::ExpectedHeadMismatch, Detail::new(&c.to_string()))
    }
}

/// The `expected_head` guard: `Ok` when there is no expectation or it is the current head.
pub fn check_expected_head(branch: &str, expected: Option<Digest>, current: Digest) -> Result<(), HeadConflict> {
    match expected {
        Some(expected) if expected != current => Err(HeadConflict { branch: branch.to_string(), expected, current }),
        _ => Ok(()),
    }
}

// ------------------------------------------------------------------ branch locks

/// One branch's ticket counters: `next` is the next ticket to hand out, `serving` the ticket
/// that holds the branch. `next - serving` callers hold or wait for the branch.
#[derive(Debug, Default)]
struct Queue {
    next: u64,
    serving: u64,
}

#[derive(Debug, Default)]
struct LocksInner {
    queues: Mutex<BTreeMap<String, Queue>>,
    turn: Condvar,
}

impl LocksInner {
    /// The queue map. Poisoning is recovered from on purpose: the map holds only counters,
    /// every critical section leaves them consistent before anything that could panic, and
    /// refusing here would strand callers that already hold tickets (their release would be
    /// lost, and every later writer of the branch would wait forever).
    fn queues(&self) -> MutexGuard<'_, BTreeMap<String, Queue>> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Per-branch FIFO writer locks (REQ-005; module docs for the lock order). Cloning shares
/// the same locks.
#[derive(Debug, Clone, Default)]
pub struct BranchLocks {
    inner: Arc<LocksInner>,
}

/// The right to write one branch, held from the route stage through the commit. Dropping it
/// releases the branch to the next ticket in arrival order (also when the holder unwinds).
#[derive(Debug)]
pub struct BranchTicket {
    inner: Arc<LocksInner>,
    branch: String,
}

impl BranchTicket {
    /// The branch this ticket holds.
    pub fn branch(&self) -> &str {
        &self.branch
    }
}

impl BranchLocks {
    /// No branch held.
    pub fn new() -> BranchLocks {
        BranchLocks::default()
    }

    /// Takes the next ticket of `branch` and waits until it is served. Callers are served in
    /// the order they took their tickets. Must not be called while holding a ticket of the
    /// same branch, nor while holding the bus's commit lock (module docs).
    pub fn acquire(&self, branch: &BranchName) -> Result<BranchTicket, BusError> {
        let name = branch.as_str();
        let mut queues = self.inner.queues();
        let queue = queues.entry(name.to_string()).or_default();
        let ticket = queue.next;
        queue.next = ticket.checked_add(1).ok_or_else(|| invariant("branch ticket counter overflow"))?;
        // From here the ticket is taken: every exit must serve it, which the ticket's drop does.
        let held = BranchTicket { inner: Arc::clone(&self.inner), branch: name.to_string() };
        loop {
            let served = queues.get(name).map(|q| q.serving == ticket);
            match served {
                Some(true) => break,
                Some(false) => {}
                None => {
                    // Release the map before `held` is dropped: its drop locks the map.
                    drop(queues);
                    return Err(invariant("a branch queue vanished while a ticket was waiting"));
                }
            }
            queues = self.inner.turn.wait(queues).unwrap_or_else(PoisonError::into_inner);
        }
        drop(queues);
        Ok(held)
    }

    /// How many callers hold or wait for `branch` (0 when it is free).
    pub fn queued(&self, branch: &str) -> u64 {
        self.inner.queues().get(branch).map_or(0, |q| q.next.saturating_sub(q.serving))
    }
}

impl Drop for BranchTicket {
    fn drop(&mut self) {
        let mut queues = self.inner.queues();
        let empty = match queues.get_mut(&self.branch) {
            Some(q) => {
                q.serving = q.serving.saturating_add(1);
                q.serving >= q.next
            }
            None => false,
        };
        if empty {
            queues.remove(&self.branch);
        }
        drop(queues);
        self.inner.turn.notify_all();
    }
}
