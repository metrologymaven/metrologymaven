---
id: GEO.CAD.ASSY.FLAT
title: Assembly tree flattening and placement checks
level: 4
parent: GEO.CAD.ASSY
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.ASSY-001, REQ-GEO.CAD.ASSY-003]
covers: []
depends_on: [GEO.CAD.ASSY.MODEL, GEO.FRAME.XFORM, GEO.FRAME.TREE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.ASSY.FLAT: Assembly tree flattening and placement checks

## Purpose

Turns an assembly tree, as an importer delivers it, into a flat list of placed instances: one per occurrence, each with the placement composed along its path, and with every placement checked to be rigid or a uniform similarity. It produces a plan (frames to create and instances to register) that the import or `geo.place_instance@1` command commits.

## Scope

**In:**
- `AssemblyTree`: nodes that are either a sub-assembly or a body definition, edges carrying a local placement (4×4 matrix or rotation plus translation plus scale) and an occurrence label
- Depth-first flattening in occurrence-label order, composing placements root to leaf
- Placement classification: `Rigid`, `Similarity { scale }`, or rejection with a reason
- Cycle and size limits
- Output `FlattenPlan { frames: Vec<FrameSpec>, instances: Vec<Instance>, rejected: Vec<Rejected> }`

**Out** (and which node owns it instead):
- Reading assembly structure from STEP (owned by `IO.CAD.STEP`)
- Committing frames and instances (owned by `GEO.CAD.ASSY.CMD` and the importer, through `GEO.FRAME.CMD`)

## Requirements

- **REQ-GEO.CAD.ASSY.FLAT-001** When a tree is flattened, the system shall produce exactly one instance per path from the root to a body occurrence, whose frame transform equals the left-to-right composition of the local placements along that path.
- **REQ-GEO.CAD.ASSY.FLAT-002** If a composed placement has a rotation part that is not orthonormal within 1e-12, a negative determinant, non-uniform scale or a non-finite entry, then the system shall put the occurrence in `rejected` as `unsupported_placement` with reason `NotOrthonormal`, `Improper`, `NonUniformScale` or `NonFinite`, and not place it.
- **REQ-GEO.CAD.ASSY.FLAT-003** If the tree contains a cycle, exceeds depth 64 or yields more than 10⁵ occurrences, then flattening shall fail with `AssemblyCycle`, `TooDeep` or `TooManyOccurrences` [unverified limits].
- **REQ-GEO.CAD.ASSY.FLAT-004** The plan shall list frames and instances in depth-first order with children visited in ascending occurrence-label order, independent of input order.
- **REQ-GEO.CAD.ASSY.FLAT-005** Composition shall follow KNP-1 (no FMA, fixed-order products) so the same tree gives bit-identical transforms on every platform.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/assy/three_level.json` (a root with two sub-assemblies, one of them placed twice, five body occurrences in all), when flattened, then five instances are produced and each frame transform matches the hand-computed composition within 1e-15 relative.
- **AC-2** Given fixture `fixtures/geo/cad/assy/mirrored.json` (one occurrence with a mirror placement and one with scale (1, 1, 2)), when flattened, then both are rejected with `Improper` and `NonUniformScale` and the others are placed.
- **AC-3** (error path) Given a tree where a sub-assembly contains itself, then `AssemblyCycle`.
- **AC-4** Given AC-1 with nodes shuffled (seed 9), then the plan is byte-identical.
- **AC-5** Given AC-1 on three OSes, then all transforms are bit-identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `AssemblyTree { nodes: Vec<AsmNode>, root }`, `AsmNode { Assembly { children: Vec<Occurrence> }, Body { body: BodyId } }`, `Occurrence { label: TopologyId, placement: Placement, target: NodeIx }`, `FrameSpec { parent: FrameRefOrNew, transform: Transform }`, `Rejected { path, reason }`.
- **Algorithm:** iterative depth-first walk with an explicit stack and visited-path check; compose with GEO.FRAME.XFORM products; classify by computing RᵀR − I, determinant and per-axis scale.
- **Errors:** `AssemblyCycle`, `TooDeep`, `TooManyOccurrences`, `EmptyTree`.
- **Determinism:** D0.
- **Performance budget:** flattening 10⁵ occurrences under 500 ms [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `three_level.json`, `mirrored.json`, a cyclic tree: hand-written JSON with exact placements (multiples of 90° and binary-exact translations). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, no I/O, KNP-1), add module `assy::flat` with `flatten(&AssemblyTree) -> Result<FlattenPlan, FlatError>`. Walk depth-first with an explicit stack, children in ascending occurrence-label order, composing local placements root to leaf with `fid-geo-frame` transform products; create one `Instance` per body occurrence (IDs per `assy::model`) and one `FrameSpec` per instance. Classify each composed placement: orthonormal within 1e-12, determinant > 0 and uniform scale, else put it in `rejected` as `unsupported_placement` with reason `NotOrthonormal | Improper | NonUniformScale | NonFinite`. Fail with `AssemblyCycle`, `TooDeep` (> 64) or `TooManyOccurrences` (> 10⁵). Acceptance: AC-1 five instances with exact compositions; AC-2 mirror and non-uniform scale rejected; AC-3 cycle; AC-4 input-order independence; AC-5 bit-identical on three OSes. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, no recursion on untrusted depth), KNP-1. Files: `crates/fid-geo-cad/src/assy/flat.rs`, `tests/assy_flat.rs`, `fixtures/geo/cad/assy/`.

## Open questions

- Mirrored instances are rejected because GEO.FRAME accepts only proper transforms; supporting them would need an improper transform kind in GEO.FRAME.XFORM and orientation flipping in every consumer (inherited from the parent's open question).

## Children

_Leaf: none._
