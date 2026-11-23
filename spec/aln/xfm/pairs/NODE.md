---
id: ALN.XFM.PAIRS
title: Rigid point-pair transform solver
level: 3
parent: ALN.XFM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.XFM-001, REQ-ALN.XFM-003, REQ-ALN.XFM-007]
covers: [SA.XFM.01]
depends_on: [ALN.BFIT.CONSTR, ALN.MODEL.ELEM, ALN.QA.REPORT, KRN.NUM, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid align run-point-pairs]
mcp: [align_run_point_pairs]
journal: [align.run_point_pairs@1]
built_in: [fid-align-solve, fid-align]
updated: 2026-11-23
---

# ALN.XFM.PAIRS: Rigid point-pair transform solver

## Purpose

The rigid transform that best maps N >= 3 measured points onto their corresponding reference points in the weighted least-squares sense, with optional per-axis DoF locks. It is both a user command (SpatialAnalyzer-style point-to-point best-fit) and the pure R1 building block that `ALN.PRE.PAIRS`, `ALN.XFM.LEAP` and LVM call.

## Scope

**In:**
- Closed-form weighted rigid solution (no locks)
- Locked solution through `ALN.BFIT.CONSTR`'s parameterisation and a fixed-schedule Gauss-Newton
- Input validation (count, collinearity, weights)
- `align.run_point_pairs@1` (W2) and `point_pairs_rigid(...)` (R1)

**Out** (and which node owns it instead):
- Uniform scale (`ALN.XFM.SCALE`)
- Choosing correspondences (`ALN.PRE`, `VIZ`)
- Network adjustment (`LVM`); transform uncertainty (`UNC.XFM`)

## Requirements

- **REQ-ALN.XFM.PAIRS-001** When given N >= 3 non-collinear pairs and no locks, the solver shall return the rigid transform minimising the weighted sum of squared pair distances, computed in closed form from the weighted cross-covariance with a reflection correction so that the rotation determinant is +1.
- **REQ-ALN.XFM.PAIRS-002** When locks are given, the solver shall minimise the same objective over the free parameters of `ALN.BFIT.CONSTR`'s parameterisation with a fixed-schedule Gauss-Newton (at most 50 iterations, stop at an update below 1e-14 m), keeping locked components at exactly 0.
- **REQ-ALN.XFM.PAIRS-003** Given noise-free synthetic pairs with a known transform, the recovered transform shall reproduce every target point within 1e-12 m for parts within 10 m of the origin [unverified].
- **REQ-ALN.XFM.PAIRS-004** If fewer than 3 pairs are given, any weight is negative or non-finite, or the measured points are collinear (second-largest to largest singular value of the weighted centred set below 1e-6 [unverified]), then the solver shall fail with `too_few_pairs`, `invalid_weight` or `collinear_pairs`, and the command shall journal nothing.
- **REQ-ALN.XFM.PAIRS-005** The residual report shall list per pair the residual vector, its length and its weight, plus weighted RMS and maximum length.
- **REQ-ALN.XFM.PAIRS-006** The solver shall run in the CPU kernel under KNP-1 and declare class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/xfm/pairs-known` (sets of 3, 4, 10 and 1,000 exact pairs under 20 known transforms, including 179-degree rotations, with points up to 10 m from the origin), when `point_pairs_rigid` runs, then every target is reproduced within 1e-12 m and the rotation determinant is +1.
- **AC-2** Given a pair set whose unweighted Kabsch solution would be a reflection (a planar set mirrored), then the result is a proper rotation and the residual report shows the non-zero residuals.
- **AC-3** Given the 10-pair set with locks {rx, ry}, then rx and ry stay exactly 0 relative to the start pose and the result matches the fixture's expected locked solution within 1e-10 m.
- **AC-4** (error path) Given 3 collinear pairs, then the command fails with `collinear_pairs` and journals nothing.
- **AC-5** Given weights (1, 1, 1, 0) on 4 pairs, then the result equals the unweighted 3-pair result within 1e-15 m.
- **AC-6** (replay) Given a run of `fid align run-point-pairs --json`, when replayed in exact mode on another platform, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `align.run_point_pairs@1` (W2). Params: `pairs: Vec<{ measured: Coord | PointRef, reference: Coord | PointRef, weight? }>`, `dof_locks?`, `lock_frame?`, `parent?`. Result: alignment ID. R1 entry: `point_pairs_rigid(measured: &[P3], reference: &[P3], weights: Option<&[f64]>, locks: Option<&ConstraintSet>) -> Result<PairSolution, XfmError>`.
- **Data structures:** `PairSolution { transform: RigidTransform, residuals: Vec<Vec3>, weighted_rms, max_len }`.
- **Algorithm:** weighted centroids; centre and scale inputs by the RMS radius; 3 x 3 weighted cross-covariance H; fid-num SVD H = U S V^T; R = V diag(1, 1, det(V U^T)) U^T; t = q_bar - R p_bar (Arun, Huang and Blostein 1987; Umeyama 1991 reflection handling). Locked case: start from the start pose, Gauss-Newton on the reduced Jacobian, fixed schedule.
- **Errors:** `too_few_pairs`, `collinear_pairs`, `invalid_weight`, `nan_input`, `all_dof_locked`.
- **Determinism:** D0 (fixed-order sums, fid-num SVD, fixed iteration schedule).
- **Performance budget:** 100,000 pairs in under 20 ms [unverified until measured].
- **Crate:** solver in `fid-align-solve`, command in `fid-align` (provisional names).

## Test fixtures

`fixtures/aln/xfm/pairs-known`: seeded generator (seed 10), known transforms including near-180-degree rotations, point sets of 3 to 1,000 points within 10 m of the origin, a mirrored planar set, and expected locked solutions computed by an out-of-kernel reference in CI. Project-generated, CC-BY-4.0.

## Build prompt

> Implement the rigid point-pair transform: R1 solver `point_pairs_rigid` in kernel crate `fid-align-solve`, and command `align.run_point_pairs@1` in `fid-align` (class W2; CLI `fid align run-point-pairs --json`; MCP `align_run_point_pairs`; journal `align.run_point_pairs@1`).
> Requirements: N >= 3 pairs (`too_few_pairs`); weights non-negative and finite (`invalid_weight`); non-collinear measured points (ratio of second-largest to largest singular value of the weighted centred set at least 1e-6, else `collinear_pairs`). Without locks: closed-form weighted solution (centroids, centring and RMS scaling, cross-covariance SVD via fid-num, reflection correction so det R = +1). With locks: use the best-fit constraint parameterisation (locked parameters held at exactly 0), fixed-schedule Gauss-Newton, at most 50 iterations, stop at an update below 1e-14 m. Residual report: per pair residual vector, length and weight; weighted RMS; max length. Errors journal nothing.
> Acceptance: fixture `fixtures/aln/xfm/pairs-known` (seed 10); tests: exact recovery within 1e-12 m including near-180-degree rotations and 10 m offsets; mirrored set gives a proper rotation; locked solution keeps locked parameters at 0 and matches the expected solution within 1e-10 m; collinear rejection; zero weight equals dropping the pair; cross-platform exact replay IDENTICAL.
> Constraints: KNP-1 (no FMA, fid-num for SVD and sqrt, no runtime SIMD dispatch, fixed-order reductions, no std HashMap/HashSet, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- `LVM.MOVE` should call `point_pairs_rigid` for its quick pose solve rather than `ALN.XFM.LEAP` (cross-domain note; no edit made there).

## Children

_Leaf: none._
