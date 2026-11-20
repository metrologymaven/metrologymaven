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
updated: 2026-11-17
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

## Children

_Leaf: none._
