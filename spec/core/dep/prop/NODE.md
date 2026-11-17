---
id: CORE.DEP.PROP
title: Change propagation and recompute mode
level: 3
parent: CORE.DEP
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.DEP-003, REQ-CORE.DEP-008, REQ-CORE.DEP-011]
covers: []
depends_on: [CORE.DEP.EDGE, CORE.PRJ.TREE, CORE.BUS]
side_effect: W2
determinism: D0
cli: [fid project set-recompute-mode]
mcp: [project_set_recompute_mode]
journal: [project.set_recompute_mode@1]
updated: 2026-11-16
---

# CORE.DEP.PROP: Change propagation and recompute mode

## Purpose

When an element changes, marks every transitive dependent `outdated` in the same commit, before any recompute starts, so the document never shows a stale value as current. It also owns the project's recompute mode: in `auto` mode a change triggers a recompute job; in `manual` mode only marking happens until the user asks for a recompute.

## Scope

**In:** transitive marking, state transition rules for marking, the `project.set_recompute_mode@1` command and the mode value, triggering a recompute submission in auto mode.
**Out:** the recompute itself (`CORE.DEP.RECOMP`); edges (`CORE.DEP.EDGE`); element states storage (`CORE.PRJ.TREE`).

## Requirements

- **REQ-CORE.DEP.PROP-001** When a commit changes an element, the dependency graph shall mark, in the same commit, every transitive dependent in state `current` or `failed` as `outdated`.
- **REQ-CORE.DEP.PROP-002** When marking, the dependency graph shall leave elements in state `not_computed` unchanged.
- **REQ-CORE.DEP.PROP-003** When `project.set_recompute_mode@1` runs, the system shall set the project's recompute mode to `auto` or `manual`; a new project's mode shall be `auto`.
- **REQ-CORE.DEP.PROP-004** While the recompute mode is `manual`, the dependency graph shall only mark dependents and shall not start a recompute.
- **REQ-CORE.DEP.PROP-005** While the recompute mode is `auto`, when a commit marks one or more elements `outdated`, the dependency graph shall submit one recompute of exactly those elements after the commit completes.
- **REQ-CORE.DEP.PROP-006** Marking over a graph of 100,000 elements shall complete in under 100 ms on reference hardware [unverified until measured].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/dep/diamond` (a → b, a → c, b → d, c → d; all `current`) in manual mode, when `a` is updated, then b, c and d are `outdated` in the same journal entry as the update, and no recompute job exists.
- **AC-2** Given the same fixture in auto mode, when `a` is updated, then exactly one recompute job for {b, c, d} is submitted after the commit.
- **AC-3** Given `d` in state `not_computed`, when `a` is updated, then `d` stays `not_computed`.
- **AC-4** Given `fixtures/core/dep/wide_100k` with one root feeding all elements, when the root is updated in manual mode, then marking completes in under 100 ms in the benchmark.
- **AC-5** Given `fid project set-recompute-mode manual --json`, then one `project.set_recompute_mode@1` entry is journaled, and `fid journal undo` restores `auto`.
- **AC-6** (error path) Given `fid project set-recompute-mode sometimes`, then the command fails with `validation` and journals nothing.

## Design

- **Registry command(s):** `project.set_recompute_mode@1` W2, D0, undo `head_move`; params `{ mode: "auto" | "manual" }`; result `{ mode }`.
- **Data structures:** `RecomputeMode` stored as project document state; event `ElementsMarked { elements: [EntityId] }` added by the commit hook.
- **Algorithm:** after EDGE's hook, breadth-first traversal of the reverse index from each changed element in ID order; collect dependents, filter by state, emit `ElementsMarked`; in auto mode, post-commit submit `project.recompute_dependents@1` with the marked set as the system principal on behalf of the committer.
- **Errors:** `validation`.
- **Determinism:** D0.
- **Performance budget:** REQ-006.
- **Crate:** `fid-dep` (module `prop`).

## Test fixtures

Generated test-kit projects `fixtures/core/dep/diamond` and `wide_100k` (shared with CORE.DEP.EDGE); CC-BY-4.0.

## Build prompt

> In crate `fid-dep`, add module `prop`. Add a commit hook that runs after the edge hook: for each element changed by the commit, traverse the reverse dependency index (breadth-first, ID order) and mark every transitive dependent in state `current` or `failed` as `outdated` (leave `not_computed` alone), emitting one `ElementsMarked` event in the same commit. Register `project.set_recompute_mode@1` (W2, D0, undo head_move; `auto|manual`, default `auto`). In auto mode, after the commit completes, submit `project.recompute_dependents@1` for exactly the marked set, principal `system` on behalf of the committer; in manual mode submit nothing. CLI `fid project set-recompute-mode`, MCP `project_set_recompute_mode`. Tests: AC-1..AC-6 with fixtures under `fixtures/core/dep/`. Constraints: CONSTITUTION §8, ordered collections, no recursion over the graph.

## Open questions

- Whether auto mode should debounce rapid successive edits into one recompute; this leaf submits one recompute per marking commit and leaves coalescing to the job scheduler.

## Children

_Leaf: none._
