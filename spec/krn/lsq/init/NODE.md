---
id: KRN.LSQ.INIT
title: Initial estimates for axial primitives
level: 3
parent: KRN.LSQ
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-fit]
built_pr: 358
satisfies: [REQ-KRN.LSQ-003, REQ-KRN.LSQ-008]
covers: []
depends_on: [KRN.LSQ.TRAIT, KRN.SOLV.MSTART, KRN.NUM.LA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# KRN.LSQ.INIT: Initial estimates for axial primitives

## Purpose

Cylinder, cone and torus fits are non-convex in the axis direction and converge to the wrong solution from a poor start, especially on partial features. This module produces deterministic primary starts and the multi-start specification (start count, perturbation box, seed) that `KRN.SOLV.MSTART` runs, so every axial fit starts the same way on every host.

## Scope

**In:** primary start heuristics for cylinder, cone and torus on centred, scaled data; a fixed set of candidate axis directions; the `StartSpec` built for `KRN.SOLV.MSTART`; use of point normals when present.
**Out:** running the fits and the agreement check (`KRN.LSQ.AXIS`, `KRN.SOLV.MSTART`); circle and sphere starts, which use an algebraic fit inside `KRN.LSQ.RND`.

## Requirements

- **REQ-KRN.LSQ.INIT-001** For a cylinder, the module shall evaluate a fixed candidate set of axis directions (the three principal directions of the centred data plus 13 fixed hemisphere directions), fit a 2D circle algebraically to the points projected along each, rank the candidates by algebraic residual (ties broken by candidate index), refine each of the best three by a pinned pattern search on the same residual, and take the lowest refined residual (ties to the better-ranked candidate) as the primary start.
- **REQ-KRN.LSQ.INIT-002** When unit normals are supplied, the module shall add the direction minimising the sum of squared projections of the normals (the smallest-eigenvalue eigenvector of their scatter matrix) as the first candidate.
- **REQ-KRN.LSQ.INIT-003** For a cone, the module shall rank the cylinder candidate set by an algebraic cone residual, refine the best three and select the axis as in REQ-001 (a circle whose radius varies linearly with the axial coordinate) and estimate apex and half-angle by a linear fit of the projected radius against the axial coordinate.
- **REQ-KRN.LSQ.INIT-004** For a torus, the module shall score the plane-fit normal of the centred data, the other principal directions and the 13 fixed hemisphere directions by a fixed-axis torus fit, refine the best three by a pinned pattern search on that score, and take the centre, major radius and minor radius from the fixed-axis fit at the best refined axis.
- **REQ-KRN.LSQ.INIT-005** The module shall return a `StartSpec` with the primary start, a perturbation box (axis angle 15 degrees [unverified], position 10 % of the data RMS radius [unverified]), a start count (default 8) and the caller's seed, all echoed for journaling.
- **REQ-KRN.LSQ.INIT-006** If fewer points are given than the primitive needs, or every candidate is degenerate, then the module shall return the typed `FitError` from `KRN.LSQ.TRAIT`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/init-axial-v1` (seeded synthetic cylinders, cones and tori, noise 1 micrometre, angular coverage 360, 180 and 90 degrees, 3 seeds each), when the module runs, then the primary-start axis is within 10 degrees [unverified] of the true axis for every case with coverage of at least 180 degrees, and at least one start in the `StartSpec` box is within 10 degrees for 90-degree coverage.
- **AC-2** Given the same fixture on x86-64, ARM64 and wasm32 under wasmtime, then every returned `StartSpec` is bit-identical and its digest matches the golden digest.
- **AC-3** Given a cylinder fixture with normals, then the normal-derived direction is the first candidate and the primary-start axis error is below 1 degree.
- **AC-4** (error path) Given 4 points for a cylinder, then the module returns `FitError::TooFewPoints{n: 4}`.

## Design

- **Registry command(s):** none; library API `fid_fit::init::{cylinder_start, cone_start, torus_start}(&PointsView, seed) -> Result<AxialStart, FitError>`; `AxialStart { primitive, frame: AxisFrame { point, axis, e1, e2 }, spec: StartSpec, candidates, selected, rms_radius, apex }`. The `StartSpec` parameters are relative to the frame (`AxisFrame::line`): cylinder `[a, b, u, v, r]`, cone `[a, b, u, v, r_ref, half_angle]`, torus `[a, b, u, v, R, r]`, axis `unit(axis + a e1 + b e2)` through `point + u e1 + v e2` (the two tangent-plane parameters of `KRN.LSQ.AXIS`); the frame point is the axis point at the data's mid-height (foot of the perpendicular from the centroid; for the torus the tube-centre plane).
- **Data structures:** the 13 hemisphere directions are a fixed const table (the 13 directions of a 3 x 3 x 3 lattice up to sign, normalised); `StartSpec` from `fid-solv`.
- **Algorithm:** algebraic circle fit (Kasa, blocked QR fold then `fid_num::la::lstsq`) on projections; principal directions and the normals' smallest-eigenvalue direction from the blocked-QR and Jacobi rules of `contract::data_shape` (`fid_num::la` has no `sym_eig`); the primary is refined by a pattern search (tangent steps 2^-3 to 2^-10, at most 16 moves each); cone axis scored by the linear algebraic cone `u^2 + v^2 + D u + E v + F + G t + H t^2 = 0`, then the radius line by `lstsq`; torus axis scored by a fixed-axis Gauss-Newton fit of `(rho - R)^2 + (h - h0)^2 - r^2` from three start scales (1, 2, 4 times the algebraic radius) on the algebraic centre's side of the data and the same three mirrored to the other side; for every primitive the best three candidates are refined and the lowest refined score wins. Half widths 15 degrees (`tan(pi/12)`) for `a, b`, 10 % of the RMS radius for `u, v`, 0 otherwise; count 8; streams 1, 2, 3; agreement tolerances `1e-6` (angles) and `1e-6 * rms` (lengths) [unverified].
- **Errors:** `FitError::{TooFewPoints, Collinear, Coplanar, IllConditioned}` (`IllConditioned { cond: None }` when every candidate is degenerate or the cone radius at mid-height is not positive).
- **Determinism:** D0.
- **Performance budget:** start generation for 100 000 points < 15 ms [unverified until measured]. **Not met by any primitive as built**, the cylinder and cone included: every candidate score and every refinement move passes over all points. The review of #358 counted about 80 (cylinder), 210 (cone) and 6 100 (torus) fit-row evaluations per point for the first version (the most 100, 370 and 11 300); refining the best three and the mirrored torus starts raise that to about 220, 620 and 18 500 (the most 270, 1 110 and 40 900) on the fixture [unverified estimate, counted on a numpy re-implementation]. At 100 000 points that is about 2 x 10^7 row evaluations for a cylinder, 6 x 10^7 for a cone and 2 x 10^9 for a torus, against a budget of about one row evaluation per point, so all three miss 15 ms by two to four orders of magnitude. On the fixture's 200-point cases, a local debug test run of all 59 cases takes about 3 minutes, nearly all of it in the 15 tori. Follow-up: score candidates on a deterministic subsample (for example a fixed stride over the point order, pinned with the algorithm version) and use all points only for the final refinement.
- **Supported torus shapes:** minor radius 0.03 to 0.99 major radii (ring tori; horn and spindle tori are out of scope) with at least 90 degrees of the major angle and the tube fully covered. A numpy re-implementation sweep (1 micrometre noise) gave every primary within 1 degree over that range at 90, 180 and 360 degrees [unverified beyond it]. Below 90 degrees fat tori degrade: at 45 degrees and minor radius 0.4 to 0.8 major radii, 8 of 20 sweep cases were 1 to 31 degrees off.
- **Crate:** `fid-fit` (module `init`).

## Test fixtures

`fixtures/fit/init-axial-v1`: seeded synthetic generator for partial cylinders, cones and tori (generator code and seeds committed), CC-BY-4.0. As built: `fixtures/fit/make_init_fixtures.py` (stdlib, 60-digit Decimal, no libm) plants 27 cases (3 primitives x coverage 360/180/90 x 3 seeds, axes up to 0.3 m and 1 m from the origin), 3 cylinders with normals, a near-planar ring 0.3 m from the origin, 19 half cylinders whose principal axes mix (9 seeds at length 1.066 radii and a sweep from 1.00 to 1.12 radii), 6 fat quarter tori (minor radius 0.45, 0.5 and 0.8 major radii) and 3 error cases (4 points, exactly coplanar, exactly collinear). It records the true axis, axis point (torus centre), radius, cone half-angle and torus minor radius; the tests assert the primary axis within 1 degree and the radii, position, half-angle and apex within stated tolerances for every start case.

## Build prompt

> In crate `fid-fit`, create module `init` with `cylinder_start`, `cone_start`, `torus_start(&PointsView, seed: u64) -> Result<StartSpec, FitError>`. Work on centred and scaled data (module `contract`). Cylinder: candidates = normal-derived direction if normals given (smallest-eigenvalue eigenvector of the normals' scatter), then the three principal directions, then 13 fixed hemisphere directions; for each, project points onto the orthogonal plane and fit a circle algebraically by linear least squares (`fid_num::la::lstsq`); primary = lowest algebraic residual, ties by candidate index. Cone: cylinder axis, then linear fit of projected radius versus axial coordinate for apex and half-angle. Torus: plane fit for the axis, algebraic circle in that plane for centre and major radius, RMS distance for minor radius. Return `fid_solv::mstart::StartSpec` with primary, perturbation box (15 degrees axis, 10 % of RMS radius position), count 8, the given seed. Tests: `fixtures/fit/init-axial-v1` (axis within 10 degrees for coverage >= 180 degrees), cross-target bit identity on x86-64/ARM64/wasm32, normals case, too-few-points error. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.INIT.

## Open questions

- The perturbation box and start count are placeholders; tune them on ORS partial-feature cases from `UNC` before freezing the algorithm version.
- **Performance follow-up:** deterministic subsampling of the candidate scores (see Performance budget), so the budget can be met; it changes the output bits and needs a new algorithm version.
- **Torus below 90 degrees of the major angle:** fat tori can get a start several degrees off (see Supported torus shapes). A guard on the fixed-axis fit's residual would need a noise floor the module does not know; left open.
- **As built (#358):** the leaf's own cylinder rule (16 fixed candidates) left a full cylinder 15 degrees off when its principal axes were mixed (length near sqrt(6) radii), the cylinder axis left cones 40 degrees off, and the plane fit left a fat quarter torus 30 degrees off, so REQ-001, -003 and -004 now include a pinned refinement and a cone and torus score (REQ-001, -003 and -004 amended). On the fixture every primary is within 0.3 degrees, 90-degree coverage included. The review of #358 found that refining only the best candidate left some half cylinders (180 degrees, length near 1.066 radii, where `L^2 / 12` equals the transverse variance of a half circle) about 90 degrees off, and that fat quarter tori (minor radius 0.45 to 0.8 major radii) were 14 to 89 degrees off because the algebraic circle of a fat partial annulus curves the wrong way; REQ-001 and REQ-003 now refine the best three, and the torus score adds mirrored starts. Larger start scales alone (8, 16) did not fix the tori. `AxialStart` wraps the `StartSpec` with the frame its parameters refer to.

## Change log

- REQ-001, REQ-003 and REQ-004 amended 2026-11-20 at build: pattern-search refinement of the primary, algebraic cone score for the cone axis, fixed-axis torus fit score for the torus axis.
- REQ-001 and REQ-003 amended 2026-11-20 after the review of #358: the best three candidates are refined (as REQ-004 already did for the torus), not only the best one. Design: mirrored torus starts, supported torus shapes, performance estimates for every primitive.

## Children

_Leaf: none._
