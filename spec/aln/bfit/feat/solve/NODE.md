---
id: ALN.BFIT.FEAT.SOLVE
title: Weighted feature best-fit solve, 2D mode and determinacy check
level: 4
parent: ALN.BFIT.FEAT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT.FEAT-001, REQ-ALN.BFIT.FEAT-003, REQ-ALN.BFIT.FEAT-004, REQ-ALN.BFIT.FEAT-005, REQ-ALN.BFIT.FEAT-006]
covers: []
depends_on: [ALN.BFIT.FEAT.DEV, ALN.BFIT.CONSTR, ALN.BFIT.STOP, ALN.QA.REPORT, ALN.QA.HIST, ALN.QA.COND, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.BFIT.FEAT.SOLVE: Weighted feature best-fit solve, 2D mode and determinacy check

## Purpose

The pure R1 entry point `bestfit_features` that the command and CMM call: it minimises the weighted sum of squared per-feature deviations (from `ALN.BFIT.FEAT.DEV`) over the free pose parameters (from `ALN.BFIT.CONSTR`), in 3D or in a named working plane, refuses feature sets that cannot determine the free DoF, and returns the pose, residual report, convergence history and diagnostics.

## Scope

**In:** Gauss-Newton iterations with QR steps on the reduced Jacobian; 2D mode as a fixed lock pattern in the working-plane frame plus in-plane deviations; the determinacy (rank) check with DoF naming; stopping through `ALN.BFIT.STOP`; residual report, history and diagnostics.
**Out:** deviation definitions (`ALN.BFIT.FEAT.DEV`); the registry command, element resolution and alignment wrapping (`ALN.BFIT.FEAT.CMD`); scale (`ALN.XFM.SCALE`).

## Requirements

- **REQ-ALN.BFIT.FEAT.SOLVE-001** The solver shall minimise sum over pairs of w_i times the squared norm of pair i's deviation vector over the free pose parameters, starting from the given start pose (the parent alignment's pose by default).
- **REQ-ALN.BFIT.FEAT.SOLVE-002** Each step shall be the least-squares solution of the weighted, reduced linear system by Householder QR (never normal equations), and the pose shall be updated through `ALN.BFIT.CONSTR`'s `pose_from_params`.
- **REQ-ALN.BFIT.FEAT.SOLVE-003** In 2D mode, the solver shall lock tz, a and b in the working-plane frame (leaving tx, ty and the rotation about the plane normal free), pass the working plane to the deviation function, and record the working plane's frame ID and digest.
- **REQ-ALN.BFIT.FEAT.SOLVE-004** Before iterating, the solver shall compute the singular values of the weighted reduced Jacobian at the start pose, and if any is below 1e-9 times the largest [unverified], then it shall fail with `underdetermined_feature_set` listing each null-space direction, named as a pose parameter when its vector is within 1e-6 of a parameter axis and as a generic twist otherwise.
- **REQ-ALN.BFIT.FEAT.SOLVE-005** Stopping shall follow `ALN.BFIT.STOP`; on `stopped` the solver shall return the lowest-objective pose with status `not_converged` and the reason.
- **REQ-ALN.BFIT.FEAT.SOLVE-006** Given the noise-free fixture sets with known transforms, the recovered translation shall be within 1e-9 m and rotation within 1e-9 rad [unverified].
- **REQ-ALN.BFIT.FEAT.SOLVE-007** The solver shall write one residual-report entry per deviation component (pair index, label, value in metres, weight), a convergence history row per iteration, and diagnostics from `ALN.QA.COND` on the final reduced Jacobian.
- **REQ-ALN.BFIT.FEAT.SOLVE-008** If a weight is negative, NaN or infinite, or all weights are zero, then the solver shall fail with `invalid_weight` (from `ALN.BFIT.CONSTR`).
- **REQ-ALN.BFIT.FEAT.SOLVE-009** The solver shall run single-threaded under KNP-1 with a fixed iteration schedule and declare class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/bfit/feat/block_holes.json` (one plane, four circles, one cylinder; known transform of 3 mm and 2 degrees), when `bestfit_features` runs in 3D, then translation error is at most 1e-9 m and rotation error at most 1e-9 rad, and status is `ok`.
- **AC-2** Given fixture `plate_2d.json` (three circles in the XY plane, known in-plane transform), when run in 2D mode with working plane XY, then tz, a and b are unchanged from the start pose within 1e-12 and the in-plane transform is recovered within 1e-9.
- **AC-3** (error path) Given two point pairs in 3D with all DoF free, then the solve fails with `underdetermined_feature_set` naming one generic twist (rotation about the line through the two points).
- **AC-4** Given `block_holes.json` with `max_iterations: 1` and a 20-degree start error, then status is `not_converged`, reason `max_iterations`, and the returned pose is the lowest-objective one.
- **AC-5** Given `block_holes.json` with Gaussian noise (seed 11, 5 µm), when run twice on two platforms, then the poses are bit-identical.

## Design

- **Registry command(s):** none; R1 library entry `bestfit_features(pairs: &[WeightedPair], params: &FeatParams, start: &Rigid) -> Result<FeatOutput, FeatError>`.
- **Data structures:** `WeightedPair { pair: FeaturePair, weight: f64, use_size: bool, length_scale: Option<f64> }`; `FeatParams { mode: Mode3d | Mode2d { plane: FrameRef }, locks: DofLocks, lock_frame: FrameRef, stop: StopCriteria }`; `FeatOutput { pose, status, stop_reason, report: ResidualReport, history: ConvergenceHistory, diagnostics: AlignmentDiagnostics, length_scales: Vec<f64>, algorithm_id: "align.bestfit.features@1.0.0" }`.
- **Algorithm:** validate constraints; in 2D mode merge the fixed lock pattern with user locks in the working-plane frame; stack weighted deviation rows (sqrt(w) times value and Jacobian) in pair order; reduce columns; rank check by SVD (KRN.NUM); loop: QR step, pose update, monitor update; at the end build report, history and diagnostics.
- **Errors:** `underdetermined_feature_set{directions}`, `invalid_weight`, `all_dof_locked`, `lock_parameterisation_singular`, `pair_type_mismatch` (pass-through), `nan_in_iteration`.
- **Determinism:** D0; single-threaded, fixed order.
- **Performance budget:** 200 pairs, 20 iterations in under 5 ms [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`block_holes.json`, `plate_2d.json` and the two-point case, generated in-repo from analytic features with a seeded script (CC0). Noise variants use the counter-based RNG with recorded seeds.

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `bestfit_features(pairs, params, start) -> Result<FeatOutput, FeatError>`. Objective: weighted sum of squared deviation vectors from `deviation(...)` (module built for ALN.BFIT.FEAT.DEV). Free parameters and pose update come from the constraint module (`validate`, `reduced_jacobian`, `pose_from_params`; ALN.BFIT.CONSTR). In 2D mode, lock tz, a, b in the working-plane frame and pass the plane to `deviation`; record frame ID and digest. Before iterating, SVD the weighted reduced Jacobian at the start pose; if any singular value is below 1e-9 of the largest, fail `underdetermined_feature_set` naming each null direction (a parameter name when within 1e-6 of an axis, else a generic twist). Iterate Gauss-Newton with Householder QR steps (no normal equations), stopping through `StopMonitor` (ALN.BFIT.STOP); on `stopped`, return the lowest-objective pose with status `not_converged`. Build a `ResidualReport` (one entry per component), a `ConvergenceHistory` row per iteration, and diagnostics via `diagnose` (ALN.QA.COND). Algorithm ID `align.bestfit.features@1.0.0`. Tests: AC-1..AC-5. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error), single-threaded, D0.

## Open questions

- None.

## Children

_Leaf: none._
