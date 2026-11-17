---
id: ALN.RPS.LOCK
title: RPS alignment with per-point direction locks
level: 3
parent: ALN.RPS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS-004, REQ-ALN.RPS-005, REQ-ALN.RPS-007]
covers: [GOM.ALN.05, PW.ALN.05]
depends_on: [ALN.MODEL.ELEM, ALN.QA.REPORT, GEO.FRAME, KRN.NUM, KRN.SOLV]
side_effect: W2
determinism: D0
cli: [fid align run-rps]
mcp: [align_run_rps]
journal: [align.run_rps@1]
updated: 2026-11-16
---

# ALN.RPS.LOCK: RPS alignment with per-point direction locks

## Purpose

Reference-point-system (RPS) alignment as used in automotive body and sheet-metal work: each reference point has a measured position, a nominal position and a lock set naming which axes of the RPS frame it fixes (for example a main hole locks X and Y, a surface point locks Z). The solver finds the pose that zeroes the locked-direction deviations when exactly six independent DoF are locked, or minimises them in an explicit least-squares RPS mode.

## Scope

**In:**
- `align.run_rps@1` with modes `exact` and `least_squares`
- DoF accounting: lock count and the rank of the locked-direction Jacobian
- The Gauss-Newton solve on locked-direction residuals
- Per-point locked-direction residuals in the residual report

**Out** (and which node owns it instead):
- Producing reference-point positions from holes, slots and surface points (`KRN`, `IND`)
- Re-projection of vector points on scan data (`ALN.RPS.VECTOR`); iterative 3-2-1 (`ALN.RPS.ITER`)
- Conditioning warnings beyond the rank check (`ALN.QA.COND`)

## Requirements

- **REQ-ALN.RPS.LOCK-001** The command shall accept for each reference point a measured position, a nominal position and a non-empty lock subset of {X, Y, Z} of the RPS frame (default: the parent alignment's frame).
- **REQ-ALN.RPS.LOCK-002** The system shall compute the rank of the locked-direction Jacobian at the nominal pose with respect to the six pose parameters, treating singular values below 1e-9 times the largest as zero [unverified].
- **REQ-ALN.RPS.LOCK-003** In `exact` mode, if the lock count is not 6 or the rank is below 6, then the command shall fail with `rps_dof_mismatch` reporting the lock count, the rank and the deficit or surplus, and journal nothing.
- **REQ-ALN.RPS.LOCK-004** In `least_squares` mode, if the lock count is below 6 or the rank is below 6, then the command shall fail with `rps_dof_mismatch`; otherwise it shall minimise the sum of squared locked-direction deviations.
- **REQ-ALN.RPS.LOCK-005** Given noise-free synthetic RPS data with a known transform, the residual in every locked direction shall be at most 1e-12 m.
- **REQ-ALN.RPS.LOCK-006** The residual report shall list, per point and per locked direction, the signed deviation in metres, and also the unlocked-direction deviations marked `not_locked`.
- **REQ-ALN.RPS.LOCK-007** The solve shall stop when the largest locked-direction update is below 1e-13 m or after 50 iterations, and a stop by iteration limit shall give status `not_converged`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/rps/body-rps6` (a synthetic body-in-white panel with 5 reference points: hole XY, slot Y and three surface points Z, totalling 6 independent locks, and a known transform), when `fid align run-rps --mode exact --json` runs, then every locked-direction residual is at most 1e-12 m and the pose matches the known transform within 1e-12 m on the panel's extreme points.
- **AC-2** Given `body-rps6` with an added seventh lock, when run in `exact` mode, then it fails with `rps_dof_mismatch` (count 7, surplus 1); in `least_squares` mode it succeeds.
- **AC-3** (error path) Given six Z locks only (count 6, rank 3), then the command fails with `rps_dof_mismatch` reporting rank 3.
- **AC-4** Given `body-rps6` with 0.05 mm noise (seed 9) in `least_squares` mode with 8 locks, then the residual report lists every point and direction and the RMS equals the root of the minimised objective over the lock count.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode on another platform, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `align.run_rps@1` (W2). Params: `points: Vec<{ name, measured: Coord, nominal: Coord, locks: AxisSet }>`, `mode: exact | least_squares`, `rps_frame` (default parent), `parent`. Result: alignment ID.
- **Data structures:** `RpsPoint`; `DofAccount { lock_count, rank, singular_values }`.
- **Algorithm:** residual r_ij = e_j · (T(p_i) - n_i) for each locked axis e_j; Jacobian with respect to (t, small rotation) at the current pose; Gauss-Newton steps through KRN.SOLV's orthogonal-factorisation driver; rotation update by fid-num exponential map; fixed iteration schedule. Rank from fid-num SVD at the nominal pose.
- **Errors:** `rps_dof_mismatch`, `empty_lock_set`, `frame_not_found`, `nan_input`.
- **Determinism:** D0 (fixed iteration schedule, KNP-1, no threads).
- **Performance budget:** 100 reference points in under 1 ms [unverified until measured].
- **Crate:** solver in `fid-align-solve`, command in `fid-align` (provisional names).

## Test fixtures

`fixtures/aln/rps/body-rps6`: seeded generator (seed 9), a 1.2 x 0.8 m doubly curved panel mesh, reference points with nominal positions and lock sets (hole XY, slot Y, surface Z, surface Z, surface Z: 6 locks rank 6), a known transform, and a noisy variant with 8 locks. Project-generated, CC-BY-4.0.

## Build prompt

> Implement RPS alignment: the solver in kernel crate `fid-align-solve`, the command `align.run_rps@1` in `fid-align` (class W2; CLI `fid align run-rps --mode exact|least_squares --json`; MCP `align_run_rps`; journal `align.run_rps@1`).
> Requirements: each reference point has measured and nominal positions and a non-empty lock subset of {X, Y, Z} of the RPS frame (default parent frame). Compute the rank of the locked-direction Jacobian at the nominal pose (singular values below 1e-9 of the largest count as zero). Exact mode needs exactly 6 locks and rank 6; least-squares mode needs at least 6 locks and rank 6; otherwise fail with `rps_dof_mismatch` reporting count, rank and deficit or surplus, journaling nothing. Solve by Gauss-Newton on locked-direction residuals using the kernel's orthogonal-factorisation driver; stop when the largest update is below 1e-13 m or after 50 iterations (then status `not_converged`). Residual report: per point and locked direction the signed deviation; unlocked directions listed as `not_locked`.
> Acceptance: fixture `fixtures/aln/rps/body-rps6` (seed 9); tests: noise-free locked residuals at most 1e-12 m and pose within 1e-12 m; surplus lock rejected in exact mode, accepted in least-squares mode; rank-3 lock set rejected; noisy least-squares report contents; cross-platform exact replay IDENTICAL.
> Constraints: KNP-1 (no FMA, fid-num for transcendentals and SVD, fixed-order reductions, no std HashMap/HashSet, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- None blocking. The rank tolerance (1e-9) is a proposal.

## Children

_Leaf: none._
