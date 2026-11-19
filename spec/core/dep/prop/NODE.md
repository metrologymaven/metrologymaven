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
built_in: [fid-dep]
built_pr: 243
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

Pinned at build time (`fid-dep::prop`):

- **Graph:** propagation reads any `DepGraph` (direct dependents and direct inputs as ascending sets). `EdgeIndex` (CORE.DEP.EDGE) implements it; so does `Adjacency`, a plain graph with no acyclicity invariant, which stands for graphs read from elsewhere and is what the cycle guard is tested on.
- **States:** `ElementState` is `current | outdated | failed | not_computed` (text forms exactly these); `ElementStates` is an ordered map from entity ID to state. Until CORE.PRJ.TREE.MODEL lands, the state type lives here; marking uses only its allowed transitions current -> outdated and failed -> outdated.
- **Pass input:** `changed` (the elements the commit changed; at most 4096, the edge module's per-commit limit) and `recomputed` (the elements the commit recomputed against values of this same pass; an ordinary edit passes none, a recompute commit lists what it recomputed). Both must be distinct (`duplicate_element`) and known (`not_found/unknown_element`); each recomputed element must be in the region (`not_in_pass`).
- **Region:** breadth-first from the changed elements (ascending) over dependents (ascending): the changed elements plus all their transitive dependents. Traversal does not stop at `not_computed` or `outdated` elements, so their dependents are reached too.
- **Order:** Kahn's algorithm over the region with an ascending-ID ready queue: topological, ties broken by entity ID. No recursion anywhere.
- **Cycles:** elements Kahn's algorithm cannot order sit on or behind a cycle. The pass is refused with `cycle`; the reported cycle is found by walking from the smallest unordered element to its smallest unordered input until an element repeats, reversed into edge direction and rotated to start at its smallest ID. Nothing changes.
- **Staleness (never fresh on stale inputs):** in propagation order, an element with an input in the region is *downstream*. A downstream element is *pending* unless it is in `recomputed`, none of its inputs in the region is pending and every input outside the region is `current` (an input with no state is `not_found/missing_state`). Anything downstream of a pending element is therefore pending. A changed element with no input in the region is the commit's own new value and is never pending; a changed element downstream of another changed element is treated like any other downstream element. Propagation never sets any state other than `outdated`.
- **Marking (REQ-001, REQ-002):** every pending element in state `current` or `failed` is marked; `outdated` and `not_computed` elements stay as they are (a pending element without a state is `not_found/missing_state`).
- **Event:** journal event type `dep.elements_marked@1`, payload `{"elements": [id, ...]}`, non-empty and strictly ascending, at most 2^20 elements; emitted only when something is marked, in the same commit as the change. Decoding refuses any other key, a missing key, a non-canonical ID, an empty, repeated or unsorted list (`bad_event`) and an oversize list (`input_limit`). Applying it (also on replay) is all or nothing: every element must be `current` or `failed`, else `invalid_transition` (or `not_found/missing_state`) and no state changes.
- **Submission (REQ-004, REQ-005):** in `auto` mode with at least one marked element, one `project.recompute_dependents@1` with params `{"scope": {"elements": [marked ids, ascending]}}`, to be submitted after the commit completes, as principal `system` on behalf of the committer (the bus integration sets the principal). In `manual` mode, or with nothing marked, nothing is submitted. Already `outdated` and `not_computed` elements are not resubmitted (RECOMP's `all_outdated` scope covers them).
- **Atomicity:** `plan` reads only; `propagate` plans, then applies the event; on any error states are unchanged and there is no event and no submission.
- **Mode (REQ-003):** `RecomputeMode` is `auto | manual`, default `auto`. `project.set_recompute_mode@1` params `{ "mode": "auto" | "manual" }`; any other key, a missing or non-string mode, or any other value (including other spellings) is `validation`, and nothing is journaled. Each successful call journals one event `project.recompute_mode_set@1` `{"mode": ...}` (also when the mode does not change) and returns `{"mode": ...}`. The mode at a head is the last such event before it, else `auto`; undo by `head_move` is replay to the earlier head. The registry definition (W2, D0, natural idempotency, undo `head_move`, effects `[project.recompute_mode_set@1]`, CLI and MCP) is built through `CommandDef::from_value`.
- **Bounds:** at most 4096 changed elements, 2^20 elements in the region or in `recomputed`, 2^24 edges followed; exceeding any is `input_limit`.
- **Errors:** closed set with stable `code()`/`reason()`: `cycle`, `not_found` (`unknown_element`, `missing_state`), `duplicate_element`, `not_in_pass`, `invalid_transition`, `validation`, `bad_event`, `input_limit`, `internal`. Errors are checked in a fixed order (limits, duplicates, unknown, cycle, not in pass, missing state) over ascending IDs, so the reported error does not depend on list order.
- **Integration left to the bus:** the bus wiring (running the hook after the edge hook inside its commit, the principal of the submission, CLI `fid project set-recompute-mode` and MCP `project_set_recompute_mode` front ends) is not in this crate, as for CORE.DEP.EDGE.
- **Fixtures:** `fixtures/core/dep/diamond/project.txt` (AC-1 to AC-3; IDs with c < b) and `fixtures/core/dep/prop/cases.txt` (20 cases) are hand-labelled; `make_prop_fixtures.py --check` re-derives every case with a stdlib reference and writes `wide_100k/prop_expected.txt` for AC-4 from the CORE.DEP.EDGE recipe (element 0 reaches all 100,000 elements).
- **Benchmark (REQ-006):** `cargo test --release -p fid-dep --test prop -- --ignored ac4_benchmark` asserts the 100 ms budget. Ordinary test runs check the AC-4 result and hold the time only to a regression guard, because CI builds tests unoptimised.

## Test fixtures

Generated test-kit projects `fixtures/core/dep/diamond` and `wide_100k` (shared with CORE.DEP.EDGE); CC-BY-4.0.

## Build prompt

> In crate `fid-dep`, add module `prop`. Add a commit hook that runs after the edge hook: for each element changed by the commit, traverse the reverse dependency index (breadth-first, ID order) and mark every transitive dependent in state `current` or `failed` as `outdated` (leave `not_computed` alone), emitting one `ElementsMarked` event in the same commit. Register `project.set_recompute_mode@1` (W2, D0, undo head_move; `auto|manual`, default `auto`). In auto mode, after the commit completes, submit `project.recompute_dependents@1` for exactly the marked set, principal `system` on behalf of the committer; in manual mode submit nothing. CLI `fid project set-recompute-mode`, MCP `project_set_recompute_mode`. Tests: AC-1..AC-6 with fixtures under `fixtures/core/dep/`. Constraints: CONSTITUTION §8, ordered collections, no recursion over the graph.

## Open questions

- Whether auto mode should debounce rapid successive edits into one recompute; this leaf submits one recompute per marking commit and leaves coalescing to the job scheduler.

## Children

_Leaf: none._
