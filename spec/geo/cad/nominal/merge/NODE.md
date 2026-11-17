---
id: GEO.CAD.NOMINAL.MERGE
title: Merging faces of one primitive
level: 4
parent: GEO.CAD.NOMINAL
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.NOMINAL-004, REQ-GEO.CAD.NOMINAL-006]
covers: []
depends_on: [GEO.CAD.NOMINAL.RECOG]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.NOMINAL.MERGE: Merging faces of one primitive

## Purpose

CAD kernels often split one cylinder into two half faces or one plane into several patches. Given recognised elements for a set of faces, this leaf groups those that describe the same primitive within tolerance and offers one merged element that references all its source faces, so a hole is one cylinder in the feature list.

## Scope

**In:**
- Compatibility test per kind: same kind and material; plane: normals parallel within angle tolerance and offset within length tolerance; cylinder and cone: axes collinear, radii (and half-angles) equal; sphere: centres and radii equal; torus: axis, centre and both radii equal; line and circle analogously
- Greedy grouping in ascending source face ID; each group's representative is its lowest-ID element
- Merged element: representative's parameters, union of extents, all source IDs ascending, maximum of member deviations
- Proposals only: the caller decides whether to merge

**Out** (and which node owns it instead):
- Recognition (owned by `GEO.CAD.NOMINAL.RECOG`)
- Creating document elements (owned by `GEO.CAD.NOMINAL.CMD`)

## Requirements

- **REQ-GEO.CAD.NOMINAL.MERGE-001** When two or more input elements are compatible within the length tolerance (default 1e-7 m [unverified]) and angle tolerance (default 1e-9 rad [unverified]), the system shall propose one merged element whose `sources` list every member face ID in ascending order.
- **REQ-GEO.CAD.NOMINAL.MERGE-002** The merged element shall take its parameters from the member with the lowest face ID, and its extent shall be the union of member extents.
- **REQ-GEO.CAD.NOMINAL.MERGE-003** Elements of different kind or different material side shall never be merged.
- **REQ-GEO.CAD.NOMINAL.MERGE-004** Grouping shall be a function of the input set only, independent of input order, deterministic class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/nominal/split_faces.json` (a hole split into two half-cylinder faces, a plane split into three patches and a coaxial boss of different radius), when merged, then two merged proposals result (the hole, the plane) and the boss stays single.
- **AC-2** Given the same input shuffled with seed 4, then proposals are identical.
- **AC-3** Given a hole and a boss of equal radius on one axis but opposite material, then they are not merged.
- **AC-4** Given two half-cylinders whose radii differ by 2e-7 m, then they are not merged at the default tolerance and are merged at 1e-6 m.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `MergeParams { len_tol_m, ang_tol_rad }`; `MergeProposal { element: NominalElement, members: Vec<TopologyId> }`.
- **Algorithm:** sort inputs by lowest source face ID; for each, test against existing group representatives in creation order and join the first compatible group, else start a new group.
- **Errors:** `BadParameter`.
- **Determinism:** D0.
- **Performance budget:** 10⁴ elements under 100 ms [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `split_faces.json`: hand-written body description with analytic split faces. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, KNP-1), add module `nominal::merge` with `propose_merges(&[NominalElement], MergeParams) -> Vec<MergeProposal>`. Sort inputs by lowest source face ID; greedily join the first group whose representative (lowest-ID member) is compatible (same kind and material; axes, normals, centres and radii within length tolerance 1e-7 m and angle tolerance 1e-9 rad by default); merged elements copy the representative's parameters, union extents, list sources ascending and take the maximum deviation. Acceptance: AC-1 hole and plane merged, boss single; AC-2 order independence; AC-3 no merge across material sides; AC-4 tolerance boundary. Constraints: CONSTITUTION §8, KNP-1. Files: `crates/fid-geo-cad/src/nominal/merge.rs`, `tests/nominal_merge.rs`, `fixtures/geo/cad/nominal/split_faces.json`.

## Open questions

- None.

## Children

_Leaf: none._
