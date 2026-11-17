---
id: GDT.PROF.BESTFIT.LSQ
title: Least-squares objective variant
level: 4
parent: GDT.PROF.BESTFIT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF.BESTFIT-001, REQ-GDT.PROF.BESTFIT-003]
covers: [PCD.DIM.06]
depends_on: [GDT.PROF.BESTFIT.OPTS, GDT.PROF.DEV, GDT.ZONE.POSE, ALN.BFIT.CONSTR, KRN.SOLV.NLLS]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.BESTFIT.LSQ: Least-squares objective variant

## Purpose

Some rows (and many incumbent workflows) place the profile zone by a least-squares fit of the measured points to the nominal over the free DOFs, then report the zone usage at that pose, rather than minimising usage directly [unverified for specific incumbents]. This leaf provides that variant: a DOF-locked least-squares pose using the `ALN.BFIT.CONSTR` parameterisation and `KRN.SOLV.NLLS`, followed by a fixed-pose usage evaluation. The value is never smaller than the minimum-zone value, and the record says which objective produced it.

## Scope

**In:** `lsq_pose(points, oracle, start, free)`: weighted sum of squared signed deviations (weight 1 unless the chain states otherwise) minimised over the free DOFs with re-projection at each iteration; fixed-pose usage via `GDT.ZONE.POSE`; diagnostics.
**Out:** option bookkeeping (owned by `GDT.PROF.BESTFIT.OPTS`); minimum-zone solve (owned by `GDT.ZONE.POSE`); alignment best fits as document operations (owned by `ALN.BFIT`).

## Requirements

- **REQ-GDT.PROF.BESTFIT.LSQ-001** When the objective is `LeastSquares`, the module shall return the pose that minimises Σ d_i² over the free DOFs, with deviations recomputed through the re-projection oracle at each iteration and locked DOFs held at exactly zero change.
- **REQ-GDT.PROF.BESTFIT.LSQ-002** The module shall evaluate the zone usage at that pose with the fixed-pose path of `GDT.ZONE.POSE` and return it as the value, with `objective: least_squares`.
- **REQ-GDT.PROF.BESTFIT.LSQ-003** If the solver stops on its iteration limit or the normal matrix is rank-deficient over the free DOFs, then the module shall return `lsq_not_converged` or `lsq_rank_deficient` and no value.
- **REQ-GDT.PROF.BESTFIT.LSQ-004** The result shall carry the RMS deviation, iteration count and stop reason, and the ResultRecord shall carry the seed of any random start (none by default).
- **REQ-GDT.PROF.BESTFIT.LSQ-005** For every fixture, the least-squares value shall be greater than or equal to the minimum-zone value from `GDT.ZONE.POSE` on the same DOFs, within 1e-12 m.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/prof/bestfit/plane_shift.json` (plane, points with a planted 0.2 mm offset in z and symmetric zero-mean form), when `lsq_pose` runs with free {tz}, then the recovered offset is 0.2 mm within 1e-12 m and the value equals the planted form range.
- **AC-2** Given fixture `asym_form.json` (one-sided spike), then LS and minimum-zone poses differ and the LS value exceeds the MZ value by the planted amount within 1e-9 m.
- **AC-3** Given a sphere fixture with free {rx, ry, rz} only, then `lsq_rank_deficient` (rotations of a sphere are unobservable).
- **AC-4** (determinism) AC-1 and AC-2 give bit-identical results on all CI platforms with the default (no random) start.

## Design

- **Registry command(s):** none (library).
- **Data structures:** `LsqResult { pose, value_m, rms_m, iterations, stop, seed: Option<u64> }`.
- **Algorithm:** parameterise the pose with `ALN.BFIT.CONSTR` locks (free DOFs from OPTS); Gauss-Newton or Levenberg-Marquardt through `KRN.SOLV.NLLS` with residuals d_i(x) from the oracle and Jacobian rows n_iᵀ·∂p_i/∂x; stop rules from NLLS; then `zone::solve_zone` with an empty free set at the LS pose.
- **Errors:** `lsq_not_converged`, `lsq_rank_deficient`, pass-through from DEV.
- **Determinism:** D2 (seeded only if a random restart is configured; D0 in the default configuration).
- **Performance budget:** 1 M points, six DOFs, 20 iterations < 3 s [unverified].
- **Crate:** `fid-gps` (module `prof::bestfit::lsq`).

## Test fixtures

Analytic plane, cylinder and sphere fixtures with planted offsets and asymmetric form (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel, KNP-1), add module `prof::bestfit::lsq`. Implement `lsq_pose(points, oracle: &dyn DeviationOracle, start: Pose, free: DofSet, cfg) -> Result<LsqResult, LsqError>` minimising Σ d_i² over the free DOFs using the DOF-lock parameterisation from the ALN best-fit constraint module and the nonlinear least-squares driver from `fid-solv` (locked DOFs held at exactly zero), re-projecting through the oracle each iteration; then compute the value by the fixed-pose zone path (`zone::solve_zone` with no free DOF) using the offset shape. Errors `lsq_not_converged`, `lsq_rank_deficient`. Return pose, value, RMS, iterations, stop reason, optional seed. Tests per the acceptance criteria (offset recovery, LS ≥ MZ property, rank deficiency, cross-platform bit identity). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, seeded counter-based RNG only, NaN is an error).

## Open questions

- Whether an LS-placed profile value may be reported as the characteristic value under any ASME or ISO row, or only as an informational value, is DMT content.

## Children

_Leaf: none._
