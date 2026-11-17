---
id: ALN.BFIT.CONSTR
title: Degree-of-freedom locks and weights for best-fit
level: 3
parent: ALN.BFIT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT-003, REQ-ALN.BFIT-004]
covers: [GOM.ALN.10, PW.ALN.07, SA.XFM.04]
depends_on: [GEO.FRAME, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.BFIT.CONSTR: Degree-of-freedom locks and weights for best-fit

## Purpose

The constrained-best-fit building block: a pose parameterisation in a named lock frame in which locked translations and rotations are constants, not unknowns, so that locked components cannot drift; and the weight model that combines per-point and per-region weights into one weight vector with a recorded digest. The best-fit loops (`ALN.BFIT.ICP`, `ALN.BFIT.FEAT`) build their Jacobians through it.

## Scope

**In:**
- `DofLocks`: any subset of {tx, ty, tz, rx, ry, rz} in a named lock frame (default: the parent alignment's frame)
- The pose parameterisation and the reduced Jacobian (free columns only)
- `WeightSpec`: per-point weights, per-region weights by selector, and their combination
- Validation and digests

**Out** (and which node owns it instead):
- The iteration loop and residual definitions (`ALN.BFIT.ICP`, `ALN.BFIT.FEAT`)
- Selector resolution (`ALN.BFIT.LOCAL`, `GEO.SEL`)
- Locks in closed-form point-pair transforms (`ALN.XFM.PAIRS`)

## Requirements

- **REQ-ALN.BFIT.CONSTR-001** The parameterisation shall express the pose relative to the start pose in the lock frame as (tx, ty, tz) and rotation R = Rz(c)·Ry(b)·Rx(a), and locked parameters shall be held at exactly 0.
- **REQ-ALN.BFIT.CONSTR-002** When locks are set, the resulting pose's locked components, evaluated in the lock frame relative to the start pose, shall differ from 0 by no more than 1e-12 m or 1e-12 rad.
- **REQ-ALN.BFIT.CONSTR-003** The function `reduced_jacobian` shall return only the columns of free parameters, in the fixed order tx, ty, tz, a, b, c.
- **REQ-ALN.BFIT.CONSTR-004** If all six parameters are locked, then validation shall fail with `all_dof_locked`; if the lock frame does not exist, it shall fail with `frame_not_found`.
- **REQ-ALN.BFIT.CONSTR-005** If the free rotation parameter b reaches an absolute value above 80 degrees during a fit, then the update shall fail with `lock_parameterisation_singular` [unverified: margin to gimbal lock].
- **REQ-ALN.BFIT.CONSTR-006** The combined weight of a point shall be its per-point weight times the product of the weights of every region containing it (default 1), and a combined weight of 0 shall exclude the point.
- **REQ-ALN.BFIT.CONSTR-007** If any weight is negative, NaN or infinite, or all combined weights are 0, then validation shall fail with `invalid_weight` naming the first offending input.
- **REQ-ALN.BFIT.CONSTR-008** The function shall return the SHA-256 digest of the canonical locks and of the combined weight vector (little-endian f64 bytes in index order).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/bfit/locks-sweep` (a synthetic cloud with 64 known poses, one per lock pattern of the six parameters), when each pose is recovered with its lock pattern applied in a small Gauss-Newton test loop, then every locked component is 0 within 1e-12 and every free component matches the true pose within 1e-9.
- **AC-2** Given locks {tz, rx, ry}, then `reduced_jacobian` has 3 columns in order tx, ty, c.
- **AC-3** (error path) Given all six locked, then validation fails with `all_dof_locked`.
- **AC-4** Given per-point weights w and two overlapping regions with weights 2 and 0.5, then a point in both has combined weight w and a point in a region with weight 0 is excluded.
- **AC-5** (error path) Given a weight of -1, then validation fails with `invalid_weight`.
- **AC-6** (determinism) Given the same weights, then the weight digest is identical across platforms.

## Design

- **Functions:** `validate(locks, frame, weights) -> Result<ConstraintSet, ConstrError>`; `pose_from_params(&ConstraintSet, free: &[f64]) -> RigidTransform`; `reduced_jacobian(&ConstraintSet, full_6col_rows) -> Matrix`; `combined_weights(&ConstraintSet, n_points) -> Vec<f64>`. All R1, no command.
- **Data structures:** `DofLocks(u8 bitmask)`, `WeightSpec { per_point: Option<Vec<f64>>, per_region: Vec<(Selector, f64)> }`, `ConstraintSet { lock_frame: Transform, locks, weights, locks_digest, weights_digest }`.
- **Algorithm:** pose = lock-frame conjugation of (translation, Rz·Ry·Rx) applied to the start pose; Jacobian columns of the six parameters at the current point, free columns selected in fixed order. Rotations through fid-num `sin`/`cos`.
- **Errors:** `all_dof_locked`, `frame_not_found`, `lock_parameterisation_singular`, `invalid_weight`.
- **Determinism:** D0 under KNP-1.
- **Performance budget:** combined weights for 1 M points and 16 regions in under 30 ms [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`fixtures/aln/bfit/locks-sweep`: seeded generator (seed 5), 2,000 points on a non-symmetric freeform patch, 64 poses with small rotations (below 5 degrees) and translations (below 5 mm). Project-generated, CC-BY-4.0.

## Build prompt

> In the kernel crate `fid-align-solve`, implement the constraint set for best-fit: DoF locks and weights.
> Requirements: locks are any subset of {tx, ty, tz, rx, ry, rz} in a named lock frame; the pose relative to the start pose is (tx, ty, tz) plus R = Rz(c)·Ry(b)·Rx(a) in the lock frame, with locked parameters held at exactly 0, so locked components never drift by more than 1e-12; `reduced_jacobian` returns only free columns in the order tx, ty, tz, a, b, c; all six locked gives `all_dof_locked`; unknown frame gives `frame_not_found`; |b| above 80 degrees gives `lock_parameterisation_singular`. Combined weight = per-point weight times the product of region weights; 0 excludes; negative, NaN, infinite or all-zero weights give `invalid_weight`. Return SHA-256 digests of the canonical locks and of the combined weight vector (little-endian f64 bytes in index order).
> Acceptance: fixture `fixtures/aln/bfit/locks-sweep` (seed 5); all 64 lock patterns recovered with locked components 0 within 1e-12 and free components within 1e-9; column order; error cases; combined-weight rule; digest stability.
> Constraints: KNP-1 (no FMA, fid-num for sin/cos, no runtime SIMD dispatch, fixed-order reductions, no std HashMap/HashSet, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- Locks with two free rotations use a fixed Euler sequence; incumbents may define rotation locks differently [unverified]. The sequence is recorded so results are reproducible.

## Children

_Leaf: none._
