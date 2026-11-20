---
id: GEO.CAD.TOPO.TRIM
title: Trimmed-face point classification
level: 4
parent: GEO.CAD.TOPO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-cad]
built_pr: 295
satisfies: [REQ-GEO.CAD.TOPO-003]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.PCURVE, GEO.NURBS.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# GEO.CAD.TOPO.TRIM: Trimmed-face point classification

## Purpose

Decides whether a surface parameter (u, v) lies inside, outside or on the boundary of a trimmed face. Projection (GEO.DIST.FACE), recognition sampling (GEO.CAD.NOMINAL) and tessellation (GEO.CAD.TESS) all ask this question, and it feeds values of record, so it is exact in its decisions and stated in metres.

## Scope

**In:**
- `TrimIndex` per face: loop trimming curves (from GEO.CAD.TOPO.PCURVE) split into monotone segments with bounding boxes in the face chart
- `classify_uv(face, u, v, tol_m) -> TrimClass { Inside, Outside, OnBoundary }`
- Crossing-number test with a horizontal ray in the face chart, half-open segment rule, and periodic charts
- Boundary band: a point is `OnBoundary` when its surface image is within `tol_m` of the boundary's surface image
- Faces with no loops (the whole surface domain) and multiple inner loops

**Out** (and which node owns it instead):
- Producing pcurves (owned by `GEO.CAD.TOPO.PCURVE`)
- Reporting loop defects (owned by `GEO.CAD.TOPO.VALID`)

## Requirements

- **REQ-GEO.CAD.TOPO.TRIM-001** When asked to classify (u, v) on a face, the system shall return `Inside`, `Outside` or `OnBoundary` using the face's trimming curves in its parameter chart.
- **REQ-GEO.CAD.TOPO.TRIM-002** The system shall return `OnBoundary` exactly when the 3D distance from S(u, v) to the nearest boundary point, evaluated through the surface, is at most `tol_m`; for other points the result shall agree with the even-odd rule over all loops.
- **REQ-GEO.CAD.TOPO.TRIM-003** If `tol_m` is not finite or not positive, or (u, v) is outside the surface domain, then the system shall return `BadTolerance` or `OutsideDomain`.
- **REQ-GEO.CAD.TOPO.TRIM-004** Classification shall be deterministic class D0: the same face, point and tolerance give the same result on every platform and thread count.
- **REQ-GEO.CAD.TOPO.TRIM-005** A point whose u differs by a whole period on a periodic surface shall classify identically.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/topo/plate_with_hole.json` and 10 000 seeded (seed 11) uv points, when classified with `tol_m = 1e-7`, then every result matches the analytic answer (inside the square, outside the circle, band of 1e-7 m).
- **AC-2** Given fixture `fixtures/geo/cad/topo/cylinder_window.json` (a cylindrical face with a rectangular window crossing the seam), when points at u and u + 2π are classified, then results are equal and match the analytic answer.
- **AC-3** Given points exactly on loop vertices and on horizontal segments, then each returns `OnBoundary` and never an inconsistent `Inside`/`Outside`.
- **AC-4** (error path) Given `tol_m = 0`, then `BadTolerance`.
- **AC-5** Given AC-1 run on three OSes, then the result vectors are identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `TrimIndex { segments: Vec<TrimSeg { a: [f64; 2], b: [f64; 2], loop_ix, coedge_ix, bbox }>, chart: FaceChart }`; `TrimClass`.
- **Algorithm:** flatten each trimming curve to a uv polyline whose surface image is within tol/4 of the curve (imported pcurves are flattened by fixed bisection; derived ones are already polylines); boundary band check first (candidate segments by bbox inflated by a uv bound derived from the surface's maximum first-derivative norm over the segment box, then exact 3D point-to-segment-image distance by GEO.NURBS.EVAL at a fixed sample count, refined by bisection); otherwise crossing number with half-open rule (count a segment when exactly one endpoint has v ≥ query v).
- **Amendment (maintainer decisions on the review of PR 295, 2026-11-20).** These rules replace the parts of the Algorithm above that they cover. `crates/fid-geo-cad/src/topo/trim.rs` holds the full pinned text.
  - *Chart points* are exact `x + k·P`: binary64 `x` plus whole periods `k`. Periods are never added in floating point.
  - *Seams.* An edge used by exactly two coedges of the same face is a seam only if the face has a periodic direction and the two uses lie `k ≠ 0` periods apart. Otherwise both uses are ordinary boundary, as for a bridge or slit on a plane. If the two uses coincide (`k = 0`, for example a sphere's meridian seam), the second use is lifted one period to the side where the material lies: the left of the first use for a `Same` face, the right for `Reversed`. A connector next to a seam (a pole connector) is not boundary. On a sphere, vertices within 2⁻⁴⁰ of ±π/2 are put on the pole.
  - *Inside and outside on a face with exactly one periodic direction.* Cast the ray in the non-periodic direction and count the crossings above q of the lifted loop curves (all chart copies), with the half-open rule in the periodic direction. Material lies on the left of every loop (on the right for a `Reversed` face). The far end of the ray (band top or pole) is material when the topmost curve has material above it. A segment at the greatest height decides this directly; otherwise the turn at the topmost vertex does. q is inside when far end XOR odd count. A point exactly on a seam or pole connector is inside. This classifies winding loops (bands, caps), seam faces and closed faces.
  - *Other faces.* With no periodic direction, use the even-odd ray `+u`. With two periodic directions, use even-odd over chart copies; a winding loop or seam there is `Undecidable`.
  - *Undecidable.* A face whose pcurves failed, whose loop is incomplete, whose NURBS pcurve does not flatten, or which is otherwise undecidable returns a typed `Undecidable` error and never a guess.
  - *Flattening.* Imported NURBS pcurves are flattened at `tol_m / 4` for each query tolerance.
  - *Band.* The band is a branch and bound with per-surface Lipschitz bounds. Its guard is a stated forward-error bound on evaluation, distance, uv rounding and period drift. Every point within `tol_m` is `OnBoundary`, and the band is closed outward by at most `tol_m·2⁻¹³` plus the guard.
  - *Copy limit.* A periodic coordinate more than 2²⁰ periods from the chart is `OutsideDomain`.
- **Errors:** `BadTolerance`, `OutsideDomain`, `Undecidable`.
- **Determinism:** D0.
- **Performance budget:** median 2 µs per classification on a face with 200 boundary segments [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `plate_with_hole.json`, `cylinder_window.json`, `trim_polygons.json`, `sphere_seam_closed.json`, `sphere_cap.json` (with PCURVE's `cylinder_band.json` and `cylinder_capped.json`): analytic faces with exact-rational or closed-form answers from `make_trim_fixtures.py`. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, KNP-1, no I/O), add module `topo::trim` with `TrimIndex::build(&Body, face)` (using `topo::pcurve` trimming curves and charts) and `classify_uv(&TrimIndex, u, v, tol_m) -> Result<TrimClass, TrimError>`. `OnBoundary` iff the surface image of (u, v) is within `tol_m` of the boundary's surface image (compute with conservative uv bounds then exact evaluation); otherwise even-odd crossing number with a half-open rule; periodic u or v reduced into the face chart. Errors `BadTolerance`, `OutsideDomain`. Acceptance: AC-1 10 000 seeded points on a plate with a hole match analytic answers; AC-2 periodic equivalence across the seam; AC-3 vertex and horizontal-segment cases give `OnBoundary`; AC-4 `BadTolerance`; AC-5 identical results on three OSes. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no FMA, no HashMap, total-order min/max). Files: `crates/fid-geo-cad/src/topo/trim.rs`, `tests/topo_trim.rs`, `fixtures/geo/cad/topo/`.

## Open questions

- None; pcurves are primary (GEO.CAD.TOPO.PCURVE) and pcurve disagreement is reported by GEO.CAD.TOPO.VALID, as the parent's open question proposed.

## Children

_Leaf: none._
