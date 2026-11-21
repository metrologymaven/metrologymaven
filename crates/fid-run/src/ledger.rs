//! The run ledger: run records folded from committed `RunStarted` events, bound to their
//! entries' verified origin (REQ-CORE.RPL.RECON.FOLD-008, "Authority-bearing folds").
//!
//! A run record carries authority: it says that a released program ran, or that a person
//! approved a trial or overrode a deprecation. So the ledger never trusts the event body for
//! who started the run, under which command, on which branch or against which lifecycle state.
//!
//! **The event.** `AUTO.RUN.EXEC` (`program.run@1`) commits one `RunStarted` event
//! `{"record": <run record>}` ([`run_started`]) in the entry that starts the run, on the run's
//! own branch `run/<run_id>` ([`crate::run_branch`]) after forking it, with the record of its
//! [`crate::RunPlan`] on that branch ([`crate::RunRecord::with_branch`]). The entry's parameters
//! are [`crate::run_params`] of the record. `RunStarted` is reserved to `program.run@1` in
//! `fid_registry::RESERVED_EFFECTS` (b), so no other command can declare it; `AUTO.RUN.BATCH`
//! starts each of its runs through `program.run@1` (maintainer decision, single owner).
//!
//! **Fold domain.** The ledger folds the entries of **all branches in commit order**. The
//! lifecycle store passed to [`RunLedger::apply_entry`] and [`RunLedger::precommit`] is the
//! project's lifecycle state (that of `main`, where people register, release and deprecate) as
//! of that entry: the fold advances it over lifecycle entries in the same commit order, and the
//! commit stage reads it at its commit. So the fold and the commit stage judge a run against the
//! same state and cannot disagree.
//!
//! **The fold** ([`RunLedger::apply_entry`]) takes each entry's own record ([`RunEntry`]: the
//! bus `EntryOrigin` with principal, command and parameters commitment; the journal's
//! `EntryOrigin` with kind, command, class, branch and adding stage; the header's
//! `promoted_from`; the entry hash). An entry without a `RunStarted` event is ignored. For one
//! with it, in this order:
//!
//! 1. exactly one `RunStarted` event and no `Request*` event (c): a run is never started by
//!    an approved agent request, so an agent cannot borrow a person's trial or override;
//! 2. a promoted copy (`promoted_from` present) is **skipped** (e): the run was admitted from
//!    its original entry on `run/<run_id>`; promotion copies that entry with its events to
//!    another branch, so the copy names a run already in the ledger (or one refused there) and
//!    adds no run and no authority. Skipping it keeps the promotion itself valid;
//! 3. the entry's command is `program.run@1` in both records, class W2, the events added by
//!    the handler (a, b);
//! 4. the body has exactly its one member and the record reads strictly
//!    ([`crate::RunRecord::from_value`]);
//! 5. the record's principal (kind and identity) is the entry's principal in both records (a);
//! 6. the parameters commitment of [`crate::run_params`] of the record is the entry's (a): the
//!    program, binding, inputs, variant and both flags are the ones the principal asked for;
//! 7. the record's branch and the entry's branch are both exactly `run/<run_id>` (a, e);
//! 8. the outcome is `pending`, and the principal rules of `prepare_run` hold for the
//!    recorded principal kind, lifecycle state and flags (REQ-AUTO.RUN.BIND-001/002), the
//!    override included: only a person's, only on a deprecated program;
//! 9. **decided-against state** (d): the program's (version, state) in the lifecycle store is
//!    the record's, and the run ID is not already in the ledger (a replayed entry).
//!
//! Every refusal but (2) is **fatal** (f): `internal/invariant_violated` naming the entry, the
//! ledger unchanged. The commit stage runs the same rules first ([`RunLedger::precommit`])
//! atomically with the commit, as a compare-and-swap on the lifecycle state and the ledger,
//! and a failure of (9) there is `conflict/head_moved` (the program changed state, or the run
//! is already recorded, since the plan was prepared). A committed entry the fold refuses is
//! then a commit-stage bug or a forged journal, so the ledger fails closed.
//!
//! **Deviation from FOLD-008 (d), stated:** two runs of one program decided against the same
//! lifecycle state are both admitted. A run consumes nothing (it changes no lifecycle state and
//! no other run's record), so the second is not decided against a stale state; each is admitted
//! under its own run ID and branch. A replay of one run is still refused by its run ID.
//!
//! **A→B→A.** Lifecycle states only move forward except `unregister` (registered → draft). An
//! unregistered version is retired: its number is never reused, and registering the same bytes
//! again is refused (`conflict`) while an identified version has them, or yields a new version
//! number otherwise. So the (version, state) a run was decided against never comes back once
//! left: a plan for registered v1 never matches the draft or a later registered v2.

use crate::prepare::{decided_lifecycle, principal_rule, RunFlags};
use crate::record::{run_branch, run_params, Outcome, RunRecord};
use fid_bus::error::{BusError, ConflictReason, Detail, InternalReason};
use fid_bus::handler::{Event, EventType};
use fid_bus::policy::projection::{params_commitment, EntryOrigin};
use fid_bus::principal::Principal;
use fid_canon::CValue;
use fid_journal::branch::{EntryOrigin as JournalOrigin, EventSource};
use fid_program::Uuid7;
use fid_program_rec::life::LifeStore;
use fid_types::{CommandId, Digest, SideEffect};
use std::collections::BTreeMap;

/// The event that starts a run; reserved to `program.run@1`.
pub const RUN_STARTED: &str = "RunStarted";
/// The command that owns [`RUN_STARTED`] (major 1).
pub const RUN_COMMAND: &str = "program.run";
/// Event types of the pending-request flow (CORE.BUS.POL.PROJ).
pub const REQUEST_EVENT_PREFIX: &str = "Request";
const BODY_KEYS: [&str; 1] = ["record"];

fn invariant(what: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(what))
}

fn run_command_id() -> Result<CommandId, BusError> {
    CommandId::parse(&format!("{RUN_COMMAND}@1")).map_err(|_| invariant("command id"))
}

/// One committed entry as its verified header records it. Never built from an event body.
#[derive(Debug, Clone, PartialEq)]
pub struct RunEntry {
    /// Principal, command `id@major` and parameters commitment (`EntryOrigin::from_draft`).
    pub origin: EntryOrigin,
    /// Principal kind, command, class, branch and the stage that added the events.
    pub journal: JournalOrigin,
    /// The header's `promoted_from`, if this entry is a promoted copy.
    pub promoted_from: Option<Digest>,
    /// This entry's hash.
    pub hash: Digest,
}

impl RunEntry {
    /// The record the commit stage gives the entry of a `program.run@1` call by `principal`
    /// with parameters commitment `commitment`, committed on `branch`.
    pub fn for_call(
        principal: &Principal,
        commitment: Digest,
        branch: &str,
        hash: Digest,
    ) -> Result<RunEntry, BusError> {
        Ok(RunEntry {
            origin: EntryOrigin::new(principal.reference(), run_command_id()?, commitment),
            journal: JournalOrigin {
                kind: principal.kind(),
                command: RUN_COMMAND.to_owned(),
                major: 1,
                class: SideEffect::W2,
                branch: branch.to_owned(),
                source: EventSource::Handler,
                human_assertion: None,
            },
            promoted_from: None,
            hash,
        })
    }
}

/// The `RunStarted` event of `record`.
pub fn run_started(record: &RunRecord) -> Result<Event, BusError> {
    let mut body = BTreeMap::new();
    body.insert("record".to_owned(), record.to_value());
    Ok(Event::new(EventType::new(RUN_STARTED)?, CValue::Obj(body)))
}

/// What the ledger did with one entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Applied {
    /// No `RunStarted` event: nothing for the ledger.
    Ignored,
    /// Skipped, with the reason (a promoted copy).
    Skipped {
        /// Why.
        reason: &'static str,
    },
    /// The run was admitted.
    Admitted {
        /// Its run ID.
        run_id: Uuid7,
    },
}

/// Run records folded from committed entries in journal order (module docs).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunLedger {
    runs: BTreeMap<Uuid7, (RunRecord, Digest)>,
}

impl RunLedger {
    /// An empty ledger.
    pub fn new() -> RunLedger {
        RunLedger::default()
    }

    /// The admitted record of `run_id` and the hash of the entry that started it.
    pub fn get(&self, run_id: &Uuid7) -> Option<(&RunRecord, &Digest)> {
        self.runs.get(run_id).map(|(r, h)| (r, h))
    }

    /// The number of admitted runs.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// Whether no run is admitted.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    fn verdict(
        &self,
        entry: &RunEntry,
        events: &[Event],
        life: &LifeStore,
        stale: &dyn Fn(&str) -> BusError,
    ) -> Result<Applied, BusError> {
        let refuse = |what: &str| invariant(&format!("entry {}: {what}", entry.hash.as_str()));
        let mut mine = events.iter().filter(|e| e.type_id().as_str() == RUN_STARTED);
        let Some(event) = mine.next() else { return Ok(Applied::Ignored) };
        if mine.next().is_some() {
            return Err(refuse("one entry holds two RunStarted events"));
        }
        if events.iter().any(|e| e.type_id().as_str().starts_with(REQUEST_EVENT_PREFIX)) {
            return Err(refuse("RunStarted in an entry that carries a request event"));
        }
        if entry.promoted_from.is_some() {
            return Ok(Applied::Skipped { reason: "promoted_copy" });
        }
        let cmd = entry.origin.command();
        let j = &entry.journal;
        if cmd.name() != RUN_COMMAND || cmd.major() != 1 || j.command != RUN_COMMAND || j.major != 1 {
            return Err(refuse("RunStarted outside a program.run@1 entry"));
        }
        if j.class != SideEffect::W2 || j.source != EventSource::Handler {
            return Err(refuse("RunStarted not added by the program.run@1 handler in a W2 entry"));
        }
        let CValue::Obj(o) = event.body() else { return Err(refuse("RunStarted body is not an object")) };
        if o.len() != BODY_KEYS.len() || !BODY_KEYS.iter().all(|k| o.contains_key(*k)) {
            return Err(refuse("RunStarted body does not have exactly its one member"));
        }
        let record = o
            .get("record")
            .ok_or_else(|| refuse("no record"))
            .and_then(|v| RunRecord::from_value(v).map_err(|e| refuse(&e.to_string())))?;
        let principal = entry.origin.principal();
        if *record.principal() != *principal || j.kind != principal.kind {
            return Err(refuse("the record's principal is not the entry's"));
        }
        let derived =
            params_commitment(&run_params(&record)).map_err(|_| refuse("run parameters have no canonical form"))?;
        if derived != entry.origin.params_commitment() {
            return Err(refuse("the record is not the run the entry's parameters asked for"));
        }
        let own = run_branch(record.run_id());
        if record.branch() != Some(own.as_str()) || j.branch != own {
            return Err(refuse("a run starts on its own branch run/<run_id>"));
        }
        if record.outcome() != Outcome::Pending {
            return Err(refuse("a run starts pending"));
        }
        let flags = RunFlags { trial: record.trial(), override_deprecated: record.override_deprecated() };
        if principal_rule(record.lifecycle_state(), record.program_version(), principal.kind, flags).is_err() {
            return Err(refuse("the principal rules refuse this run"));
        }
        let now = decided_lifecycle(life, record.program_id(), &record.program_digest());
        if now != (record.program_version(), record.lifecycle_state()) {
            return Err(stale("the program's lifecycle state is not the one the run was decided against"));
        }
        if self.runs.contains_key(&record.run_id()) {
            return Err(stale("the run is already recorded"));
        }
        Ok(Applied::Admitted { run_id: record.run_id() })
    }

    /// The commit stage's check, run atomically with the commit (module docs): the fold's
    /// rules, with a decided-against failure as `conflict/head_moved`.
    pub fn precommit(&self, entry: &RunEntry, events: &[Event], life: &LifeStore) -> Result<Applied, BusError> {
        let stale = |what: &str| BusError::Conflict(ConflictReason::HeadMoved, Detail::new(what));
        self.verdict(entry, events, life, &stale)
    }

    /// Folds one committed entry (module docs). On refusal the ledger is unchanged.
    pub fn apply_entry(&mut self, entry: &RunEntry, events: &[Event], life: &LifeStore) -> Result<Applied, BusError> {
        let hash = entry.hash;
        let stale = |what: &str| invariant(&format!("entry {}: {what}", hash.as_str()));
        let applied = self.verdict(entry, events, life, &stale)?;
        if let Applied::Admitted { run_id } = applied {
            let record = events
                .iter()
                .find(|e| e.type_id().as_str() == RUN_STARTED)
                .and_then(|e| match e.body() {
                    CValue::Obj(o) => o.get("record"),
                    _ => None,
                })
                .ok_or_else(|| invariant("record vanished"))
                .and_then(|v| RunRecord::from_value(v).map_err(|e| invariant(&e.to_string())))?;
            self.runs.insert(run_id, (record, hash));
        }
        Ok(applied)
    }
}
