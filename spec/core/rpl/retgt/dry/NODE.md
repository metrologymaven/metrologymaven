---
id: CORE.RPL.RETGT.DRY
title: Dry-run replay with changeset diff
level: 4
parent: CORE.RPL.RETGT
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.RETGT-003, REQ-CORE.RPL.RETGT-004]
covers: []
depends_on: [CORE.RPL.EXEC.LOOP, CORE.JRN.BR]
side_effect: R1
determinism: D0
cli: [fid journal replay-dry-run]
mcp: [journal_replay_dry_run]
journal: [journal.replay_dry_run@1]
updated: 2026-11-17
---

# CORE.RPL.RETGT.DRY: Dry-run replay with changeset diff

## Purpose

"What would happen if I ran these commands here?" The command executes a recorded span on top of a chosen head in an in-memory throwaway branch and returns the changeset diff (entities added, removed and changed, with old and new output digests) without writing anything to the project. Agents use it to preview a plan; people use it before a re-targeted run.

## Scope

**In:** the in-memory throwaway branch, the changeset diff in `CORE.JRN.BR`'s diff format, the memory bound, the command `journal.replay_dry_run@1`.
**Out:** motion simulation for M1 steps (`CMM`); persistent output (`CORE.RPL.RETGT.RUN`); diff rendering (`VIZ.INBOX`).

## Requirements

- **REQ-CORE.RPL.RETGT.DRY-001** When `journal.replay_dry_run@1` runs, the engine shall execute the span on top of the chosen base head with every commit sent to an in-memory scratch sink.
- **REQ-CORE.RPL.RETGT.DRY-002** The command shall return the changeset diff between the base head and the scratch head, keyed by stable entity ID with old and new output digests, in the result type of `journal.diff_branch@1`.
- **REQ-CORE.RPL.RETGT.DRY-003** After the command returns, fails or is cancelled, every persistent branch head shall be unchanged and the Record Journal shall hold no entry from the dry run.
- **REQ-CORE.RPL.RETGT.DRY-004** Steps that need I1, D1 or M1 observations shall be served from the recording when the span is replayed on its own recorded base, and otherwise reported `NOT_REPLAYABLE(device)` without calling any device or simulator.
- **REQ-CORE.RPL.RETGT.DRY-005** If the scratch state exceeds the memory bound (default 1 GiB) [unverified that 1 GiB suits large scans], then the command shall stop with `dry_run_too_large` and discard the scratch state.
- **REQ-CORE.RPL.RETGT.DRY-006** The command shall be class R1 and shall be logged to the Activity Log only under the D-031 rules.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/rpl/retgt/dry/part_a` and a span of 10 entries replayed on the head of branch `b`, when `fid journal replay-dry-run --branch main --from 3 --to 12 --base b --json` runs, then the diff equals `fixtures/core/rpl/retgt/dry/expected_diff.json`.
- **AC-2** Given AC-1, when the database is inspected afterwards, then the entry count and every branch head are unchanged.
- **AC-3** (error path) Given a cancel after step 4, then the command returns `cancelled` and AC-2's check still holds.
- **AC-4** Given a span with a D1 device read replayed on a different base, then that step is `NOT_REPLAYABLE(device)` and no device or simulator is called.
- **AC-5** (error path) Given a memory bound of 1 MiB and a span that creates a 2 MiB element, then the command fails with `dry_run_too_large` and AC-2 holds.

## Design

- **Registry command(s):** `journal.replay_dry_run@1`: R1, projection `exposed`, `fs_effect: none`, `network: none`, `web: unavailable` (note "needs project storage"); params `{ branch, from_seq, to_seq, base?: string (entry hash), memory_limit_mib?: integer }`; result `{ diff: BranchDiff, steps: [{ seq, outcome }] }`. Runs as a cancellable job.
- **Data structures:** `ScratchBranch { base, sink: ScratchSink, state }`; `BranchDiff` (type from `CORE.JRN.BR`).
- **Algorithm:** reconstruct base state (through the injected `Reconstructor`) → LOOP with scratch sink → diff by entity ID → drop scratch.
- **Errors:** `span_invalid`, `dry_run_too_large`, `cancelled`.
- **Determinism:** D0 (diff sorted by entity ID).
- **Performance budget:** overhead < 1 ms per step beyond handler time [unverified until measured].
- **Crate:** `fid-replay` (module `dry_run`).

## Test fixtures

`fixtures/core/rpl/retgt/dry/` synthetic projects and expected diffs from the seeded generator; CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `dry_run` and register `journal.replay_dry_run@1` (R1, exposed; params branch, from_seq, to_seq, optional base, optional memory_limit_mib default 1024; cancellable job). Reconstruct the base state, run the span through `run::replay_span` with an in-memory `ScratchSink`, return the `BranchDiff` (the result type of `journal.diff_branch@1`) between base and scratch heads plus per-step outcomes, then drop all scratch state. Never write to the database: verify by entry count and head hashes in tests. Device steps are served from recordings only on their recorded base, otherwise `NOT_REPLAYABLE(device)`; never call a device or simulator. Enforce the memory bound with `dry_run_too_large`. Tests AC-1..AC-5. Constraints: CONSTITUTION §3 (R1 not in the Record Journal, D-031 Activity Log), §8.

## Open questions

- A dry run that should simulate motion belongs to `CMM`'s simulator path; this leaf never simulates.

## Children

_Leaf: none._
