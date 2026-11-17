---
id: ALN.MODEL.HIER.DEL
title: Alignment deletion with dependent handling
level: 4
parent: ALN.MODEL.HIER
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL.HIER-006]
covers: []
depends_on: [ALN.MODEL.HIER.CHAIN, ALN.MODEL.HIER.GROUP, ALN.MODEL.ELEM, CORE.DEP.EDGE, GEO.FRAME.TREE]
side_effect: W2
determinism: D0
cli: [fid align delete-alignment]
mcp: [align_delete_alignment]
journal: [align.delete_alignment@1]
updated: 2026-11-17
---

# ALN.MODEL.HIER.DEL: Alignment deletion with dependent handling

## Purpose

Deletes an alignment safely: by default it refuses while anything depends on it, and on request it either re-parents the children (keeping their composed transforms) or cascades the deletion to every dependent, always as one journaled commit.

## Scope

**In:** `align.delete_alignment@1` with modes `reject` (default), `re_parent` and `cascade`; dependent discovery through CORE.DEP; groups and frames that reference the alignment; mirror-frame removal.
**Out:** the chain and mirror frames (`ALN.MODEL.HIER.CHAIN`); group membership edits (`ALN.MODEL.HIER.GROUP`); generic frame deletion (`GEO.FRAME.CMD`).

## Requirements

- **REQ-ALN.MODEL.HIER.DEL-001** In mode `reject`, if the alignment has child alignments, nested frames, groups naming it or dependent results, then the command shall fail with `has_dependents` listing them (up to 100, with the total count) and journal nothing; otherwise it shall delete the alignment and its mirror frame.
- **REQ-ALN.MODEL.HIER.DEL-002** In mode `re_parent`, each child alignment and nested frame shall move to the deleted alignment's parent with its stored transform replaced by T_deleted · T_child, so that its composed-to-root transform is unchanged within 1e-12 m on points within 10 m [unverified].
- **REQ-ALN.MODEL.HIER.DEL-003** In mode `re_parent`, groups naming the deleted alignment shall have their alignment cleared, and results that depended directly on it shall be marked `outdated`.
- **REQ-ALN.MODEL.HIER.DEL-004** In mode `cascade`, the system shall delete the alignment and every transitive dependent (alignments, nested frames and results) and clear groups' references, in one commit whose journal event lists every deleted ID in sorted order.
- **REQ-ALN.MODEL.HIER.DEL-005** Frames built `from_alignment` (snapshots) shall not count as dependents and shall keep their recorded source digest.
- **REQ-ALN.MODEL.HIER.DEL-006** If the alignment does not exist, then the command shall fail with `not_found` and journal nothing.

## Acceptance criteria

- **AC-1** Given `chain3.fidproj`, when `fid align delete-alignment --alignment B --json` runs, then it fails with `has_dependents` listing C, and journals nothing.
- **AC-2** Given the same, with `--mode re_parent`, then B is deleted, C's parent is A, and `compose_to_root(C)` is unchanged within 1e-12 m.
- **AC-3** Given the same, with `--mode cascade`, then B, C and results evaluated in them are deleted in one event listing the sorted IDs.
- **AC-4** Given a snapshot frame from B, then it survives AC-3 with its digest intact.
- **AC-5** (replay) Exact replay of AC-2 and AC-3 gives `IDENTICAL`.

## Design

- **Registry command:** `align.delete_alignment@1` (W2). Params: `alignment`, `mode: reject | re_parent | cascade` (default `reject`). Result: `{ deleted: [EntityId], re_parented: [EntityId], marked_outdated: u64 }`.
- **Algorithm:** collect dependents via CORE.DEP reverse edges (sorted); apply mode; remove mirror frame via GEO.FRAME.TREE after children moved.
- **Errors:** `has_dependents{ids, total}`, `not_found`.
- **Determinism:** D0.
- **Performance budget:** cascade of 10,000 dependents in under 100 ms [unverified until measured].
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`chain3.fidproj` plus results and a snapshot frame (CC0).

## Build prompt

> In crate `fid-align`, register `align.delete_alignment@1` (W2; CLI `fid align delete-alignment`; MCP `align_delete_alignment`; journal `align.delete_alignment@1`), modes `reject` (default), `re_parent`, `cascade`. Find dependents through CORE.DEP reverse edges (sorted). `reject`: fail `has_dependents` (up to 100 IDs plus total) if any child alignment, nested frame, group reference or dependent result exists. `re_parent`: move children and nested frames to the deleted alignment's parent with stored transform T_deleted·T_child (composed transform unchanged), clear group references, mark direct dependents `outdated`. `cascade`: delete all transitive dependents and clear group references in one commit listing sorted IDs. Snapshot frames are not dependents. Remove the mirror frame (ALN.MODEL.HIER.CHAIN) last. Errors journal nothing. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- After `re_parent`, a child's next recompute re-runs its solver from the new parent; its composed transform is kept only until then. This matches the parent requirement's wording.

## Children

_Leaf: none._
