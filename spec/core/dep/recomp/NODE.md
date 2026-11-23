---
id: CORE.DEP.RECOMP
title: Deterministic recompute with failure isolation and single-entry commit
level: 3
parent: CORE.DEP
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.DEP-004, REQ-CORE.DEP-005, REQ-CORE.DEP-006, REQ-CORE.DEP-009, REQ-CORE.DEP-010]
covers: [GOM.PRJ.05]
depends_on: [CORE.DEP.EDGE, CORE.DEP.PROP, CORE.JOB.SCHED, CORE.JOB.PROG, CORE.BUS.PIPE.STAGE, CORE.PRJ.TREE]
side_effect: W2
determinism: D0
cli: [fid project recompute-dependents]
mcp: [project_recompute_dependents]
journal: [project.recompute_dependents@1]
built_in: [fid-dep]
updated: 2026-11-23
---

# CORE.DEP.RECOMP: Deterministic recompute

## Purpose

The parametric engine's core loop. RECOMP takes a set of outdated elements, orders them topologically with a deterministic tie-break, re-runs each element's own registered command and recorded arguments against its current inputs through the bus's handler-only path, isolates failures, and commits the whole recompute as one journal entry that lists every recomputed element with its new output digest and every failure with its error code. It runs as a cancellable job; cancellation discards everything.

## Scope

**In:** `project.recompute_dependents@1` (W2) and its scopes (element list, `all_outdated`, subtree); topological order with ascending-ID tie-break; per-element handler runs as `system` with `on_behalf_of` the caller; failure isolation; one recompute entry; job progress and cancellation between elements; determinism checks for D0 and D1 elements. P0 runs elements sequentially.
**Out:** marking and the auto/manual mode (`CORE.DEP.PROP`); edges and cycle checks (`CORE.DEP.EDGE`); input replacement (`CORE.DEP.REPL`); element computations (`KRN`, `ALN`, `GDT`, `SCAN`); job scheduling (`CORE.JOB.SCHED`); the handler-only bus path (`CORE.BUS.PIPE.STAGE`).

## Requirements

- **REQ-CORE.DEP.RECOMP-001** When a recompute runs, the dependency graph shall process the elements in scope in a topological order whose ties are broken by ascending stable entity ID.
- **REQ-CORE.DEP.RECOMP-002** When a recompute completes, the system shall append exactly one `project.recompute_dependents@1` entry listing every recomputed element with its new output digest and every failed element with its error code, in processing order.
- **REQ-CORE.DEP.RECOMP-003** If an element fails to recompute, then the dependency graph shall mark it `failed`, leave its transitive dependents `outdated` without running them, and continue with elements that do not depend on it.
- **REQ-CORE.DEP.RECOMP-004** If a recompute is cancelled, then the dependency graph shall discard all partial results, leave every element in its pre-recompute state, and append nothing to the Record Journal.
- **REQ-CORE.DEP.RECOMP-005** Recomputing the same graph with the same inputs and kernel shall produce output digests identical to the original computation for D0 and D1 elements.
- **REQ-CORE.DEP.RECOMP-006** If elements of one topological level are ever computed in parallel, then the committed entry shall be byte-identical to the entry of a sequential run.
- **REQ-CORE.DEP.RECOMP-007** The recompute shall run as a job that reports progress as elements processed over elements in scope and checks for cancellation between elements.
- **REQ-CORE.DEP.RECOMP-008** When a scope contains an element that is not `outdated` or `failed`, the recompute shall skip it and list it as `skipped` in the entry.
- **REQ-CORE.DEP.RECOMP-009** If the branch head moves while the recompute job runs, then the commit shall fail with `conflict`, and the elements shall remain `outdated`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/dep/recomp/diamond` (a → b, a → c, b → d, c → d; IDs ordered c < b; all `outdated` after a change of a), when `fid project recompute-dependents --all-outdated --json` runs, then the processing order is c, b, d, all three are `current`, and the Record Journal holds exactly one new `project.recompute_dependents@1` entry with three digests.
- **AC-2** Given fixture `fixtures/core/dep/recomp/fail_b` (as AC-1 but b's handler fails with `fit_failed`), when recomputed, then b is `failed`, d stays `outdated` and is not run, c is `current`, and the entry lists c with its digest and b with `fit_failed`.
- **AC-3** (error path) Given a 1,000-element chain fixture `fixtures/core/dep/recomp/chain1000` and cancellation after 100 elements, then every element keeps its pre-recompute state and output, and the Record Journal is unchanged.
- **AC-4** (replay) Given fixture `fixtures/core/dep/recomp/kernel_d0` (elements computed by D0 kernel commands), when recomputed without input change, then every new digest equals its original digest; when the session is replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-5** Given the chain fixture, when the job runs, then `fid job show-status` reports progress values that never decrease and end at 1.0.
- **AC-6** (error path) Given a W2 commit on the same branch while the recompute job runs, then the recompute commit fails with `conflict` and the elements stay `outdated`.

## Design

- **Registry command(s):** `project.recompute_dependents@1`, W2, D0, long-running, undo `head_move`; params `{ scope: { elements: [EntityRef] } | "all_outdated" | { subtree: EntityRef } }`; result `{ recomputed: [{ id, digest }], failed: [{ id, code }], skipped: [EntityId] }`.
- **Data structures:** `RecomputePlan { order: Vec<EntityId> }`; `ElementOutcome { Recomputed { digest, events } | Failed { code } | Blocked | Skipped }`; scratch overlay `StateView` that layers recomputed outputs over the head state.
- **Algorithm:** collect the scope; build the induced subgraph from the edge index; Kahn's algorithm with a `BTreeSet` ready-queue keyed by entity ID; for each element, if any input is `failed` or blocked mark it blocked (stays `outdated`), else call `Bus::run_handlers_as_system` with the element's recorded command and arguments against the overlay; on success add its events to the overlay; check cancellation and report progress between elements; at the end, emit one `RecomputeCommitted` event whose body lists outcomes, plus the state-change events, and commit them through the normal pipeline commit with `expected_head` = the head at start.
- **Errors:** `conflict`, `cancelled`, `validation` (empty or unknown scope).
- **Determinism:** D0 for the order and the entry; element outputs keep their own classes (REQ-005 applies to D0 and D1 elements).
- **Performance budget:** ordering 100,000 elements in < 100 ms, excluding element computation [unverified until measured].
- **Crate:** `fid-dep` (module `recompute`).

## Test fixtures

`fixtures/core/dep/recomp/` (`diamond`, `fail_b`, `chain1000`, `kernel_d0`): synthetic projects built by a committed generator with fixed seeds, using test element types whose handlers are pure functions; CC-BY-4.0.

## Build prompt

> In crate `fid-dep`, add module `recompute` and register `project.recompute_dependents@1` (W2, D0, long-running, undo head_move; CLI `fid project recompute-dependents`, MCP `project_recompute_dependents`; scope = element list, `all_outdated` or subtree). Order the scope topologically with Kahn's algorithm and a `BTreeSet` ready-queue (ascending entity ID). For each element: if an input failed, leave it `outdated` and do not run it; otherwise call `Bus::run_handlers_as_system(element's recorded command and args, overlay_view)` as principal `system` with `on_behalf_of` the caller, and layer its events into a scratch overlay. Failed elements become `failed` with their error code. Report progress (processed / total) and check the cancel token between elements; on cancel return nothing to commit. Commit one entry (outcomes in processing order plus state-change events) with `expected_head` = head at start, so a moved head gives `conflict`. Sequential in P0; any later parallelism must keep the entry byte-identical. Tests: AC-1..AC-6 with `fixtures/core/dep/recomp/`. Constraints: CONSTITUTION C1 (values of record only from the kernel), §4 (fixed-order processing, no std HashMap or HashSet in order-relevant code), §8 (no panic, no unsafe, deny unwrap/expect).

## Open questions

- **Resolved:** the handler-only path for system principals is `Bus::run_handlers_as_system`, offered by `CORE.BUS.PIPE.STAGE`.
- Parallel recompute of independent elements is deferred; it is allowed only if REQ-006 holds and KNP-1 fixed-order reductions inside each element still apply.

## Children

_Leaf: none._
