---
id: CORE.RPL.RETGT.REBASE
title: Rebase re-execution for non-fast-forward promotion
level: 4
parent: CORE.RPL.RETGT
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL.RETGT-005]
covers: []
depends_on: [CORE.RPL.EXEC.LOOP, CORE.JRN.CHAIN, CORE.RPL.CLASS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.RPL.RETGT.REBASE: Rebase re-execution for non-fast-forward promotion

## Purpose

When a human promotes a branch whose target has moved on, `CORE.JRN.BR` needs the source branch's commands re-executed on top of the new target head. REBASE finds the common ancestor, re-executes the source commands after it on a scratch copy of the target head, and returns the drafts and per-step outcomes. It moves no head and appends nothing; BR records the result with `promoted_from`.

## Scope

**In:** common-ancestor search on the logical DAG, re-execution of the source suffix on the target head into a scratch sink, conflict detection, the `RebaseResult` returned to BR.
**Out:** recording entries and moving heads (`CORE.JRN.BR`); deciding who may promote (`CORE.JRN.BR`, `CORE.BUS.POL`).

## Requirements

- **REQ-CORE.RPL.RETGT.REBASE-001** When asked to rebase source head S onto target head T, the engine shall find the nearest common ancestor A of S and T on the logical DAG.
- **REQ-CORE.RPL.RETGT.REBASE-002** The engine shall re-execute, in order, every W-class entry on the path from A (exclusive) to S, on a scratch state reconstructed at T, with commits sent to a scratch sink.
- **REQ-CORE.RPL.RETGT.REBASE-003** The result shall hold, per re-executed step, the source entry hash, the draft entry for the target, and a comparison outcome of the new outputs against the source outputs.
- **REQ-CORE.RPL.RETGT.REBASE-004** If a step fails its precondition or returns a bus error on the target state, then the engine shall stop and return `conflict` naming that step, with the drafts of the steps before it.
- **REQ-CORE.RPL.RETGT.REBASE-005** The engine shall move no head and append no entry to any journal.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/rpl/retgt/rebase/diverged` (branch `agent/1` with 6 entries after A, `main` with 3 entries after A that touch other elements), when rebased, then the result has 6 drafts, each outcome `IDENTICAL` or `EQUIVALENT`, and both heads are unchanged.
- **AC-2** (error path) Given `fixtures/core/rpl/retgt/rebase/conflict` where `main` deleted an element that `agent/1` step 3 edits, then the result is `conflict` at step 3 with 2 drafts.
- **AC-3** Given S already a descendant of T, then the result has the same drafts as a plain re-execution of S's suffix (BR would fast-forward instead; this checks the function is total).

## Design

- **Registry command(s):** none; library called by `CORE.JRN.BR` through an injected `Rebaser` trait.
- **Data structures:** `RebaseResult { ancestor, steps: Vec<RebaseStep { source_entry, draft: EntryDraft, outcome }>, conflict: Option<Conflict { seq, error }> }`.
- **Algorithm:** ancestor by walking logical parents from both heads with visited sets in ordered maps; reconstruct T (injected `Reconstructor`); LOOP over the suffix with a scratch sink; compare outputs with `CORE.RPL.CLASS`.
- **Errors:** `conflict`, `no_common_ancestor` (cannot happen within one project; reported if the DAG is damaged).
- **Determinism:** D0.
- **Performance budget:** ancestor search over 100,000 entries in < 100 ms [unverified until measured].
- **Crate:** `fid-replay` (module `rebase`).

## Test fixtures

`fixtures/core/rpl/retgt/rebase/` synthetic diverged branches from the seeded generator; CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `rebase` with `rebase(journal, source_head, target_head) -> RebaseResult`: find the nearest common ancestor on the logical DAG (ordered maps, no hash maps), reconstruct the target head through an injected `Reconstructor`, re-execute the source suffix's W entries via `run::replay_span` into a scratch sink, and return per-step drafts with outcomes from `classify`. Stop with `conflict` at the first precondition or bus failure. Move no head, append nothing. Expose it to `CORE.JRN.BR` through a `Rebaser` trait so BR has no crate dependency on `fid-replay`. Tests AC-1..AC-3. Constraints: CONSTITUTION §3 (humans promote), §4, §8.

## Open questions

- Whether BR should accept `EQUIVALENT` steps or demand `IDENTICAL` before recording a promotion is BR's policy; this leaf only reports.

## Children

_Leaf: none._
