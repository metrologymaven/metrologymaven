---
id: GEO.CAD.TESS.REFINE
title: Tolerance-driven refinement of face meshes
level: 4
parent: GEO.CAD.TESS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TESS-001, REQ-GEO.CAD.TESS-003]
covers: []
depends_on: [GEO.CAD.TESS.FACE, GEO.NURBS.EVAL]
uses_artifacts: [GEO.CAD.TESS.SPIKE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TESS.REFINE: Tolerance-driven refinement of face meshes

## Purpose

Refines a face's initial mesh until every triangle is within the chordal tolerance of the surface and adjacent triangle normals stay within the angular tolerance of the surface normals, by inserting interior points chosen in a fixed order. Boundary segments are never split, because the neighbouring face shares them.

## Scope

**In:**
- Per-triangle error estimate: maximum distance between the flat triangle and the surface at a fixed sample pattern (centroid and three edge-midpoints in uv, plus seven interior barycentric points) and maximum angle between the triangle normal and the surface normals at its vertices
- Refinement by inserting the uv point of the worst sample into the constrained Delaunay mesh
- A priority order (error descending, then triangle creation index ascending) that is independent of threads
- Limits: maximum triangles per face and per body
- Final per-face check that reports the achieved chordal and angular errors

**Out** (and which node owns it instead):
- Initial meshing and predicates (owned by `GEO.CAD.TESS.FACE`)
- Body assembly and parallelism (owned by `GEO.CAD.TESS.BODY`)

## Requirements

- **REQ-GEO.CAD.TESS.REFINE-001** When refinement finishes for a face, every triangle's measured chordal deviation at the sample pattern shall be at most t, unless the face is reported with `FaceTolerance`.
- **REQ-GEO.CAD.TESS.REFINE-002** When refinement finishes, the angle between each triangle's normal and the surface normal at each of its vertices shall be at most a, unless reported with `FaceTolerance`.
- **REQ-GEO.CAD.TESS.REFINE-003** Inserted vertices shall be exact surface evaluations, and no boundary segment shall be split.
- **REQ-GEO.CAD.TESS.REFINE-004** If a face needs more than 2 × 10⁶ triangles [unverified] or a boundary-adjacent triangle cannot meet t without splitting a boundary segment, then the system shall stop refining that face and report `FaceTolerance { face, achieved_chordal_m, achieved_angular_rad }`.
- **REQ-GEO.CAD.TESS.REFINE-005** Refinement shall be deterministic class D0: the insertion order is fully determined by the error values and creation indices.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/tess/analytic_set` at t = 1e-5 m and a = 0.2 rad, when refined, then an independent dense check (100 points per triangle against the analytic surface) finds chordal error ≤ t × 1.05 [unverified margin for the sample pattern] and normal angles ≤ a.
- **AC-2** Given the sphere at t = 1e-5 m and at t = 1e-6 m, then the triangle count grows by a factor between 5 and 15 [unverified, roughly 1/t for chordal control].
- **AC-3** Given a face whose boundary edge was sampled coarser than t (`EdgeTolerance`), then `FaceTolerance` is reported and boundary segments are unchanged.
- **AC-4** Given AC-1 on three OSes, then refined meshes are bit-identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `RefineParams { chordal_m, angular_rad, max_tris_face }`; a binary heap keyed by (error bits descending, creation index ascending), implemented over `Vec` with a total-order comparison.
- **Algorithm:** evaluate the sample pattern per triangle; pop the worst; insert its worst sample's uv point (Delaunay insertion with flips, constrained edges preserved); re-evaluate affected triangles; stop when the heap's worst is within tolerance or limits are hit.
- **Errors:** `FaceTolerance` (reported per face).
- **Determinism:** D0.
- **Performance budget:** a 10⁶-triangle body at t = 1e-5 m under 5 s on 8 threads, combined with GEO.CAD.TESS.BODY [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies; buildable for wasm32).

## Test fixtures

- `fixtures/geo/cad/tess/analytic_set` with analytic reference distances. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, no I/O, KNP-1, wasm32-buildable), add module `tess::refine` with `refine_face(&mut FaceMesh, &Body, RefineParams) -> Result<RefineReport, FaceTolerance>`. Read `docs/spikes/geo-cad-tess-base.md` first. Estimate each triangle's chordal error at a fixed sample pattern (centroid, three uv edge-midpoints, seven barycentric points) and the angle between its normal and the surface normals at its vertices; repeatedly take the worst triangle (error descending, creation index ascending, total-order compare) and insert its worst sample point using `tess::face` Delaunay insertion without splitting constrained boundary segments; inserted vertices are exact surface evaluations. Stop at tolerance or report `FaceTolerance` (over 2 × 10⁶ triangles, or unreachable without splitting boundaries). Acceptance: AC-1 dense check; AC-2 triangle growth with tighter t; AC-3 boundary-limited face reported; AC-4 bit-identical on three OSes. Constraints: CONSTITUTION §8, KNP-1 (no FMA, no HashMap, `fid-num` sqrt and acos). Files: `crates/fid-geo-cad/src/tess/refine.rs`, `tests/tess_refine.rs`.

## Open questions

- The sample pattern bounds the chordal error only approximately for high-curvature NURBS; a curvature-based a priori bound could replace it if AC-1's margin fails [unverified].

## Children

_Leaf: none._
