---
id: ALN.BFIT.ICP.METRIC
title: Point-to-point and point-to-plane residuals and Jacobians
level: 4
parent: ALN.BFIT.ICP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-align-solve]
built_pr: 646
satisfies: [REQ-ALN.BFIT.ICP-002, REQ-ALN.BFIT.ICP-005]
covers: []
depends_on: [ALN.BFIT.CONSTR, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.BFIT.ICP.METRIC: Point-to-point and point-to-plane residuals and Jacobians

## Purpose

Turns correspondences into weighted residual rows and their Jacobian with respect to the free pose parameters, for the two surface metrics: point-to-point (three rows per point) and point-to-plane (one row per point, the distance along the reference normal).

## Scope

**In:** residual and twist-Jacobian rows for both metrics; weighting by the square root of the combined weight; column reduction through `ALN.BFIT.CONSTR`; the metric enum and its algorithm-ID suffix.
**Out:** correspondences (`ALN.BFIT.ICP.CORR`); the step and loop (`ALN.BFIT.ICP.LOOP`); weights and locks (`ALN.BFIT.CONSTR`).

## Requirements

- **REQ-ALN.BFIT.ICP.METRIC-001** For `point_to_point`, each valid correspondence shall give the three rows x - q (x the moved measured point, q the foot) with Jacobian rows [-[x]x | I] with respect to the twist (rotation, translation).
- **REQ-ALN.BFIT.ICP.METRIC-002** For `point_to_plane`, each valid correspondence shall give the single row n · (x - q) with Jacobian row [(x cross n)^T, n^T].
- **REQ-ALN.BFIT.ICP.METRIC-003** Each row and its Jacobian shall be multiplied by the square root of the point's combined weight, and points with weight 0 or no correspondence shall contribute no rows.
- **REQ-ALN.BFIT.ICP.METRIC-004** The function shall return the reduced Jacobian (free columns only, order tx, ty, tz, a, b, c) through `ALN.BFIT.CONSTR`'s `reduced_jacobian`.
- **REQ-ALN.BFIT.ICP.METRIC-005** The metric name shall map one-to-one to the algorithm IDs `align.bestfit.point_to_point@1.0.0` and `align.bestfit.point_to_plane@1.0.0`.
- **REQ-ALN.BFIT.ICP.METRIC-006** The function shall follow KNP-1 and return rows in measured-index order, so output is D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/bfit/icp/metric_rows.json` (five correspondences with known x, q, n), when rows are built for each metric, then values match the analytic ones within 1e-15 m.
- **AC-2** Given random poses (seed 5), then the Jacobian matches central finite differences to a relative 1e-6.
- **AC-3** Given weights (0, 1, 4) on three points, then the first contributes no rows and the third's rows are doubled.
- **AC-4** Given locks on tz and c, then the reduced Jacobian has four columns in the order tx, ty, a, b.

## Design

- **Registry command(s):** none; R1 library function.
- **Function:** `build_rows(metric: Metric, moved: &[P3], corr: &Correspondences, weights: &[f64], cs: &ConstraintSet) -> Result<Rows, MetricError>`; `Rows { r: Vec<f64>, j: Mat, point_of_row: Vec<u64> }`.
- **Algorithm:** as in REQ-001 and REQ-002, rows appended in index order, then column reduction.
- **Errors:** `nan_input`, `empty_rows` (no valid correspondence).
- **Determinism:** D0.
- **Performance budget:** 1,000,000 point-to-plane rows in under 30 ms [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`metric_rows.json`, analytic, generated in-repo (CC0).

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `build_rows(metric, moved, corr, weights, cs) -> Result<Rows, MetricError>`. Point-to-point: three rows x - q, Jacobian [-[x]x | I]. Point-to-plane: one row n·(x - q), Jacobian [(x×n)^T, n^T]. Multiply rows by sqrt(combined weight); skip weight-0 and invalid correspondences; reduce columns with `reduced_jacobian` (ALN.BFIT.CONSTR) in order tx, ty, tz, a, b, c. Map metrics to algorithm IDs `align.bestfit.point_to_point@1.0.0` and `align.bestfit.point_to_plane@1.0.0`. Errors `nan_input`, `empty_rows`. Tests: AC-1..AC-4 including finite differences. Constraints: CONSTITUTION §8, KNP-1 (no FMA, fixed-order, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- Build notes (fid-align-solve `icp::metric`): `Correspondences` (the structure of arrays named by `ALN.BFIT.ICP.CORR`'s Design, `valid` as `Vec<bool>`, `reason` with an extra `hit` value for valid points) is defined in `fid_align_solve::icp` because this leaf reads it; CORR fills it. `weights` is the per-point combined weight (normally `cs.combined_weights()`, or those scaled by the loop) and must cover `cs.n_points()` points. Refusals added beyond `nan_input` and `empty_rows`, all fail closed: `length_mismatch`, `invalid_weight` (negative), `inconsistent_correspondence` (`valid` disagrees with `reason`), `non_unit_normal` (point-to-plane, `|n·n - 1| > 1e-12`), `non_finite` (a row overflowed), `input_limit`, `allocation_failed`; `nan_input` also covers infinities.
- For `ALN.BFIT.ICP.LOOP`: the twist columns are derivatives for a small motion `x -> x + w x x + v` about the origin of the coordinates the points are given in. The reduced columns mean the locked DoFs only when the points, feet and normals are passed in the lock frame, and they are first order: the loop must map each step onto the `ALN.BFIT.CONSTR` parameters, whose `pose_from_params` certifies the locks (`lock_drift`).
- The 30 ms budget for 1,000,000 point-to-plane rows is not measured [unverified].

## Children

_Leaf: none._
