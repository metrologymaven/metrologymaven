---
id: ALN.RPS.VECTOR.LOOP
title: Vector-point iterative locked-direction solve
level: 4
parent: ALN.RPS.VECTOR
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS.VECTOR-001, REQ-ALN.RPS.VECTOR-003, REQ-ALN.RPS.VECTOR-004, REQ-ALN.RPS.VECTOR-005, REQ-ALN.RPS.VECTOR-006]
covers: []
depends_on: [KRN.SOLV.NLLS, KRN.NUM, GEO.FRAME.XFORM, ALN.QA.REPORT, ALN.QA.HIST, ALN.QA.COND]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.RPS.VECTOR.LOOP: Vector-point iterative locked-direction solve

## Purpose

The pure R1 entry `solve_vector_points` shared by the sheet-metal vector-point alignment and the iterative 3-2-1 (`ALN.RPS.ITER`): every vector point has a nominal position and nominal unit vector; on each outer iteration a deviation provider re-evaluates where each point's measured counterpart lies under the current pose, the pose is re-solved so that locked points' deviations along their nominal vectors are minimised, and the loop stops when those deviations stop changing.

## Scope

**In:**
- the `DeviationProvider` interface and its discrete-hit implementation (deviation = (T m - n) · d);
- the per-iteration locked-direction solve over six pose parameters (KRN.SOLV.NLLS, QR steps);
- the DoF check (rank of the locked-direction Jacobian) after exclusions;
- exact mode (6 locks) and least-squares mode (more than 6 locks);
- the outer convergence test, iteration limit, history and status;
- deviations of unlocked points reported, not fitted.

**Out:**
- the measured-surface provider (`ALN.RPS.VECTOR.SURF`);
- the commands (`ALN.RPS.VECTOR.CMD`, `ALN.RPS.ITER`);
- frame-axis RPS locks (`ALN.RPS.LOCK`).

## Requirements

- **REQ-ALN.RPS.VECTOR.LOOP-001** On each outer iteration k, the solver shall ask the provider for every point's measured counterpart q_i under pose T_k (for a discrete hit, the hit itself) and the signed deviation e_i = (T_k q_i - n_i) · d_i, or an exclusion with a reason.
- **REQ-ALN.RPS.VECTOR.LOOP-002** The solver shall then compute T_{k+1} minimising the sum of e_i(T)^2 over locked, non-excluded points, starting from T_k, by Gauss-Newton with QR steps (KRN.SOLV.NLLS).
- **REQ-ALN.RPS.VECTOR.LOOP-003** Before each solve, if the locked-direction Jacobian at T_k has rank below 6 (singular values below 1e-9 of the largest [unverified]), then the solver shall fail with `rps_dof_mismatch` reporting lock count, rank and the excluded points.
- **REQ-ALN.RPS.VECTOR.LOOP-004** With exactly 6 effective locks the mode shall be recorded as `exact`; with more, as `least_squares`.
- **REQ-ALN.RPS.VECTOR.LOOP-005** The loop shall stop with `converged` when the largest absolute change of any locked deviation between iterations is below the threshold (default 1e-6 m [unverified]), or with `not_converged` at the iteration limit (default 50), recording count, reason and the per-iteration largest change.
- **REQ-ALN.RPS.VECTOR.LOOP-006** Given noise-free synthetic vector points with a known transform, the converged locked deviations shall be at most 1e-9 m [unverified].
- **REQ-ALN.RPS.VECTOR.LOOP-007** The residual report shall list every point's deviation along its nominal vector at the final pose, marking unlocked points `not_locked` and excluded points with their reason.
- **REQ-ALN.RPS.VECTOR.LOOP-008** The solver shall be single-threaded under KNP-1 with a fixed schedule and declare class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/rps/vector/panel_hits.json` (8 hits on a curved panel, 6 locked, known transform 1 mm and 0.5 degree), when `solve_vector_points` runs with the hit provider, then the locked deviations are at most 1e-9 m, the mode is `least_squares` only if more than 6 are locked, and status is `converged` within 3 iterations.
- **AC-2** Given 6 locks whose vectors are all parallel, then it fails with `rps_dof_mismatch` reporting rank 3.
- **AC-3** Given a provider that never settles (test double alternating deviations) and limit 5, then status is `not_converged` with 5 history rows.
- **AC-4** Given AC-1 run twice on two platforms, then the poses are bit-identical.

## Design

- **Registry command(s):** none; R1 library entry `solve_vector_points(points: &[VectorPoint], provider: &dyn DeviationProvider, params: &VectorParams, start: &Rigid) -> Result<VectorOutput, VectorError>`.
- **Data structures:** `VectorPoint { name, nominal: P3, vector: UnitV3, locked: bool, source: SourceRef }`; `trait DeviationProvider { fn counterpart(&self, i, pose: &Rigid) -> Result<Counterpart, ProviderError>; }` with `Counterpart = At(q) | Excluded(reason)`; `VectorParams { threshold, max_iterations }`; `VectorOutput { pose, status, mode, iterations, history, report, diagnostics, excluded }`.
- **Algorithm:** as REQ-001 to REQ-005; algorithm ID `align.rps.vector_points@1.0.0`.
- **Errors:** `rps_dof_mismatch`, `nan_in_iteration`, solver errors passed through.
- **Determinism:** D0.
- **Performance budget:** 200 points, 50 iterations in under 10 ms with the hit provider [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`panel_hits.json` and degenerate variants, generated in-repo from an analytic panel with seeds (CC0).

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `solve_vector_points(points, provider, params, start) -> Result<VectorOutput, VectorError>` and the `DeviationProvider` trait with a discrete-hit implementation. Outer loop: get each point's counterpart q_i under T_k (or an exclusion), deviation e_i = (T_k q_i - n_i)·d_i; check the rank of the locked-direction Jacobian (singular values below 1e-9 of the largest → `rps_dof_mismatch` with count, rank, exclusions); solve min sum e_i(T)^2 over locked points with the kernel NLLS driver (`fid-solv`, QR steps); record mode `exact` (6 locks) or `least_squares` (>6); stop when the largest change of a locked deviation is below the threshold (default 1e-6 m) or at the limit (default 50) with `not_converged`; record history (ALN.QA.HIST), residual report with `not_locked` and exclusion reasons (ALN.QA.REPORT) and diagnostics (ALN.QA.COND). Algorithm ID `align.rps.vector_points@1.0.0`. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error), single-threaded D0.

## Open questions

- ALN.RPS.LOCK solves locks along shared frame axes only, so it cannot take per-point nominal vectors; this leaf calls the kernel NLLS driver directly instead of ALN.RPS.LOCK. The rank check mirrors LOCK's threshold.

## Children

_Leaf: none._
