---
id: CORE.RPL.EXEC.LOOP
title: Re-execution loop, old-major dispatch and step cursor
level: 4
parent: CORE.RPL.EXEC
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.EXEC-003, REQ-CORE.RPL.EXEC-007]
covers: []
depends_on: [CORE.BUS.PIPE, CORE.BUS.PRIN, CORE.JRN.CHAIN, CORE.JRN.ENTRY, CORE.REG.VER.LIFE, CORE.RPL.EXEC.SUBST]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.RPL.EXEC.LOOP: Re-execution loop, old-major dispatch and step cursor

## Purpose

The engine room of every replay mode. LOOP walks a journal span in sequence order, rebuilds each recorded call (command ID, major, arguments from the payload), dispatches it through the bus as principal `system` on behalf of the requester, with observations supplied by SUBST and commits sent to a scratch sink, and collects per-step recorded and replayed digests for CLASS. It also applies migrators for old majors and exposes a step cursor so a viewer can advance one entry at a time.

## Scope

**In:** span iteration, call reconstruction, bus dispatch with a scratch commit sink, hidden-major dispatch and migrator application, per-step `StepComparison` assembly, step cursor, cancellation through `CORE.JOB`.
**Out:** where observations come from and missing-item detection (`CORE.RPL.EXEC.SUBST`); worker processes and kernel choice (`CORE.RPL.EXEC.VERIFY`); report and commands (`CORE.RPL.EXEC.REPORT`); classification (`CORE.RPL.CLASS`); new-branch output (`CORE.RPL.RETGT`).

## Requirements

- **REQ-CORE.RPL.EXEC.LOOP-001** When a span is replayed, the loop shall re-execute its W-class entries in sequence order through the bus with principal kind `system` and the requester recorded as `on_behalf_of`.
- **REQ-CORE.RPL.EXEC.LOOP-002** When a recorded entry names a `hidden` major, the loop shall dispatch to that major, and the bus shall accept it because the caller is the replay engine.
- **REQ-CORE.RPL.EXEC.LOOP-003** When the recorded major is `removed` and a registered migrator chain leads from it to a retained major, the loop shall apply the migrators in order to the recorded arguments, dispatch to the resulting major, and record the migrator IDs used for that step.
- **REQ-CORE.RPL.EXEC.LOOP-004** Every replayed commit shall go to a scratch sink owned by the replay, and the loop shall never call the journal's append on any branch of the source project.
- **REQ-CORE.RPL.EXEC.LOOP-005** For each step the loop shall produce a `StepComparison` holding the determinism class, kernel and platform flags, recorded and replayed output digests and values with quantity kinds, and decisions.
- **REQ-CORE.RPL.EXEC.LOOP-006** The step cursor shall advance exactly one entry per call and return the step's comparison and the scratch state digest after it.
- **REQ-CORE.RPL.EXEC.LOOP-007** When the replay job is cancelled, the loop shall stop before the next step and return the comparisons made so far, marked `cancelled`.

## Acceptance criteria

- **AC-1** Given fixture journal `fixtures/core/rpl/exec/loop/session_50` (50 W2 entries of fixture commands), when replayed with a recording `JournalSink` probe, then the source journal's entry count and head hash are unchanged and the scratch sink holds 50 commits.
- **AC-2** Given `fixtures/core/rpl/exec/loop/hidden_major` (an entry for `demo.cmd@1`, now hidden), when replayed, then the step dispatches to `@1` and its comparison has digests equal to the recording.
- **AC-3** Given `fixtures/core/rpl/exec/loop/migrated` (an entry for removed `demo.cmd@1` with migrator `demo.migrate_cmd_1_2@1.0.0`), when replayed, then the step runs `demo.cmd@2` and the step record lists the migrator ID.
- **AC-4** Given the cursor on `session_50`, when advanced 10 times, then it returns entries 1..10 in order and the scratch state digest after step 10 equals the fixture's recorded digest.
- **AC-5** Given a cancel after step 20, then exactly 20 comparisons are returned, marked `cancelled`.

## Design

- **Registry command(s):** none; library used by REPORT, RETGT and XVER.
- **Data structures:** `Span { branch, from_seq, to_seq }`; `ReplayCall { command: CommandRef, args, migrators: Vec<AlgorithmId> }`; `StepComparison` (type from `CORE.RPL.CLASS`); `Cursor { span, next_seq, scratch: ScratchState }`; `JournalSink` trait with `ScratchSink` implementation.
- **Algorithm:** read entries with `Journal::entries(range)`; skip R0/R1 records (not in the Record Journal); for each entry build the call from its payload (arguments verified against the header commitment), resolve major via lifecycle records, run migrators, call `Bus::execute` with the replay channel and the scratch sink, collect outputs.
- **Errors:** `payload_commitment_mismatch` (passed to CLASS as divergent input), `bus_error` per step (recorded, loop continues), `cancelled`.
- **Determinism:** D0 for the loop's own logic; step outputs carry their command's class.
- **Performance budget:** loop overhead < 1 ms per step excluding handler time [unverified until measured].
- **Crate:** `fid-replay` (module `run`).

## Test fixtures

Synthetic journals under `fixtures/core/rpl/exec/loop/` produced by a fixture generator from fixture commands `demo.*` (seeded); CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `run`: `replay_span(span, &dyn ObservationSource, &Bus, Requester) -> Vec<StepComparison>` and a `Cursor` with `step()`. For each W-class entry of the span in sequence order: rebuild the call from the payload (verify arguments against the header commitment), resolve the major via `fid-registry` lifecycle records (dispatch hidden majors; apply migrator chains for removed majors and record their IDs), execute through `Bus::execute` as principal `system` on behalf of the requester with commits sent to a `ScratchSink` (never append to the source journal), and assemble a `StepComparison` for `fid-replay::classify`. Support cancellation between steps. Tests AC-1..AC-5 with fixtures in `fixtures/core/rpl/exec/loop/`. Constraints: CONSTITUTION §3 (one journal event per committed command: replays commit nothing to the source), §4, §8 (no panic, no `unsafe`), BTreeMap only.

## Open questions

- `CORE.BUS.PIPE` must offer a commit-sink option for replay; without it, AC-1 cannot pass (cross-node follow-up).

## Children

_Leaf: none._
