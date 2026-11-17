---
id: KRN.LSQ.CURVE.POLY
title: Least-squares polyline with a given vertex count
level: 4
parent: KRN.LSQ.CURVE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.CURVE-002, REQ-KRN.LSQ.CURVE-004, REQ-KRN.LSQ.CURVE-005]
covers: [GOM.CON.18, PW.CON.03]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.LIN, KRN.SOLV.NLLS, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit ls-polyline]
mcp: [fit_ls_polyline]
journal: [fit.ls_polyline@1]
updated: 2026-11-17
---

# KRN.LSQ.CURVE.POLY: Least-squares polyline with a given vertex count

## Purpose

Fits a polyline with a caller-given number of vertices to points ordered along a curve, by minimising the squared orthogonal distance of each point to its nearest segment. It turns a measured profile (a section edge, a probed contour) into a compact chain of straight segments, in a plane or in 3D.

## Scope

**In:** algorithm `fit.polyline.ls@1.0.0`; inputs ordered points (order taken as given), vertex count k (2 to 1 024), optional fixed end vertices, optional projection plane; start by splitting the points into k-1 runs of equal arc length and fitting a line per run (`KRN.LSQ.LIN`); vertex positions refined by nonlinear least squares with deterministic point-to-segment assignment (nearest segment, ties by lower segment index) updated after each accepted step; an ordering check; the command; a spec card.
**Out:** smoothing splines (`KRN.LSQ.CURVE.SPLINE`); automatic choice of vertex count (a later version); section extraction (`KRN.CONS.DERIV.SECT`, `SCAN`).

## Requirements

- **REQ-KRN.LSQ.CURVE.POLY-001** The module shall fit a polyline with exactly the given vertex count to ordered points, minimising the sum of squared distances from each point to its assigned segment.
- **REQ-KRN.LSQ.CURVE.POLY-002** Point-to-segment assignment shall be a deterministic function of the current vertices (nearest segment, ties by lower index), and the final assignment shall be returned.
- **REQ-KRN.LSQ.CURVE.POLY-003** If the points are not ordered along the curve (the projection of consecutive points onto the start polyline goes backward by more than the configured `order_tol_m`), then the fit shall return `NotOrdered{index}`.
- **REQ-KRN.LSQ.CURVE.POLY-004** If any segment is assigned fewer than 2 points, then the fit shall return `UnderSampled{segment}`.
- **REQ-KRN.LSQ.CURVE.POLY-005** When the command is committed, the system shall journal one `fit.ls_polyline@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/poly-v1` (seeded zig-zag profiles with 3 to 50 known vertices and 2 micrometre noise), when `fid fit ls-polyline --vertices <k> --json` runs with the true k, then every vertex is within 3 sigma of the generator's propagated noise [unverified bound].
- **AC-2** Given the same profile with two points swapped far apart, then the fit returns `NotOrdered{index}` and journals nothing.
- **AC-3** Given k larger than half the point count, then the fit returns `UnderSampled{segment}`.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.ls_polyline@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`, order as stored), `vertices: u32`, optional fixed ends, optional projection plane, `order_tol_m`, `NllsConfig`. Result: `FittedFeature::Polyline { vertices }`, assignment, per-segment residual statistics, diagnostics.
- **Algorithm:** equal-arc-length runs; per-run line fits; vertices at intersections of consecutive run lines (or run ends when near-parallel); LM over vertex coordinates with reassignment after each accepted step and a cap of 20 reassignment rounds.
- **Errors:** `TooFewPoints`, `NotOrdered`, `UnderSampled`, `NotConverged`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 100 000 points, 100 vertices < 300 ms [unverified until measured].
- **Crate:** `fid-fit` (module `ls_poly`).

## Test fixtures

`fixtures/fit/poly-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_poly` implementing the `Fit` trait for `fit.polyline.ls@1.0.0`: ordered input points, required vertex count (2..=1024), optional fixed ends and projection plane; start from equal-arc-length runs with per-run least-squares lines; refine vertices with `fid_solv::nlls` using deterministic nearest-segment assignment (ties by lower index), reassigned after each accepted step (max 20 rounds); return `NotOrdered{index}` beyond `order_tol_m` and `UnderSampled{segment}` for segments with fewer than 2 points; return the final assignment. Add the registry data row `fit.ls_polyline@1` (W2, dry-run R1, D0; CLI `fid fit ls-polyline`; MCP `fit_ls_polyline`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/poly-v1`, swapped-order error, under-sampled error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.CURVE.POLY.

## Open questions

- None.

## Children

_Leaf: none._
