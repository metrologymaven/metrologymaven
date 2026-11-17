---
id: GEO.DIST.FACE.ONE
title: Closest point on a single trimmed face
level: 4
parent: GEO.DIST.FACE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.DIST.FACE-001, REQ-GEO.DIST.FACE-002, REQ-GEO.DIST.FACE-005, REQ-GEO.DIST.FACE-006]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.TRIM, GEO.CAD.TOPO.PCURVE, GEO.NURBS.INVERT, GEO.NURBS.ANALYTIC, GEO.NURBS.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.DIST.FACE.ONE: Closest point on a single trimmed face

## Purpose

The exact closest point from a query point to one trimmed CAD face. The closest point is either an interior critical point of the distance on the surface that lies inside the trimmed region, or a point on the face boundary (an edge or a vertex). This leaf enumerates both kinds of candidate on the exact surface and edge curves, never the tessellation, and returns the nearest with its parameters and the boundary feature it lies on.

## Scope

**In:**
- Interior candidates: for analytic surfaces, all critical points in closed form (plane 1, cylinder and sphere 2, cone and torus up to 4); for NURBS, one local minimum per Bézier patch whose lower bound beats the current best (GEO.NURBS.INVERT candidates)
- Trim filter: keep interior candidates classified `Inside` or `OnBoundary` by GEO.CAD.TOPO.TRIM at the face's topology tolerance
- Boundary candidates: closest point on each boundary edge curve of the face (curve inversion) and on each boundary vertex
- `FaceHit { foot, distance, face, uv, feature: Interior | Edge(TopologyId) | Vertex(TopologyId) }` or `NoHit` under a maximum distance
- Tie rules: lower distance, then interior before edge before vertex, then lowest entity ID

**Out** (and which node owns it instead):
- Choosing among faces of a body (owned by `GEO.DIST.FACE.BODY`)
- Sign and incident normals (owned by `GEO.DIST.FACE.INCID`)
- Single-surface inversion internals (owned by `GEO.NURBS.INVERT` and `GEO.NURBS.ANALYTIC`)

## Requirements

- **REQ-GEO.DIST.FACE.ONE-001** For each query point, the system shall return the minimum-distance point over the interior critical points inside the trimmed region and the closest points on all boundary edges and vertices of the face.
- **REQ-GEO.DIST.FACE.ONE-002** The foot point shall lie on the exact surface within 1e-9 m [unverified] and be classified `Inside` or `OnBoundary` at the topology tolerance; an `Edge` foot shall lie on its edge curve within 1e-9 m.
- **REQ-GEO.DIST.FACE.ONE-003** For NURBS faces the system shall consider one candidate per Bézier patch whose box lower bound is below the current best distance, so a trimmed-away global minimum cannot hide a closer interior minimum inside the trim.
- **REQ-GEO.DIST.FACE.ONE-004** When a maximum distance d is given and no candidate is within d, the system shall return `NoHit`.
- **REQ-GEO.DIST.FACE.ONE-005** The computation shall read only exact surface and curve geometry and trimming data, never a tessellation.
- **REQ-GEO.DIST.FACE.ONE-006** The computation shall be deterministic class D0 under KNP-1, with candidates visited in a fixed order.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/dist/face/ref_v1` (GEO.DIST.FACE.REF), when every single-face query is run, then foot points match the oracle within 1e-9 m and features match exactly.
- **AC-2** Given fixture `fixtures/geo/dist/face/annulus_trap.json` (a planar annulus with the query above the hole's centre), then the foot is on the inner circle edge, not the trimmed-away plane point.
- **AC-3** Given fixture `fixtures/geo/dist/face/saddle_two_minima.json` (a NURBS face whose global surface minimum is trimmed away and a second local minimum is inside), then the inside minimum is returned.
- **AC-4** Given `max_distance` smaller than the true distance, then `NoHit`.
- **AC-5** Given AC-1 on three OSes, then results are bit-identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `FaceHit`, `FaceFeature`, `FaceQueryOptions { max_distance, topo_tol_m }`.
- **Algorithm:** analytic: closed-form critical points via GEO.NURBS.ANALYTIC; NURBS: Bézier patches ordered by (lower bound, patch index), Newton per patch via GEO.NURBS.INVERT's per-patch entry point; trim filter by `topo::trim`; boundary: curve inversion on each boundary edge (3D curve) in ascending edge ID order, then vertices; minimum with the tie rules.
- **Errors:** `NonFiniteQuery`, `NonConvergence { best }` (returned with the best candidate found).
- **Determinism:** D0.
- **Performance budget:** median 20 µs per query on a trimmed bicubic face with 4 boundary edges [unverified].
- **Crate:** `fid-geo-dist` (L0, kernel graph, KNP-1 applies).

## Test fixtures

- `ref_v1` (from GEO.DIST.FACE.REF), `annulus_trap.json`, `saddle_two_minima.json` (hand-written with oracle answers). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-dist` (L0, no I/O, KNP-1), add module `face::one` with `project_face(&Body, face, p, FaceQueryOptions) -> Result<FaceProjection, FaceError>` where `FaceProjection = Hit(FaceHit { foot, distance, face, uv, feature }) | NoHit`. Candidates: all closed-form critical points for analytic surfaces (`fid-geo-nurbs` analytic), or one Newton minimum per Bézier patch whose box lower bound beats the current best (GEO.NURBS.INVERT per-patch entry point); keep those `fid-geo-cad::topo::trim` classifies `Inside`/`OnBoundary`; add closest points on each boundary edge curve (ascending edge ID) and each vertex. Choose by distance, then Interior < Edge < Vertex, then lowest ID. Never use tessellation. Acceptance: AC-1 oracle agreement within 1e-9 m; AC-2 annulus; AC-3 hidden interior minimum; AC-4 `NoHit`; AC-5 bit-identical on three OSes. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no FMA, fixed order, total-order min). Files: `crates/fid-geo-dist/src/face/one.rs`, `tests/face_one.rs`, `fixtures/geo/dist/face/`.

## Open questions

- REQ-003 needs GEO.NURBS.INVERT to expose a per-Bézier-patch Newton entry point (its design already decomposes into patches); if it only returns the global result, this leaf must re-implement the patch loop. Raised for GEO.NURBS.INVERT.

## Children

_Leaf: none._
