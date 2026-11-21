---
id: CORE.BUS.PIPE.STAGE
title: Stage orchestration, all-or-nothing commit, jobs, reads and overhead
level: 4
parent: CORE.BUS.PIPE
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS.PIPE-001, REQ-CORE.BUS.PIPE-008, REQ-CORE.BUS.PIPE-010]
covers: []
depends_on: [CORE.BUS.PIPE.VALID, CORE.BUS.PIPE.HANDLER, CORE.BUS.PIPE.ERR, CORE.BUS.PRIN, CORE.BUS.POL.CLASS, CORE.BUS.POL.PROJ, CORE.BUS.POL.FS, CORE.BUS.POL.EGRESS, CORE.BUS.ROUTE, CORE.BUS.UNIT, CORE.JRN.CHAIN.APPEND, CORE.JRN.STREAM.ALOG, CORE.JOB.SCHED]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# CORE.BUS.PIPE.STAGE: Stage orchestration, all-or-nothing commit, jobs, reads and overhead

## Purpose

`Bus::execute` itself. STAGE runs the fixed stage order (resolve, validate, authorise, precondition, route, handler, effects check, commit, return), calls each sibling module at its stage, and guarantees that a failure anywhere commits nothing. It hands long-running commands to the job system and commits only on success, hands completed reads to the Activity Log writer (D-031), carries consumed R1 digests into W-class entries, offers dry-run and the handler-only path that recompute uses, and owns the overhead benchmark.

## Scope

**In:** stage order; the commit transaction boundary; the invocation envelope (`command`, `params`, `expected_head`, `idempotency_key`, `dry_run`, `approval_of`, `consumed_r1`, `program_step`); job hand-off and deferred commit; read hand-off to the Activity Log; dry-run; `run_handlers_as_system` for `CORE.DEP.RECOMP`; the overhead benchmark.
**Out:** resolution, validation and preconditions (`CORE.BUS.PIPE.VALID`); handler trait and effects check (`CORE.BUS.PIPE.HANDLER`); error codes (`CORE.BUS.PIPE.ERR`); principal (`CORE.BUS.PRIN`); authorisation and pending requests (`CORE.BUS.POL.*`); branch, conflict and idempotency (`CORE.BUS.ROUTE`); literal normalisation (`CORE.BUS.UNIT`); append and durability (`CORE.JRN.CHAIN.APPEND`); Activity Log storage and the recording-session flag (`CORE.JRN.STREAM.ALOG`); scheduling (`CORE.JOB.SCHED`).

## Requirements

- **REQ-CORE.BUS.PIPE.STAGE-001** When a command is invoked, the bus shall run resolve, validate, authorise, precondition, route, handler, effects check and commit in that order, each stage at most once.
- **REQ-CORE.BUS.PIPE.STAGE-002** If any stage returns an error, then the bus shall return that error, run no later stage, and leave the Record Journal, the branch heads and the idempotency table unchanged.
- **REQ-CORE.BUS.PIPE.STAGE-003** When the authorise stage returns `PendingRequest`, the bus shall commit only the pending-request entry produced by `CORE.BUS.POL.PROJ` and shall not run the handler.
- **REQ-CORE.BUS.PIPE.STAGE-004** When a command declared long-running passes the route stage, the bus shall submit its handler to the job system, return the job ID at once, and run the effects check and commit only after the job ends `succeeded`.
- **REQ-CORE.BUS.PIPE.STAGE-005** If the target branch head moved while a long-running job ran, then the bus shall return a typed `conflict` error at commit time and commit nothing.
- **REQ-CORE.BUS.PIPE.STAGE-006** When an R0 or R1 command returns, the bus shall hand the call record to the Activity Log writer and shall write nothing to the Record Journal.
- **REQ-CORE.BUS.PIPE.STAGE-007** When a W-class invocation lists R1 result digests in `consumed_r1`, the bus shall copy them into the entry's input digests.
- **REQ-CORE.BUS.PIPE.STAGE-008** When `dry_run` is set, the bus shall run every stage up to and including the effects check, return the events it would commit, and commit nothing.
- **REQ-CORE.BUS.PIPE.STAGE-009** When `run_handlers_as_system` is called by a `system` principal, the bus shall run validate, precondition, handler and effects check for each listed command against the given state view and return the events without committing; any other principal shall get a `policy` error.
- **REQ-CORE.BUS.PIPE.STAGE-010** The bus shall add no more than 1 ms median overhead per R0 command, measured over 10,000 calls of a no-op R0 command on reference hardware [unverified until measured].
- **REQ-CORE.BUS.PIPE.STAGE-011** When a handler whose command declares an `fs_effect` other than `none` succeeds, the bus shall have it prepare its file under a temporary name, commit the journal entry, and only then place the file, discarding the prepared file if the commit fails; when `dry_run` is set, the bus shall pass it to the handler as the handler's own dry run, so no file (temporary or final) is written, and still return the would-be events.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/pipe/stage/probes` (a test registry whose fake modules record call order), when a W2 command runs, then the recorded order is resolve, validate, authorise, precondition, route, handler, effects, commit.
- **AC-2** (error path) Given each stage in turn forced to fail, when the command runs, then the error of that stage is returned, later stages are not called, and the journal head and idempotency table are byte-identical to before.
- **AC-3** Given an MCP call to a `request_only` command, when executed, then exactly one pending-request entry is appended and the handler call count is 0.
- **AC-4** Given a long-running test command with a 200 ms handler, when invoked, then a job ID returns within 10 ms, and the entry appears only after the job succeeds; when the job is cancelled, then nothing is appended.
- **AC-5** (error path) Given a long-running job and a second W2 commit on the same branch while it runs, when the job succeeds, then the result is `conflict` and nothing from the job is appended.
- **AC-6** Given an R0 call while no recording session is active, then the Record Journal and the Activity Log are unchanged; with recording active, then one Activity Log record exists and the Record Journal is unchanged.
- **AC-7** Given a W2 call with `consumed_r1: ["sha256:..."]`, when committed, then the entry's input digests contain that digest.
- **AC-8** Given `dry_run` on a W2 command, then the returned events equal those of a real run and the journal is unchanged.
- **AC-9** (error path) Given a `script` principal calling `run_handlers_as_system`, then the result is `policy`.
- **AC-10** Given the benchmark `bench_r0_overhead` (10,000 no-op R0 calls), when run on reference hardware, then the median overhead is below 1 ms [unverified until measured].
- **AC-11** (error path) Given a test `write_root` command whose handler prepares a file, when the journal commit is forced to fail, then no file exists at or beside the target; when the commit succeeds, the file exists only after the entry is committed; and with `dry_run`, the returned events equal those of a real run and the export root is unchanged.

## Design

- **Registry command(s):** none; library API.
- **Data structures:** `Invocation { command: CommandRef, params: Json, expected_head: Option<EntryHash>, idempotency_key: Option<Key>, dry_run: bool, approval_of: Option<RequestId>, consumed_r1: Vec<Digest>, program_step: Option<ProgramStepRef> }`; `CommandResult { Done { result, entry: Option<EntryHash> } | Job { job_id } | Pending { request_id } | DryRun { events } }`; `trait Stage` implemented by each sibling module for test probes.
- **Algorithm:** a straight-line function with early return on `Err`; the commit stage holds the branch writer lock from `CORE.BUS.ROUTE` and calls `Journal::append` once; for jobs, the handler closure runs under `Jobs::submit` and a completion callback re-acquires the lock, re-checks the head, runs the effects check and appends.
- **Errors:** all from `CORE.BUS.PIPE.ERR`; this module adds none.
- **Determinism:** D0; no clock or randomness of its own (job IDs come from `CORE.JOB.SCHED`).
- **Performance budget:** REQ-010.
- **Crate:** `fid-bus` (module `pipeline`).

## Test fixtures

`fixtures/core/bus/pipe/stage/probes`: a synthetic test registry with no-op, failing, long-running and R1 commands and probe modules; generated by a committed script; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `pipeline` implementing `Bus::execute(invocation, channel) -> Result<CommandResult, BusError>`. Run in order: resolve and validate (module `validate`), authorise (`policy`: returns Run, PendingRequest or Refuse), precondition (`validate`), route (`route`: branch, writer lock, expected_head, idempotency), handler (`handler`), effects check (`handler`), commit (`Journal::append` once, under the writer lock). Any error returns at once and changes nothing. PendingRequest commits only the request entry. Long-running commands: submit to `fid-jobs` `Jobs::submit`, return the job ID, and on success re-lock, re-check the head (`conflict` if moved), effects-check and append; on failure or cancel append nothing. R0/R1: hand the call record to the journal's Activity Log writer (it decides, from the recording flag and `program_step`, whether to store it); never append to the Record Journal. Copy `consumed_r1` digests into the entry's input digests. `dry_run` stops after the effects check and returns the events. Add `run_handlers_as_system(commands, view)` for `system` principals only (used by recompute). Add benchmark `bench_r0_overhead`. Tests: AC-1..AC-10 with `fixtures/core/bus/pipe/stage/probes`. Constraints: CONSTITUTION §3 (side-effect classes, Reads D-031), C3 (no side doors), §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- Whether a long-running command whose head moved should be offered an automatic rebase instead of `conflict`; this leaf returns `conflict` and leaves retry to the caller.
- **Build status (2026-11-21, partial; `built_in` not set).** Built in `fid-bus` module `pipeline`: `Bus::execute` with the fixed stage order, the all-or-nothing synchronous commit with its compare-and-swap, pending-request filing, the Activity Log hand-off of reads, dry run, `run_handlers_as_system`, and the generic commit hooks below. Tests: `crates/fid-bus/tests/pipeline.rs` against `fixtures/core/bus/pipe/stage/probes` (reference script `fixtures/core/bus/pipe/stage/make_fixtures.py`, hand-labelled outcomes).
  - Covered: AC-1, AC-2, AC-3, AC-6, AC-7, AC-8, AC-9; AC-10 as a CI smoke bound only (CI runners are not the reference hardware).
  - **Deferred ACs:** AC-4 and AC-5 (REQ-004, REQ-005: long-running commands and the job hand-off). The registry has no long-running field yet, and `fid-jobs` depends on `fid-bus`, so the hand-off needs a job seam in `fid-bus` that a higher crate implements with `fid_jobs::Jobs`. The synchronous commit already returns `conflict/head_moved` when the branch head moved, which the job path will reuse. AC-11 (REQ-011: prepare under a temporary name, commit, then place): the handler contract (CORE.BUS.PIPE.HANDLER REQ-001) lets a handler return only events, so a handler cannot hand the bus a prepared file. That contract needs an extension first.
  - **Fail closed until CORE.BUS.ROUTE:** the route stage takes the branch from an injected `Router`. `OpenBranch` sends every commit to the session's open branch and refuses an agent's commit-class call with `policy/agent_write_forbidden`. A call with `idempotency_key` is refused with `validation/schema` rather than ignored. The writer lock is one commit lock per `Bus`, not a per-branch FIFO lock.
  - **Not wired yet:** the network permit (`issue_permit`) of X1, I1 and D1 handlers (the handler contract has no permit parameter; CORE.BUS.POL.EGRESS AC-4); release protection of `main` on a first W3 entry (CORE.JRN.BR.NAME REQ-006 / AC-4), which needs an entry-level record of which stage added an event; checking the result value against `result_schema`.
- **Commit (as built).** Under the bus's commit lock: (1) read the stream tail, then the branch head, which must equal the call's view head (the decided-against state), else `conflict/head_moved`; (2) build the entry, and any leading entries, with fresh salts at the next sequence numbers, so the hash is known before the append; (3) run each watching hook's `precommit` on that built entry; (4) append with `expected_prev` = the tail read in (1) and the same salts, so the stored entry is byte-identical to the one judged. Any append in between makes the append fail with `conflict/head_moved`. (5) Fold the entry into each watching hook. A failure in (1) to (4) appends nothing. A failure after the append (hash mismatch, fold refusal) poisons the bus: every later commit is `internal/invariant_violated` until the project is reopened.
- **Commit hook API (as built; REQ-CORE.RPL.RECON.FOLD-008 (d)).** `Bus::register_hook(Arc<dyn CommitHook>)`. A hook declares `watch() -> Watch` (`All`, or `Effects(set of event types)`). It implements `precommit(&CommitRecord, &[Event]) -> Result<(), BusError>`, called before the append and returning `conflict/head_moved` for a stale decision, and `fold(&CommitRecord, &[Event])`, called after the append. `CommitRecord` is built only by the commit stage from the entry about to be appended. It gives: `origin()` (the bus `EntryOrigin`); `journal()` (the journal `EntryOrigin`, with source `handler` and the header's `human_assertion`); `promoted_from()` (always `None`, because the stage writes no promoted copies); `decided_at()` (the branch head before the entry); `header()`; `payload()` (salts and openings); `hash()`; and `args()`. Hooks run in registration order. Two more registrations shape the entry itself. `Bus::set_reason_param(command, field)` records a top-level string parameter as the entry's `reason`; CORE.JRN.UNDO.SUPER uses `reason`. `Bus::register_leading(command, Arc<dyn LeadingEntries>)` appends session-header entries right before the command's entry, chained in the same compare-and-swap; any other entry kind is refused. This serves CORE.PRJ.CONT.LIFE create and open. If the command's own append fails after a leading session header was appended, that header stays: the journal has no multi-entry transaction.
- **Deviations (as built).** `execute` takes a `Call { sessions, channel, view, roots }` besides the invocation, because validation, authorisation and preconditions read the state view and the precondition roots. `Invocation.command` is a `CommandId`. `CommandResult` is `Done { result, events, entry, activity } | Pending { request_id, entry } | DryRun { events } | WouldFile { command }`; there is no `Job` variant until REQ-004 is built. A handler's result value comes from the new provided method `Handler::run_with_result`, which defaults to no result. `trait Stage` is a refuse-only `StageProbe` called as each stage starts. A read skips the route and commit stages; a filing skips the precondition, handler and effects stages.
- **Obligations the hooks now let other leaves meet (each leaf wires its own hook):** CORE.JRN.UNDO.SUPER registers a hook whose `precommit` is `Supersessions::precommit` and sets `reason` as its reason parameter. CORE.RPL.RECON.FOLD-008 (d) folds (TRUST.PROFILE.SELECT, AUTO.RUN.BIND, CORE.BUS.POL.PROJ's `RequestBook`) each register their precommit and fold. CORE.PRJ.CONT.LIFE registers leading session headers and gets `promoted_from` from `CommitRecord`. Still open for CORE.JRN.BR.PROMO: its `head_move` and `promoted_from` entries (the stage commits command entries only) and committing a request entry on the request's source branch (a `Router` concern for CORE.BUS.ROUTE).

## Children

_Leaf: none._
