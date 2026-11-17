---
id: ALN.BFIT.ICP.LOOP
title: Surface best-fit iteration loop and R1 entry point
level: 4
parent: ALN.BFIT.ICP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT.ICP-001, REQ-ALN.BFIT.ICP-004, REQ-ALN.BFIT.ICP-005, REQ-ALN.BFIT.ICP-006, REQ-ALN.BFIT.ICP-007]
covers: []
depends_on: [ALN.BFIT.ICP.CORR, ALN.BFIT.ICP.METRIC, ALN.BFIT.CONSTR, ALN.BFIT.STOP, ALN.QA.REPORT, ALN.QA.HIST, ALN.QA.COND, KRN.OUT.POL, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.BFIT.ICP.LOOP: Surface best-fit iteration loop and R1 entry point

## Purpose

The pure function `bestfit(measured, reference, params) -> BestfitOutput` that SCAN, GDT, CMM, `ALN.PRE.GLOBAL` and the command call: it iterates correspondence, outlier masking, residual rows, a QR step and the pose update until `ALN.BFIT.STOP` says stop, and returns the pose with its residual report, history, diagnostics and exclusion record.

## Scope

**In:** the iteration; the linear step by Householder QR on the reduced system; pose update through `ALN.BFIT.CONSTR`; the per-iteration outlier hook (KRN.OUT policy applied to correspondence distances); best-pose tracking; output assembly.
**Out:** correspondences (`ALN.BFIT.ICP.CORR`); rows (`ALN.BFIT.ICP.METRIC`); scope resolution against the document (`ALN.BFIT.LOCAL`, called by the command); stopping rules (`ALN.BFIT.STOP`); the command and element wrapping (`ALN.BFIT.ICP.CMD`); the benchmark corpus (`ALN.BFIT.BENCH`).

## Requirements

- **REQ-ALN.BFIT.ICP.LOOP-001** Each iteration shall move the in-scope measured points by the current pose, compute correspondences, apply the outlier policy if set, build rows for the chosen metric, solve the reduced weighted linear least-squares step by Householder QR, and update the pose.
- **REQ-ALN.BFIT.ICP.LOOP-002** With scope `global` and no outlier policy, every point with a correspondence and non-zero weight shall contribute to every iteration.
- **REQ-ALN.BFIT.ICP.LOOP-003** When an outlier policy is set, it shall be applied every iteration to the current correspondence distances, and the output shall record the policy's algorithm ID, its parameters and the SHA-256 digest of the final excluded index set.
- **REQ-ALN.BFIT.ICP.LOOP-004** The loop shall feed translation and rotation increments and the objective (weighted sum of squared rows) to `StopMonitor`, append one history row per iteration, and on `stopped` return the lowest-objective pose with status `not_converged`.
- **REQ-ALN.BFIT.ICP.LOOP-005** Given `ALN.BFIT.BENCH` noise-free cases started inside the basin, both metrics shall recover translation within 1e-9 m and rotation within 1e-9 rad [unverified].
- **REQ-ALN.BFIT.ICP.LOOP-006** The output shall be bit-identical for any thread count, and the function shall declare class D0.
- **REQ-ALN.BFIT.ICP.LOOP-007** On the `ALN.BFIT.BENCH` timing case (1,000,000 points, 1,000,000 triangles, prealigned start), the function shall finish in under 5 s on reference hardware [unverified].
- **REQ-ALN.BFIT.ICP.LOOP-008** If fewer than 6 rows remain after correspondence and outlier masking in any iteration, then the function shall fail with `insufficient_correspondences` stating the count.

## Acceptance criteria

- **AC-1** Given `ALN.BFIT.BENCH` noise-free cases, when `bestfit` runs with `point_to_plane`, then every case meets 1e-9 m and 1e-9 rad.
- **AC-2** Given the same cases with `point_to_point`, then every case meets the same bounds.
- **AC-3** Given fixture `fixtures/aln/bfit/icp/plate_outliers.json` (2 % outliers at 5 mm) and policy `NominalGate{max_distance_m: 1e-3}`, then the excluded set equals the planted outliers and its digest is recorded.
- **AC-4** Given `max_iterations: 2` from a 5-degree start, then status is `not_converged` with reason `max_iterations` and the returned pose has the lowest objective seen.
- **AC-5** Given the bench timing case with 1 and 8 threads, then outputs are bit-identical; the 8-thread run is timed against REQ-007.

## Design

- **Registry command(s):** none; R1 library entry `bestfit(measured: &PointView, reference: &dyn ProjectionTarget, params: &BestfitParams, start: &Rigid, cancel) -> Result<BestfitOutput, BestfitError>`.
- **Data structures:** `BestfitParams { scope: ResolvedScope, metric: Metric, constraints: ConstraintSet, outlier: Option<OutlierPolicy>, stop: StopCriteria, max_corr_distance: Option<f64> }`; `BestfitOutput { pose, status, stop_reason, algorithm_id, report: ResidualReport, history: ConvergenceHistory, diagnostics: AlignmentDiagnostics, exclusion: Option<{ policy_id, params, excluded_digest, n_excluded }> }`.
- **Algorithm:** pose_k; moved = pose_k(points in scope); corr = correspond(...); mask = policy(dist) if set; rows = build_rows(...); delta = QR solve (KRN.NUM) of min ||J delta + r||; pose_{k+1} = pose_from_params(...); monitor. The report holds per-point signed distance along the normal at the final pose.
- **Errors:** `insufficient_correspondences`, `nan_in_iteration`, `lock_parameterisation_singular`, `cancelled`, policy errors (`too_few_remaining`, `excessive_exclusion`) passed through.
- **Determinism:** D0; parallel work only inside GEO.DIST.BATCH (thread-independent), reductions fixed-order.
- **Performance budget:** REQ-007.
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`ALN.BFIT.BENCH` corpus; `plate_outliers.json` (seeded, CC0).

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `bestfit(measured, reference, params, start, cancel) -> Result<BestfitOutput, BestfitError>`. Loop: move in-scope points by the current pose; `correspond` (ALN.BFIT.ICP.CORR); if an outlier policy is set, apply the KRN.OUT policy (`fid-filter`) to correspondence distances; `build_rows` (ALN.BFIT.ICP.METRIC); if fewer than 6 rows, fail `insufficient_correspondences`; solve the reduced step by Householder QR (KRN.NUM, never normal equations); update via `pose_from_params` (ALN.BFIT.CONSTR); feed `StopMonitor` (ALN.BFIT.STOP); append a `ConvergenceHistory` row (ALN.QA.HIST). On `stopped`, return the lowest-objective pose, status `not_converged`. Finish with a `ResidualReport` (ALN.QA.REPORT), `diagnose` (ALN.QA.COND) on the final reduced Jacobian, and the exclusion record (policy ID, parameters, SHA-256 of the excluded index set). Output must be bit-identical for any thread count (D0). Tests: AC-1..AC-5 and the bench corpus. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- KRN.OUT.POL is tagged P1b while this leaf is P1a; until it lands, `outlier` must be `None` and the parameter is refused with `outlier_policy_unavailable`.

## Children

_Leaf: none._
