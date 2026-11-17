---
id: ALN.DATUM.ELEM.LEVELS
title: Priority-level element solve with DoF projectors
level: 4
parent: ALN.DATUM.ELEM
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM.ELEM-001, REQ-ALN.DATUM.ELEM-002, REQ-ALN.DATUM.ELEM-003, REQ-ALN.DATUM.ELEM-004, REQ-ALN.DATUM.ELEM-005, REQ-ALN.DATUM.ELEM-006]
covers: []
depends_on: [ALN.BFIT.FEAT.DEV, ALN.DATUM.DOF, ALN.BFIT.STOP, ALN.QA.REPORT, ALN.QA.COND, GEO.FRAME.XFORM, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.DATUM.ELEM.LEVELS: Priority-level element solve with DoF projectors

## Purpose

The pure R1 entry `solve_elements`: pairs of measured and nominal elements are grouped by priority; each level is fitted by weighted least squares on the pair deviations from `ALN.BFIT.FEAT.DEV`, with the pose update restricted to the free subspace that `ALN.DATUM.DOF` leaves after the higher levels, so a lower level never disturbs what a higher level fixed.

## Scope

**In:** priority grouping (1 is highest); per-level Gauss-Newton with steps restricted to the free subspace; screw-motion (exponential map) pose updates; the unconstrained-DoF policy via `ALN.DATUM.DOF`; per-pair nominal and effective DoF report; residual report and diagnostics.
**Out:** pair residual definitions (`ALN.BFIT.FEAT.DEV`); subspace bases and projectors (`ALN.DATUM.DOF`); the command (`ALN.DATUM.ELEM.CMD`); datum-system alignment (`ALN.DATUM.SYS`).

## Requirements

- **REQ-ALN.DATUM.ELEM.LEVELS-001** The solver shall group pairs by integer priority, process groups from priority 1 upward, and within a group minimise the weighted sum of squared pair deviations.
- **REQ-ALN.DATUM.ELEM.LEVELS-002** For each level, the solver shall obtain from `ALN.DATUM.DOF` an orthonormal basis Q of the free subspace left by all higher levels, evaluated at the higher-level measured features in their solved positions, and restrict every step to twists Q y.
- **REQ-ALN.DATUM.ELEM.LEVELS-003** The pose shall be updated by the exponential map of the restricted twist, so that after all levels every DoF fixed by a higher level differs from its value after that level by at most 1e-12 m or 1e-12 rad (measured as that level's deviation components).
- **REQ-ALN.DATUM.ELEM.LEVELS-004** Within a level whose system is rank deficient in the free subspace, the step shall be the minimum-norm solution, so no motion is taken in directions the level does not determine.
- **REQ-ALN.DATUM.ELEM.LEVELS-005** The solver shall report for each pair its nominal and effective constrained DoF counts and any `redundant_pair` warning from `ALN.DATUM.DOF`.
- **REQ-ALN.DATUM.ELEM.LEVELS-006** When DoF remain free after all levels, the solver shall apply the policy: `keep_parent` holds them at the start (parent) pose; `reject` fails with `underconstrained` listing them.
- **REQ-ALN.DATUM.ELEM.LEVELS-007** If a pair's measured and nominal types differ, then the solver shall fail with `pair_type_mismatch` naming the pair before any iteration.
- **REQ-ALN.DATUM.ELEM.LEVELS-008** Given noise-free fixtures with a known transform, the recovered pose shall match within 1e-9 m and 1e-9 rad [unverified].
- **REQ-ALN.DATUM.ELEM.LEVELS-009** Each level shall stop through `ALN.BFIT.STOP`; if any level stops `stopped`, the output status shall be `not_converged` naming the level.
- **REQ-ALN.DATUM.ELEM.LEVELS-010** The solver shall be single-threaded under KNP-1 and declare class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/datum/elem/plane_cyl_point.json` (plane priority 1, cylinder priority 2, point priority 3; known transform 2 mm and 1 degree), when `solve_elements` runs, then the pose matches within 1e-9 m and 1e-9 rad and the effective counts are 3, 2, 1.
- **AC-2** Given the same fixture with 10 µm noise (seed 13), then the plane deviation components after level 1 and after level 3 differ by at most 1e-12.
- **AC-3** Given two planes at priority 1 (perpendicular) and a sphere at priority 2, then both planes are fitted together and the sphere moves only along the remaining line.
- **AC-4** (error path) Given a plane paired with a cylinder nominal, then the solver fails with `pair_type_mismatch{index}`.
- **AC-5** Given only a plane and policy `reject`, then the solver fails with `underconstrained` naming two translations and one rotation; with `keep_parent`, those components equal the parent pose within 1e-15.
- **AC-6** Given a second plane parallel to the first at priority 2, then that pair has effective count 0 and a `redundant_pair` warning.

## Design

- **Registry command(s):** none; R1 library entry `solve_elements(pairs: &[PrioritisedPair], policy: Unconstrained, start: &Rigid, stop: &StopCriteria) -> Result<ElemOutput, ElemError>`.
- **Data structures:** `PrioritisedPair { pair: FeaturePair, priority: u32, weight: f64, length_scale: Option<f64> }`; `ElemOutput { pose, status, per_pair: [{ nominal_dof, effective_dof, warning }], free_dof: Vec<DofName>, report, history, diagnostics, algorithm_id: "align.elements.priority_lsq@1.0.0" }`.
- **Algorithm:** validate types (DEV); for level k: Q_k from `accumulate` on higher-level features in current position; iterate GN with J_k Q_k, minimum-norm QR/SVD step y, twist Q_k y, pose = exp(twist) * pose; after all levels apply policy. Exponential map through `fid-num` transcendentals.
- **Errors:** `pair_type_mismatch`, `underconstrained`, `unsupported_feature_type`, `invalid_weight`, `nan_in_iteration`.
- **Determinism:** D0.
- **Performance budget:** 20 pairs over 3 levels in under 2 ms [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

Analytic element sets generated in-repo with a seeded script (CC0): `plane_cyl_point.json`, `two_planes_sphere.json`, `parallel_planes.json`.

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `solve_elements(pairs, policy, start, stop) -> Result<ElemOutput, ElemError>`. Validate pair types with the deviation module (ALN.BFIT.FEAT.DEV; `pair_type_mismatch`). Group pairs by priority (1 highest). For each level, get an orthonormal free-subspace basis Q from `accumulate` (ALN.DATUM.DOF) over the higher-level measured features in their solved positions; run Gauss-Newton on the level's weighted deviations with Jacobian J·Q, taking the minimum-norm step, updating the pose by the exponential map of Q·y; stop each level with `StopMonitor` (ALN.BFIT.STOP). After all levels, apply `keep_parent` (hold free DoF at the start pose) or `reject` (`underconstrained`). Report per-pair nominal and effective DoF and `redundant_pair` warnings, a residual report, history and diagnostics. Algorithm ID `align.elements.priority_lsq@1.0.0`. Tests: AC-1..AC-6, including the 1e-12 invariance check of higher levels. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error), single-threaded D0.

## Open questions

- Same-priority pairs are fitted together by least squares; GOM's and PolyWorks' handling of equal priorities is not confirmed [unverified].

## Children

_Leaf: none._
