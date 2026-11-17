---
id: KRN.ENV.XCHK
title: Combinatorial cross-checks for MCC and MIC
level: 3
parent: KRN.ENV
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV-004, REQ-KRN.ENV-008]
covers: []
depends_on: [KRN.MZ.VOR, KRN.NUM.RNG, KRN.LSQ.TRAIT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.ENV.XCHK: Combinatorial cross-checks for MCC and MIC

## Purpose

Independent, solver-free algorithms that check the conic envelope fits: Welzl's algorithm for the minimum circumscribed circle and sphere (convex, so it gives the global optimum), and the Voronoi-based maximum inscribed circle restricted to a convex region. They run in CI against `KRN.ENV.MCC` and `KRN.ENV.MIC`, and any disagreement fails the build.

## Scope

**In:** iterative Welzl (move-to-front) for 2D circle and sphere under a seeded, journaled shuffle; Voronoi MIC 2D circle with the centre restricted to a convex polygon (the convex hull, intersected with a trust disc); algorithm IDs and spec cards.
**Out:** the conic MCC and MIC fits and the CI agreement jobs (`KRN.ENV.MCC`, `KRN.ENV.MIC`); Voronoi diagram construction (`KRN.MZ.VOR`).

## Requirements

- **REQ-KRN.ENV.XCHK-001** The module shall compute the minimum enclosing circle and sphere by an iterative (non-recursive) move-to-front Welzl algorithm over a shuffle drawn from the `fid-num` RNG with a caller-given seed.
- **REQ-KRN.ENV.XCHK-002** The enclosing result shall be independent of the seed up to the support set, and the module shall return the support set (2 to 4 points) with the radius recomputed from it.
- **REQ-KRN.ENV.XCHK-003** The module shall compute the maximum inscribed 2D circle whose centre lies in a given convex polygon, by evaluating every nearest-point Voronoi vertex inside the polygon, every intersection of a Voronoi edge with the polygon boundary, and every polygon vertex.
- **REQ-KRN.ENV.XCHK-004** If the inscribed optimum lies on the polygon boundary, then the module shall report `on_region_boundary` so the conic path's `unbounded_envelope` handling can be checked.
- **REQ-KRN.ENV.XCHK-005** If fewer points are given than the primitive needs, or all points are collinear (circle) or coplanar (sphere), then the module shall return a typed `FitError`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/xchk-mcc-v1` (1 000 seeded point sets of 3 to 5 000 points, including cocircular and cospherical cases), when Welzl runs with seeds 1, 2 and 3, then the radius is bit-identical across seeds and matches a test-only brute-force oracle over all 2-, 3- and 4-point support sets for n <= 30 within 1e-12 m.
- **AC-2** Given fixture `fixtures/fit/xchk-mic-v1` (seeded hole-like point sets with coverage 360, 180 and 90 degrees), when the Voronoi MIC runs with the convex hull as region, then it matches a test-only grid-refinement oracle within 1e-9 m for full coverage and reports `on_region_boundary` for the 90-degree cases.
- **AC-3** Given AC-1 and AC-2 inputs on x86-64, ARM64 and wasm32 under wasmtime, then output digests are identical.
- **AC-4** (error path) Given 2 points for a sphere, then the module returns `FitError::TooFewPoints`.

## Design

- **Registry command(s):** none; kernel algorithms `fit.circle2d.mcc_welzl@1.0.0`, `fit.sphere.mcc_welzl@1.0.0`, `fit.circle2d.mic_vor@1.0.0` with spec cards, used as cross-checks.
- **Data structures:** `Enclosing { centre, radius, support: Vec<u32> }`; `Inscribed { centre, radius, contacts: Vec<u32>, on_region_boundary: bool }`.
- **Algorithm:** Welzl 1991, "Smallest enclosing disks (balls and ellipsoids)", in the iterative move-to-front form with an explicit stack; circumcircle and circumsphere by small linear solves through `fid_num::la`. MIC per requirement 003 using `KRN.MZ.VOR` diagrams and exact-sign predicates.
- **Errors:** `FitError::{TooFewPoints, Collinear, Coplanar, NonFinite}`.
- **Determinism:** D0 (the seed changes only the visiting order; the support set and recomputed radius do not depend on it).
- **Performance budget:** Welzl on 100 000 points < 20 ms [unverified until measured].
- **Crate:** `fid-fit` (module `xchk`).

## Test fixtures

`fixtures/fit/xchk-mcc-v1` and `fixtures/fit/xchk-mic-v1`: seeded generators committed with the fixtures, CC-BY-4.0.

## Build prompt

> In crate `fid-fit`, create module `xchk`. Implement iterative move-to-front Welzl for the minimum enclosing 2D circle and sphere (explicit stack, no recursion), shuffling input order with `fid_num::rng::Cursor::shuffle` from a caller seed; recompute the radius from the final support set (2 to 4 points) with `fid_num::la` solves. Implement the maximum inscribed 2D circle with the centre restricted to a convex polygon, using the nearest-point Voronoi diagram from module `vor` (spec node KRN.MZ.VOR): evaluate Voronoi vertices inside the polygon, Voronoi-edge/polygon-edge intersections and polygon vertices; report `on_region_boundary`. Register `fit.circle2d.mcc_welzl@1.0.0`, `fit.sphere.mcc_welzl@1.0.0`, `fit.circle2d.mic_vor@1.0.0` with spec cards. Tests: `fixtures/fit/xchk-mcc-v1` (seed independence, brute-force oracle n <= 30), `fixtures/fit/xchk-mic-v1` (grid-refinement oracle, boundary flag on partial coverage), cross-target digest identity on x86-64/ARM64/wasm32, too-few-points error. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no unbounded recursion, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.ENV.XCHK.

## Open questions

- A sphere MIC cross-check (3D Voronoi) is not included; it is costly and has no simple exact path [unverified]. Sphere and cylinder MIC rely on multi-start and ORS cases.

## Children

_Leaf: none._
