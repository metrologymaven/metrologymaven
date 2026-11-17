---
id: CORE.RPL.RETGT
title: Re-targeted, dry-run and rebase re-execution
level: 3
parent: CORE.RPL
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL-010, REQ-CORE.RPL-011]
covers: []
depends_on: [CORE.JRN.CHAIN, CORE.JRN.BR, CORE.BUS.PIPE, CORE.RPL.EXEC.LOOP]
cli: [fid journal replay-retargeted, fid journal replay-dry-run]
mcp: [journal_replay_retargeted, journal_replay_dry_run]
journal: [journal.replay_retargeted@1, journal.replay_dry_run@1]
updated: 2026-11-17
---

# CORE.RPL.RETGT: Re-targeted, dry-run and rebase re-execution

## Purpose

Re-execution that produces *new* state rather than checking old state. Re-targeted replay runs a recorded span with named inputs bound to new data (the next part, a new scan) and writes the results to a new branch. Dry-run replay runs a span on a throwaway branch and returns a changeset diff without touching any persistent branch. The same machinery re-executes a source branch's commands onto a moved target head when a promotion is not a fast-forward (`CORE.JRN.BR`).

## Scope

**In:**
- Input binding model (which recorded inputs are re-bindable, type checks on the new data)
- Re-targeted replay to a new branch
- Dry-run on a throwaway branch with a changeset diff
- Rebase re-execution for non-fast-forward promotion, returning entries for BR to record
- Commands `journal.replay_retargeted@1` (W2, new branch) and `journal.replay_dry_run@1` (R1)

**Out** (and which node owns it instead):
- Command Programs and their binding syntax (owned by `AUTO.PROG`)
- Motion simulation for dry-run of motion requests (owned by `CMM`)
- Recording the promotion (owned by `CORE.JRN.BR`)

## Requirements

- **REQ-CORE.RPL.RETGT-001** When replaying in re-targeted mode, the replay engine shall bind each named input to the supplied data and write all results to a new branch, never to the source branch.
- **REQ-CORE.RPL.RETGT-002** If a supplied binding's entity type differs from the recorded input's type, then the replay engine shall reject the replay with a typed `binding_type` error before executing any step.
- **REQ-CORE.RPL.RETGT-003** When replaying in dry-run mode, the replay engine shall execute on a throwaway branch, return a changeset diff, and leave every persistent branch head unchanged.
- **REQ-CORE.RPL.RETGT-004** When the throwaway branch of a dry run is discarded, no entry of it shall remain in the Record Journal.
- **REQ-CORE.RPL.RETGT-005** When asked to rebase a source branch onto a target head, the replay engine shall re-execute the source branch's commands after their common ancestor on the target head and return the resulting entries and per-step outcomes without moving any head.

## Interfaces

- **Offers:** `Replay::run(Retargeted { bindings }, span)` and `journal.replay_retargeted@1` (RUN); `Replay::run(DryRun, span)` and `journal.replay_dry_run@1` (DRY); `Replay::rebase(source, target_head) -> RebaseResult` (REBASE), called by `CORE.JRN.BR` through an injected trait; the changeset diff type is `CORE.JRN.BR`'s diff result.
- **Commands:** `journal.replay_retargeted@1` (W2), `journal.replay_dry_run@1` (R1).
- **Consumes:** `CORE.JRN.CHAIN`, `CORE.JRN.BR` (branch creation and diff type), `CORE.BUS.PIPE`, `CORE.RPL.EXEC.LOOP`.

## Decomposition hints

Applied: RUN (binding model plus the re-targeted command), DRY, REBASE. BIND was folded into RUN, its only user.

## Open questions

- **Resolved:** throwaway-branch entries live in an in-memory scratch sink (`CORE.RPL.EXEC.LOOP`), never in a database table, so REQ-004 holds by construction.
- `CORE.JRN.BR` must call REBASE through an injected trait; a crate dependency from BR to RPL would form a cycle through `CORE.BUS.PIPE → CORE.JRN` (cross-node follow-up).
- `MIG.EXPLAIN` depends on this node for re-targeted replay; it should narrow its dependency to `CORE.RPL.RETGT.RUN`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.RPL.RETGT.RUN | Re-targeted replay to a new branch (leaf) | 001 002 | none |
| CORE.RPL.RETGT.DRY | Dry-run replay with changeset diff (leaf) | 003 004 | none |
| CORE.RPL.RETGT.REBASE | Rebase re-execution for non-fast-forward promotion (leaf) | 005 | none |
