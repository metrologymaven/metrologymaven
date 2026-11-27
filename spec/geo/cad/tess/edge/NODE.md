---
id: GEO.CAD.TESS.EDGE
title: Shared edge discretisation
level: 4
parent: GEO.CAD.TESS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-cad]
satisfies: [REQ-GEO.CAD.TESS-001, REQ-GEO.CAD.TESS-002, REQ-GEO.CAD.TESS-003]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.ADJ, GEO.CAD.TOPO.PCURVE, GEO.NURBS.EVAL]
uses_artifacts: [GEO.CAD.TESS.SPIKE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TESS.EDGE: Shared edge discretisation

## Purpose

Watertightness starts at the edges: each edge is sampled exactly once, under the chordal and angular tolerances, and both faces that meet at it use the same sample points (the same f64 values and the same mesh vertex indices), mapped into each face's parameter space through its coedge. Faces are then meshed against these fixed boundaries.

## Scope

**In:**
- Per-edge parameter samples on the 3D edge curve meeting chordal tolerance t and angular tolerance a
- One shared vertex per body vertex; one shared vertex per interior edge sample
- Mapping of each sample to uv for every coedge of the edge (imported pcurve evaluated at the matched parameter, or the derived pcurve from GEO.CAD.TOPO.PCURVE), in the face's unwrapped chart
- Seam edges: one sample list used twice in one face, at both chart sides
- `EdgeSamples { edge, t: Vec<f64>, vertex_ix: Vec<u64> }` and `CoedgeUv { coedge, uv: Vec<[f64; 2]> }`

**Out** (and which node owns it instead):
- Meshing face interiors (owned by `GEO.CAD.TESS.FACE`)
- Refinement inside faces (owned by `GEO.CAD.TESS.REFINE`)

## Requirements

- **REQ-GEO.CAD.TESS.EDGE-001** For every edge, the chord between consecutive samples shall deviate from the edge curve by at most t, and the angle between curve tangents at consecutive samples shall be at most a, checked at the chord midpoint and quarter points [unverified sufficiency of three checks].
- **REQ-GEO.CAD.TESS.EDGE-002** Every coedge of an edge shall reference the same vertex indices in the same order (reversed for a reversed coedge), so adjacent faces share every boundary vertex.
- **REQ-GEO.CAD.TESS.EDGE-003** Edge sample vertices shall lie on the 3D edge curve within 1e-12 m; their distance to each adjacent surface shall be reported per edge as `max_surface_gap_m`.
- **REQ-GEO.CAD.TESS.EDGE-004** Sampling shall be deterministic class D0: recursive bisection in fixed left-to-right order with a depth limit of 24, independent of the order in which edges are processed.
- **REQ-GEO.CAD.TESS.EDGE-005** If an edge cannot meet the tolerances within the depth limit, then the system shall report `EdgeTolerance { edge, achieved_m }` and keep the samples reached, so the faces on both sides still share them.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/tess/analytic_set` at t = 1e-5 m, a = 0.2 rad, when edges are sampled, then a dense check (1 000 points per segment) confirms chordal ≤ t and tangent angle ≤ a on every segment.
- **AC-2** Given the cube and cylinder-with-hole bodies, then for every edge both coedges reference identical vertex index lists (one reversed).
- **AC-3** Given a circle edge of radius r, then the number of samples equals the closed-form minimum for the chordal tolerance rounded up to the next bisection level.
- **AC-4** Given edges processed in reverse order, then sample lists are bit-identical.
- **AC-5** Given an edge 1e-6 m off its surface, then `max_surface_gap_m` ≈ 1e-6 is reported.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** as in Scope; `EdgeTessParams { chordal_m, angular_rad, max_depth }`.
- **Algorithm:** start from the edge's parameter range with its two vertices; bisect a segment while the midpoint or quarter-point chord error exceeds t or the tangent angle exceeds a; vertex indices assigned in ascending edge-ID order, then sample order; uv mapping by evaluating the coedge's trim curve at the sample (imported pcurve: parameter matched by GEO.CAD.TOPO.PCURVE's correspondence; derived: interpolation along the stored `t`).
- **Errors:** `EdgeTolerance` (reported, not fatal).
- **Determinism:** D0.
- **Performance budget:** 10⁴ edges at t = 1e-5 m under 200 ms [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies; buildable for wasm32).

## Test fixtures

- `fixtures/geo/cad/tess/analytic_set`: cube, cylinder with a through hole, sphere, torus, as GEO.CAD.TOPO.MODEL JSON descriptions; plus an off-surface edge variant. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, no I/O, KNP-1, wasm32-buildable), add module `tess::edge`. Read the spike report `docs/spikes/geo-cad-tess-base.md` first and follow its recommendation on reusing algorithms. For each edge in ascending ID order, sample the 3D edge curve by fixed left-to-right bisection (depth ≤ 24) until chord error at midpoint and quarter points ≤ chordal tolerance and tangent angle ≤ angular tolerance; assign one shared vertex index per body vertex and edge sample; map samples into uv for each coedge via `topo::pcurve` trim curves (reversed order for reversed coedges, both chart sides for seams); report `max_surface_gap_m` per edge and `EdgeTolerance` when the limit is reached. Acceptance: AC-1 dense tolerance check; AC-2 identical index lists per edge; AC-3 circle sample count; AC-4 order independence; AC-5 surface gap reported. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no FMA, no HashMap, fixed order). Files: `crates/fid-geo-cad/src/tess/edge.rs`, `tests/tess_edge.rs`, `fixtures/geo/cad/tess/analytic_set/`.

## Open questions

- REQ-GEO.CAD.TESS-001 asks every mesh vertex to lie on its source surface within 1e-9 m. Edge vertices lie on the edge curve, which real STEP data places only within the model tolerance of each surface (often around 1e-7 m [unverified]); this leaf therefore reports the gap instead of promising 1e-9 m on edges. The parent requirement should be read as "within max(1e-9 m, the body's edge-to-surface gap)".

## Children

_Leaf: none._
