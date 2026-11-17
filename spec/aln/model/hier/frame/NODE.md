---
id: ALN.MODEL.HIER.FRAME
title: Named frames nested under alignments and frames from alignments
level: 4
parent: ALN.MODEL.HIER
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL.HIER-002, REQ-ALN.MODEL.HIER-004]
covers: [SA.XFM.02]
depends_on: [ALN.MODEL.HIER.CHAIN, ALN.MODEL.ELEM, GEO.FRAME.TREE, GEO.FRAME.XFORM]
side_effect: W2
determinism: D0
cli: [fid align create-frame]
mcp: [align_create_frame]
journal: [align.create_frame@1]
updated: 2026-11-17
---

# ALN.MODEL.HIER.FRAME: Named frames nested under alignments and frames from alignments

## Purpose

SpatialAnalyzer-style named frames on the alignment side: create a named frame nested under another frame or under an alignment, or build a frame whose transform is a snapshot of an alignment's composed transform at creation time, recording the source alignment's digest. Plain frame creation in GEO's tree remains `geo.create_frame@1`.

## Scope

**In:** `align.create_frame@1` with modes `nested` (parent is a frame or an alignment's mirror frame, given local transform) and `from_alignment` (snapshot); unique frame names; recording of the source digest; cycle and depth checks through `ALN.MODEL.HIER.CHAIN`.
**Out:** generic frame commands (`GEO.FRAME.CMD`); frames built from geometry by 3-2-1 (`ALN.RPS.CLASSIC`); frame deletion when its parent alignment is deleted (`ALN.MODEL.HIER.DEL`).

## Requirements

- **REQ-ALN.MODEL.HIER.FRAME-001** In mode `nested`, `align.create_frame` shall insert a named frame in GEO.FRAME.TREE under the given frame or the given alignment's mirror frame, with the given local transform.
- **REQ-ALN.MODEL.HIER.FRAME-002** In mode `from_alignment`, the new frame's transform relative to its parent (root by default) shall equal the source alignment's `compose_to_root` transform at creation (expressed relative to that parent), and the frame shall record the source alignment's ID and content digest.
- **REQ-ALN.MODEL.HIER.FRAME-003** A frame created `from_alignment` shall be a snapshot: later changes to the source alignment shall not change it, and the frame shall report `source_changed: true` when the source's current digest differs from the recorded one.
- **REQ-ALN.MODEL.HIER.FRAME-004** If the name already exists among frames, then the command shall fail with `frame_name_exists`; if the parent does not exist, with `not_found`; if insertion would create a cycle, with `hierarchy_cycle`; each journals nothing.
- **REQ-ALN.MODEL.HIER.FRAME-005** Lengths in the local transform authored in non-SI units shall keep their authored value and unit, with one journaled conversion to metres.

## Acceptance criteria

- **AC-1** Given `chain3.fidproj`, when `fid align create-frame --name F1 --mode from_alignment --source B --json` runs, then F1's transform from root equals `compose_to_root(B)` within 1e-12 m on the test points and F1 records B's digest.
- **AC-2** Given F1, when B is recomputed to a new transform, then F1 is unchanged and reports `source_changed: true`.
- **AC-3** Given `--mode nested --parent-alignment A --translate 10,0,0 mm`, then the frame sits 0.01 m along A's X and the conversion is journaled.
- **AC-4** (error path) Given an existing name, then the command fails with `frame_name_exists` and journals nothing.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command:** `align.create_frame@1` (W2). Params: `name`, `mode: nested { parent: FrameId | AlignmentId, local: TransformSpec } | from_alignment { source: AlignmentId, parent?: FrameId }`. Result: `{ frame: FrameId }`.
- **Data structures:** frame metadata `{ name, source: Option<{ alignment, digest }> }` stored with the GEO frame.
- **Errors:** `frame_name_exists`, `not_found`, `hierarchy_cycle`, `hierarchy_too_deep`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`chain3.fidproj` from ALN.MODEL.HIER.CHAIN (CC0).

## Build prompt

> In crate `fid-align`, register `align.create_frame@1` (W2; CLI `fid align create-frame`; MCP `align_create_frame`; journal `align.create_frame@1`). Mode `nested`: insert a named frame into GEO's frame tree (GEO.FRAME.TREE) under a frame or an alignment's mirror frame with a local transform (lengths converted once to metres and journaled). Mode `from_alignment`: snapshot `compose_to_root(source)` (ALN.MODEL.HIER.CHAIN) relative to the chosen parent (root default), store source ID and digest, and report `source_changed` when the source digest later differs. Use `check_parent` for cycles and depth. Errors (`frame_name_exists`, `not_found`, `hierarchy_cycle`) journal nothing. Tests: AC-1..AC-5. Constraints: CONSTITUTION §3, §4, §8 (no `unwrap`/`expect`/`panic`/`unsafe`).

## Open questions

- Snapshot semantics follow the parent's open question; a live frame is modelled as a child alignment instead.

## Children

_Leaf: none._
