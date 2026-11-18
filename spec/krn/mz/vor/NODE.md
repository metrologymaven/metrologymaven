---
id: KRN.MZ.VOR
title: Exact Voronoi-based minimum-zone 2D circle
level: 3
parent: KRN.MZ
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ-006]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.NUM.RED, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.MZ.VOR: Exact Voronoi-based minimum-zone 2D circle

## Purpose

A second, independent path for the minimum-zone 2D circle. The optimal centre lies at a vertex of the nearest-point Voronoi diagram, a vertex of the farthest-point Voronoi diagram, or an intersection of an edge of each. Enumerating those candidates gives the global optimum combinatorially, with no solver iterates, so it checks the conic path in CI. The Voronoi primitives are also reused by `KRN.ENV` for the maximum inscribed circle.

## Scope

**In:** deterministic nearest-point and farthest-point Voronoi diagrams of planar points; candidate-centre enumeration; zone evaluation per candidate; global minimum selection with a total-order tie-break; the algorithm `fit.circle2d.mz_vor@1.0.0`; a brute-force oracle for small inputs (test-only).
**Out:** the conic minimum-zone path and the CI agreement job (`KRN.MZ.RND`); maximum inscribed circle (`KRN.ENV`), which reuses the diagrams.

## Requirements

- **REQ-KRN.MZ.VOR-001** The module shall build the nearest-point Voronoi diagram (by deterministic incremental Delaunay triangulation with insertion in input order after a fixed lexicographic sort) and the farthest-point Voronoi diagram of the convex-hull points.
- **REQ-KRN.MZ.VOR-002** The module shall evaluate the zone width (maximum minus minimum distance) at every nearest-diagram vertex, every farthest-diagram vertex and every intersection of a nearest-diagram edge with a farthest-diagram edge, and return the minimum.
- **REQ-KRN.MZ.VOR-003** Ties between candidates within 0 m difference shall be broken by total order on the centre coordinates, and the result shall list the critical points (two on each boundary or three on one and one on the other).
- **REQ-KRN.MZ.VOR-004** Geometric predicates (orientation, in-circle) shall use an adaptive-precision evaluation that returns the exact sign, built only from f64 operations without FMA.
- **REQ-KRN.MZ.VOR-005** If fewer than 4 distinct points are given or all points are collinear, then the module shall return a typed `FitError`.
- **REQ-KRN.MZ.VOR-006** For inputs up to 40 points, the result shall equal the brute-force optimum over all candidate centres defined by point quadruples and triples within 1e-12 m [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/vor-small-v1` (500 seeded sets of 5 to 40 points near a circle, including tied and cocircular cases), when the module and the test-only brute-force oracle run, then zone widths agree within 1e-12 m and the selected critical sets are identical.
- **AC-2** Given the PTB TraCIM Chebyshev 2D circle sets in `rdl/tracim-cheb-v0`, when the module runs, then each zone width and centre is within 1/10 of the TraCIM default MPE [unverified].
- **AC-3** Given AC-1 inputs on x86-64, ARM64 and wasm32 under wasmtime, then output digests are identical.
- **AC-4** (error path) Given 100 collinear points, then the module returns `FitError::Collinear`.
- **AC-5** Given a set of 2 000 points, when run, then it completes in under 200 ms on reference hardware [unverified].

## Design

- **Registry command(s):** none; kernel algorithm `fit.circle2d.mz_vor@1.0.0`, registered with a spec card and used as a cross-check. Library API `fid_fit::vor::{nearest_voronoi, farthest_voronoi, mz_circle2d}`.
- **Data structures:** half-edge Delaunay triangulation in vectors indexed by u32 (no hash maps); `VoronoiDiagram { vertices, edges (finite and rays) }`.
- **Algorithm:** Bowyer-Watson incremental Delaunay with a super-triangle; farthest-point Delaunay on hull points (Andrew's monotone chain hull, then the farthest-point triangulation); candidates per requirement 002. Reference: Roy and Zhang 1992, "Establishment of a pair of concentric circles with the minimum radial separation for assessing roundness error" [unverified citation details]. Predicates per Shewchuk 1997, "Adaptive precision floating-point arithmetic and fast robust geometric predicates", written without FMA.
- **Errors:** `FitError::{TooFewPoints, Collinear, NonFinite}`.
- **Determinism:** D0.
- **Performance budget:** as AC-5 [unverified until measured].
- **Crate:** `fid-fit` (module `vor`).

## Test fixtures

`fixtures/fit/vor-small-v1`: seeded generator (committed), CC-BY-4.0. TraCIM Chebyshev 2D circle sets from `UNC.RDL` at CI time (licence [unverified]).

## Build prompt

> In crate `fid-fit`, create module `vor`. Implement exact-sign orientation and in-circle predicates (adaptive precision expansions from f64 two-sum and two-product built with Dekker splitting; no `mul_add`), a deterministic Bowyer-Watson Delaunay triangulation (sort points lexicographically by total order, then insert in that order; half-edge structure in `Vec`s indexed by u32; no HashMap), the nearest-point Voronoi diagram as its dual, the convex hull (monotone chain) and the farthest-point Voronoi diagram of hull points. Implement `mz_circle2d(&PointsView /*already planar*/) -> Result<MzCircle, FitError>`: evaluate zone width at every nearest-diagram vertex, every farthest-diagram vertex and every nearest-edge x farthest-edge intersection; choose the minimum, ties by total order on the centre; return centre, inner and outer radius, width and the critical points. Register algorithm ID `fit.circle2d.mz_vor@1.0.0` with a spec card. Add a test-only brute-force oracle over point triples and quadruples for n <= 40. Tests: `fixtures/fit/vor-small-v1` vs oracle within 1e-12 m; TraCIM Chebyshev 2D circle sets from the RDL at CI time; cross-target digest identity on x86-64/ARM64/wasm32; collinear error; 2 000 points under 200 ms. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max` (use `fid_num::f::total_*`), no HashMap/HashSet, no recursion without a fixed bound, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.MZ.VOR.

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** move the predicate code into KRN.NUM.PRED and depend on it.
- None.

## Children

_Leaf: none._
