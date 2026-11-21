---
id: ALN.QA.COND
title: Conditioning and weak-DoF diagnostics
level: 3
parent: ALN.QA
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-align-solve, fid-align]
built_pr: 411
satisfies: [REQ-ALN.QA-002, REQ-ALN.QA-005, REQ-ALN.QA-007]
covers: []
depends_on: [ALN.QA.REPORT, ALN.QA.HIST, KRN.NUM]
side_effect: R0
determinism: D0
cli: [fid align get-diagnostics]
mcp: [align_get_diagnostics]
journal: [align.get_diagnostics@1]
updated: 2026-11-21
---

# ALN.QA.COND: Conditioning and weak-DoF diagnostics

## Purpose

Detects alignments whose data barely constrains some motion, for example a rotation about a cylinder's axis or a slide along a long flat plane, and names the weakest DoF. Solvers pass their final (free-parameter) Jacobian or sensitivity matrix; this leaf computes a length-scaled condition number and singular structure, attaches warnings, and offers `align.get_diagnostics@1`, which returns these diagnostics together with the convergence history from `ALN.QA.HIST`.

## Scope

**In:**
- `diagnose(jacobian, parameter_names, length_scale) -> AlignmentDiagnostics`
- Length scaling of rotation columns, condition number, singular values, weakest-DoF naming
- Warnings `ill_conditioned` and `unobservable_dof`, threshold settable with default
- `align.get_diagnostics@1` (R0)

**Out** (and which node owns it instead):
- Residual statistics (`ALN.QA.REPORT`); convergence history type (`ALN.QA.HIST`)
- Refusal policies based on conditioning (solvers and `LVM.MOVE`)
- Transform uncertainty (`UNC.XFM`)

## Requirements

- **REQ-ALN.QA.COND-001** The function shall scale each rotation column of the Jacobian by a characteristic length L (the RMS distance of the used inputs from their centroid, recorded) and compute the singular values with fid-num.
- **REQ-ALN.QA.COND-002** When the ratio of largest to smallest singular value exceeds the threshold (default 1e8 [unverified]), the function shall attach `ill_conditioned` with the condition number and the weakest DoF.
- **REQ-ALN.QA.COND-003** When the smallest singular value is below rho times the largest, with rho = max(1e-15, 8 u n (1 + |c| / L)) (u = 2^-53, n the number of free DoFs, |c| the distance of the centroid from the frame origin, L the length scale; the constant 8 [unverified]), the function shall attach `unobservable_dof` instead, naming the motion.
- **REQ-ALN.QA.COND-004** The weakest DoF shall be the right singular vector of the smallest singular value, named as a translation along or rotation about an axis of the alignment frame (through the centroid when the two perpendicular translations are free, else the frame origin) when its largest normalised component exceeds 0.9 in magnitude, and as a generic twist otherwise.
- **REQ-ALN.QA.COND-005** `fid align get-diagnostics <alignment> --json` and MCP `align_get_diagnostics` shall return identical content (conditioning, warnings and convergence history) as R0 reads recorded only in the Activity Log.
- **REQ-ALN.QA.COND-006** The diagnostics shall be bit-identical across platforms for identical Jacobians (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/qa/cond-cases` (point sets on a full cylinder, a half cylinder, a long thin plane, a sphere and a generic freeform patch, with their point-to-plane Jacobians), when `diagnose` runs, then the full cylinder (axis through the frame origin, `tz` locked, because a bare cylinder's axial slide ties exactly with the rotation about its axis) gives `unobservable_dof` naming the rotation about the cylinder axis, the same cylinder 50 mm off the origin also names the rotation about z, the sphere gives three unobservable rotations reported as the weakest one plus the count (the three are degenerate, so the weakest is named "generic twist"), a sphere 2 m from the origin also gives `unobservable_dof` with count 3, and the freeform patch gives no warning.
- **AC-2** Given the long thin plane (1000 x 1 mm) with a stated form error of its normals (an ideal plane's in-plane DoFs are exactly unobservable), whose condition number lies between 1e8 and 1e12, then `ill_conditioned` is attached at the default threshold and the weakest DoF is named.
- **AC-3** Given a threshold of 1e12 on AC-2's case, then no `ill_conditioned` warning is attached.
- **AC-4** Given an alignment in `fixtures/aln/model/chain3`, when the CLI and MCP reads run, then the payloads are identical and include the convergence history field (empty for closed-form methods).
- **AC-5** (determinism) Given AC-1's cases on two platforms, then singular values and names are bit-identical.

## Design

- **Registry command:** `align.get_diagnostics@1` (R0). Param `alignment`. Result: `AlignmentDiagnostics { length_scale, singular_values, condition_number, weakest_dof, warnings, history: Option<ConvergenceHistory> }`.
- **Function:** `diagnose(j: &Matrix, params: &[ParamName], points_for_scale: &[P3]) -> Result<AlignmentDiagnostics, CondError>`; solvers call it once at the end and store the result with the residual report.
- **Algorithm:** column scaling (each rotation pivots on the centroid when the two translations perpendicular to its axis are free: rx needs ty and tz, ry needs tx and tz, rz needs tx and ty; otherwise on the frame origin), a fixed-block Householder QR in fid-num (its matrices hold at most 4096 rows) and the one-sided Jacobi SVD in fid-num (fixed sweep order) of the triangle, ratio tests, axis naming from the right singular vector after converting the scaled twist back to angular and linear parts. The pinned rules are in `fid_align_solve::cond`.
- **Errors:** `empty_jacobian`, `nan_input`, `not_found`.
- **Determinism:** D0 under KNP-1.
- **Performance budget:** 1,000,000 x 6 Jacobian in under 100 ms [unverified until measured].
- **Crate:** `fid-align-solve` (diagnose) and `fid-align` (command); provisional names.

## Test fixtures

`fixtures/aln/qa/cond-cases`: seeded generators (seed 14 for the five geometries with 10,000 points each; seed 15 for a 50-point freeform patch; seed 16 for the 50 mm offset cylinder and the sphere 2 m from the origin, 4,000 points each) and their Jacobians, with a Python reference that also cross-checks the QR route against a direct SVD. Project-generated, CC-BY-4.0.

## Build prompt

> Implement alignment conditioning diagnostics: `diagnose(jacobian, param_names, points_for_scale)` in kernel crate `fid-align-solve` and the command `align.get_diagnostics@1` in `fid-align` (class R0; CLI `fid align get-diagnostics <alignment> --json`; MCP `align_get_diagnostics`; Activity Log only).
> Requirements: scale rotation columns by L = RMS distance of used inputs from their centroid (recorded); singular values by fid-num one-sided Jacobi SVD with fixed sweep order; `ill_conditioned` when largest/smallest exceeds the threshold (default 1e8, settable); `unobservable_dof` when smallest is below max(1e-15, 8 u n (1 + |c| / L)) of largest; weakest DoF from the right singular vector of the smallest value, named as translation along or rotation about an alignment-frame axis (through the centroid when the two perpendicular translations are free, else the frame origin) when its largest normalised component exceeds 0.9, else a generic twist. The read returns conditioning, warnings and the stored convergence history (from the history module); CLI and MCP identical.
> Acceptance: fixture `fixtures/aln/qa/cond-cases` (seed 14): full cylinder (tz locked) and the 50 mm offset cylinder name rotation about z as unobservable; sphere and the sphere 2 m off report three unobservable rotations; long thin plane with form error is ill-conditioned at 1e8 but not at 1e12; freeform patch has no warning; CLI and MCP payloads identical using `fixtures/aln/model/chain3`; bit-identical results across platforms.
> Constraints: KNP-1 (no FMA, fid-num SVD and sqrt, fixed-order loops, no std HashMap/HashSet, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- The 1e8 default threshold needs calibration on `ALN.BFIT.BENCH` and `ALN.PRE.CORPUS` cases.
- The guard constant 8 in REQ-003's unobservable ratio needs calibration on the same cases (parts far from the frame origin, many points).

## Change log

- 2026-11-21 (review of #411, maintainer decision on REQ-003): AC-1 states the cylinder with `tz` locked and its axis through the frame origin, adds the 50 mm offset cylinder and the sphere 2 m from the origin, and records that the sphere's weakest DoF is "generic twist". AC-2 states the plane's form error and the 1e8 to 1e12 condition-number range. REQ-003's ratio becomes max(1e-15, 8 u n (1 + |c| / L)). The Design records the per-rotation pivot and the QR step. No REQ ID was renumbered.

## Children

_Leaf: none._
