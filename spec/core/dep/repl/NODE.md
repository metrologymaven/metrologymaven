---
id: CORE.DEP.REPL
title: Input replacement
level: 3
parent: CORE.DEP
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.DEP-007]
covers: [GOM.PRJ.07]
depends_on: [CORE.DEP.EDGE, CORE.DEP.PROP, CORE.DEP.RECOMP, CORE.PRJ.TREE, CORE.BUS]
side_effect: W2
determinism: D0
cli: [fid project replace-input]
mcp: [project_replace_input]
journal: [project.replace_input@1]
updated: 2026-11-16
---

# CORE.DEP.REPL: Input replacement

## Purpose

"Replace the mesh and everything updates." REPL swaps the content of an input element (a measured mesh, a CAD body, a referenced element) for the content of a newly imported element of the same type, keeps the target's ID and the IDs of all its dependents, and lets propagation and recompute bring every dependent up to date against the new input.

## Scope

**In:** the `project.replace_input@1` command, type compatibility checks, consuming the staged replacement element, interaction with marking and recompute, undo behaviour.
**Out:** importing the new data (`IO`); marking (`CORE.DEP.PROP`); recompute (`CORE.DEP.RECOMP`).

## Requirements

- **REQ-CORE.DEP.REPL-001** When `project.replace_input@1` runs with a target element and a replacement element, the system shall give the target the replacement's content while keeping the target's ID, name, group, tags and metadata.
- **REQ-CORE.DEP.REPL-002** If the replacement's registered type differs from the target's, then the command shall fail with a typed `type_mismatch` error and commit nothing.
- **REQ-CORE.DEP.REPL-003** When the replacement completes, the system shall remove the replacement element from the tree in the same commit.
- **REQ-CORE.DEP.REPL-004** When the replacement commits, every transitive dependent of the target shall keep its ID and be marked `outdated` in the same commit.
- **REQ-CORE.DEP.REPL-005** While the recompute mode is `auto`, after the replacement commits, the dependents shall be recomputed against the new content in one separate recompute entry.
- **REQ-CORE.DEP.REPL-006** When the replacement is undone, the target shall regain its previous content and its dependents shall again be marked `outdated`.
- **REQ-CORE.DEP.REPL-007** If the replacement element has dependents of its own, then the command shall fail with a typed `replacement_in_use` error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/dep/replace_chain` (test-kit elements: input `m` with value 10, `s1 = m + 1`, `s2 = s1 * 2`; staged replacement `m'` with value 20) in auto mode, when `fid project replace-input --target m --replacement m' --json` runs, then `m` keeps its ID, holds 20, `m'` is gone, and after the recompute job `s1 = 21` and `s2 = 42` with unchanged IDs.
- **AC-2** Given the journal after AC-1, then it holds exactly one `project.replace_input@1` entry followed by exactly one `project.recompute_dependents@1` entry listing `s1` and `s2`.
- **AC-3** (error path) Given a replacement of a different test-kit type, then the command fails with `type_mismatch` and journals nothing.
- **AC-4** Given AC-1 then `fid journal undo` twice (recompute and replace), then `m` holds 10 again and `s1`, `s2` return to 11 and 22 after recompute.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-6** (error path) Given a replacement element that some other element already depends on, then the command fails with `replacement_in_use`.

## Design

- **Registry command(s):** `project.replace_input@1` W2, D0, undo `head_move`; params `{ target: EntityRef, replacement: EntityRef }`; result `{ target, content_digest, marked: [EntityId] }`.
- **Data structures:** event `InputReplaced { target, old_content_digest, new_content_digest, replacement_removed }`.
- **Algorithm:** validate types and replacement usage; emit `InputReplaced` and an element-removed event; the PROP hook marks dependents; in auto mode PROP submits the recompute.
- **Errors:** `type_mismatch`, `replacement_in_use`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** the replace commit itself < 50 ms excluding recompute [unverified until measured].
- **Crate:** `fid-dep` (module `replace`).

## Test fixtures

Generated test-kit project `fixtures/core/dep/replace_chain` with a staged replacement element; CC-BY-4.0.

## Build prompt

> In crate `fid-dep`, add module `replace` registering `project.replace_input@1` (W2, D0, undo head_move; params target and replacement entity refs). Check both exist and share a registered element type (`type_mismatch` otherwise) and that the replacement has no dependents (`replacement_in_use`). Emit `InputReplaced { target, old_content_digest, new_content_digest }` plus removal of the replacement element, keeping the target's ID, name, group, tags and metadata. Rely on the existing propagation hook to mark dependents outdated in the same commit and, in auto mode, to submit one recompute. CLI `fid project replace-input`, MCP `project_replace_input`. Tests: AC-1..AC-6 with `fixtures/core/dep/replace_chain` and a test-kit element type. Constraints: CONSTITUTION §8, handlers return events only, ordered collections.

## Open questions

- Large inputs (50 GB scans) are replaced by digest reference, not by copying; this assumes the replacement's content is already in the blob store from its import.

## Children

_Leaf: none._
