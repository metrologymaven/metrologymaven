---
id: ALN.MODEL.HIER.CHAIN
title: Alignment parent chain, composition to root and invalidation
level: 4
parent: ALN.MODEL.HIER
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL.HIER-001, REQ-ALN.MODEL.HIER-002, REQ-ALN.MODEL.HIER-005, REQ-ALN.MODEL.HIER-007]
covers: [GOM.ALN.08]
depends_on: [ALN.MODEL.ELEM, GEO.FRAME.TREE, GEO.FRAME.XFORM, CORE.DEP.EDGE, CORE.DEP.PROP]
side_effect: W2
determinism: D0
cli: [fid align set-parent]
mcp: [align_set_parent]
journal: [align.set_parent@1]
built_in: [fid-align]
built_pr: 508
updated: 2026-11-22
---

# ALN.MODEL.HIER.CHAIN: Alignment parent chain, composition to root and invalidation

## Purpose

Makes alignments a chain: each alignment has at most one parent (another alignment or the root frame), its transform composes with its ancestors' to the root, and re-parenting or a parent change invalidates descendants before anything recomputes (GOM alignment hierarchy and chained alignments). Each alignment is mirrored as a frame in GEO's frame tree so composition uses GEO's one rule.

## Scope

**In:** `align.set_parent@1`; the `compose_to_root` function used by ACTIVE, QA.CMP and XFM.LEAP; mirroring alignments as GEO frames; cycle and depth checks for every command that sets a parent; descendant invalidation through CORE.DEP.
**Out:** the element and its edges (`ALN.MODEL.ELEM`); groups (`ALN.MODEL.HIER.GROUP`); named frames (`ALN.MODEL.HIER.FRAME`); deletion (`ALN.MODEL.HIER.DEL`); composition arithmetic and the frame tree (`GEO.FRAME`).

## Requirements

- **REQ-ALN.MODEL.HIER.CHAIN-001** Every alignment shall have a mirror frame in GEO.FRAME.TREE whose parent is its parent alignment's mirror frame (or the root frame) and whose transform is the alignment's transform.
- **REQ-ALN.MODEL.HIER.CHAIN-002** `compose_to_root(alignment)` shall return GEO.FRAME.TREE's `resolve(mirror, root)`, and composing it with its inverse shall map every point within 10 m of the origin back within 1e-12 m [unverified].
- **REQ-ALN.MODEL.HIER.CHAIN-003** If `align.set_parent` (or any command that sets a parent) would make an alignment its own ancestor, then it shall fail with `hierarchy_cycle` naming the cycle path and journal nothing.
- **REQ-ALN.MODEL.HIER.CHAIN-004** If the resulting chain from an alignment to the root would exceed 64 alignments, then the command shall fail with `hierarchy_too_deep` stating the depth and journal nothing [unverified: limit chosen for bounded composition].
- **REQ-ALN.MODEL.HIER.CHAIN-005** When `align.set_parent` commits, the system shall replace the alignment's parent input edge in CORE.DEP and, in the same commit, mark the alignment and every transitive dependent `outdated` before any recompute.
- **REQ-ALN.MODEL.HIER.CHAIN-006** When an alignment's transform changes for any reason, its mirror frame shall be updated in the same commit, and every descendant alignment and dependent result shall be marked `outdated` through CORE.DEP.
- **REQ-ALN.MODEL.HIER.CHAIN-007** `align.set_parent` with `root` shall make the alignment a top-level alignment.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/model/hier/chain3.fidproj` (A under root, B under A, C under B, known transforms), when `compose_to_root(C)` is called, then it equals T_A·T_B·T_C within 1e-12 m on the fixture's test points.
- **AC-2** (error path) Given the same project, when `fid align set-parent --alignment A --parent C --json` runs, then it fails with `hierarchy_cycle{[A, C, B, A]}` and journals nothing.
- **AC-3** Given a chain of 64 alignments, when a 65th is parented to the deepest, then it fails with `hierarchy_too_deep`.
- **AC-4** Given `chain3`, when `fid align set-parent --alignment C --parent A` commits, then C and its dependents are marked `outdated` in that commit, and one journal event is written.
- **AC-5** (replay) Exact replay of AC-4 gives `IDENTICAL`.

## Design

- **Registry command:** `align.set_parent@1` (W2). Params: `alignment: AlignmentId`, `parent: AlignmentId | root`. Result: `{ alignment, parent, marked_outdated: u64 }`.
- **Functions:** `compose_to_root(doc, id) -> Result<Transform, HierError>`; `check_parent(doc, id, parent) -> Result<(), HierError>` (cycle and depth), called by every parent-setting command.
- **Data structures:** `MirrorFrame { alignment: AlignmentId, frame: FrameId }` map kept in a BTreeMap.
- **Errors:** `hierarchy_cycle{path}`, `hierarchy_too_deep{depth}`, `not_found`.
- **Determinism:** D0; composition order is GEO.FRAME.TREE's fixed order.
- **Performance budget:** `compose_to_root` at depth 64 in under 20 µs [unverified until measured].
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`chain3.fidproj` and a generated 64-deep chain (seeded transforms, CC0).

## Build prompt

> In crate `fid-align`, mirror every alignment as a frame in GEO's frame tree (`fid-geo-frame`, GEO.FRAME.TREE) whose parent is the parent alignment's mirror frame or the root, and implement `compose_to_root(doc, id)` via the tree's `resolve`. Implement `check_parent` (cycle → `hierarchy_cycle{path}`, depth over 64 → `hierarchy_too_deep`) used by every parent-setting command. Register `align.set_parent@1` (W2; CLI `fid align set-parent`; MCP `align_set_parent`; journal `align.set_parent@1`), params alignment and `parent: AlignmentId|root`: replace the parent edge in CORE.DEP (`fid-dep` edges) and mark the alignment and all transitive dependents `outdated` in the same commit (CORE.DEP.PROP). Keep mirror frames in step with transform changes. Errors journal nothing. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet` in anything digested.

## Open questions

- None.

## Children

_Leaf: none._
