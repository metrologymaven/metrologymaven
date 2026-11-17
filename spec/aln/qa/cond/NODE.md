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
satisfies: [REQ-ALN.QA-002, REQ-ALN.QA-005, REQ-ALN.QA-007]
covers: []
depends_on: [ALN.QA.REPORT, ALN.QA.HIST, KRN.NUM]
side_effect: R0
determinism: D0
cli: [fid align get-diagnostics]
mcp: [align_get_diagnostics]
journal: [align.get_diagnostics@1]
updated: 2026-11-16
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
- **REQ-ALN.QA.COND-003** When the smallest singular value is below 1e-15 times the largest, the function shall attach `unobservable_dof` instead, naming the motion.
- **REQ-ALN.QA.COND-004** The weakest DoF shall be the right singular vector of the smallest singular value, named as a translation along or rotation about an axis of the alignment frame (through a stated point) when its largest normalised component exceeds 0.9 in magnitude, and as a generic twist otherwise.
- **REQ-ALN.QA.COND-005** `fid align get-diagnostics <alignment> --json` and MCP `align_get_diagnostics` shall return identical content (conditioning, warnings and convergence history) as R0 reads recorded only in the Activity Log.
- **REQ-ALN.QA.COND-006** The diagnostics shall be bit-identical across platforms for identical Jacobians (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/qa/cond-cases` (point sets on a full cylinder, a half cylinder, a long thin plane, a sphere and a generic freeform patch, with their point-to-plane Jacobians), when `diagnose` runs, then the full cylinder gives `unobservable_dof` naming "rotation about the cylinder axis", the sphere gives three unobservable rotations reported as the weakest one plus the count, and the freeform patch gives no warning.
- **AC-2** Given the long thin plane (1000 x 1 mm), then `ill_conditioned` is attached at the default threshold and the weakest DoF is named.
- **AC-3** Given a threshold of 1e12 on AC-2's case, then no `ill_conditioned` warning is attached.
- **AC-4** Given an alignment in `fixtures/aln/model/chain3`, when the CLI and MCP reads run, then the payloads are identical and include the convergence history field (empty for closed-form methods).
- **AC-5** (determinism) Given AC-1's cases on two platforms, then singular values and names are bit-identical.

## Design

- **Registry command:** `align.get_diagnostics@1` (R0). Param `alignment`. Result: `AlignmentDiagnostics { length_scale, singular_values, condition_number, weakest_dof, warnings, history: Option<ConvergenceHistory> }`.
- **Function:** `diagnose(j: &Matrix, params: &[ParamName], points_for_scale: &[P3]) -> Result<AlignmentDiagnostics, CondError>`; solvers call it once at the end and store the result with the residual report.
- **Algorithm:** column scaling, one-sided Jacobi SVD in fid-num (fixed sweep order), ratio tests, axis naming from the right singular vector after converting the scaled twist back to angular and linear parts.
- **Errors:** `empty_jacobian`, `nan_input`, `not_found`.
- **Determinism:** D0 under KNP-1.
- **Performance budget:** 1,000,000 x 6 Jacobian in under 100 ms [unverified until measured].
- **Crate:** `fid-align-solve` (diagnose) and `fid-align` (command); provisional names.

## Test fixtures

`fixtures/aln/qa/cond-cases`: seeded generator (seed 14) for the five geometries with 10,000 points each and their Jacobians. Project-generated, CC-BY-4.0.

## Build prompt

> Implement alignment conditioning diagnostics: `diagnose(jacobian, param_names, points_for_scale)` in kernel crate `fid-align-solve` and the command `align.get_diagnostics@1` in `fid-align` (class R0; CLI `fid align get-diagnostics <alignment> --json`; MCP `align_get_diagnostics`; Activity Log only).
> Requirements: scale rotation columns by L = RMS distance of used inputs from their centroid (recorded); singular values by fid-num one-sided Jacobi SVD with fixed sweep order; `ill_conditioned` when largest/smallest exceeds the threshold (default 1e8, settable); `unobservable_dof` when smallest is below 1e-15 of largest; weakest DoF from the right singular vector of the smallest value, named as translation along or rotation about an alignment-frame axis (through a stated point) when its largest normalised component exceeds 0.9, else a generic twist. The read returns conditioning, warnings and the stored convergence history (from the history module); CLI and MCP identical.
> Acceptance: fixture `fixtures/aln/qa/cond-cases` (seed 14): full cylinder names rotation about its axis as unobservable; sphere reports unobservable rotations; long thin plane is ill-conditioned at 1e8 but not at 1e12; freeform patch has no warning; CLI and MCP payloads identical using `fixtures/aln/model/chain3`; bit-identical results across platforms.
> Constraints: KNP-1 (no FMA, fid-num SVD and sqrt, fixed-order loops, no std HashMap/HashSet, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- The 1e8 default threshold needs calibration on `ALN.BFIT.BENCH` and `ALN.PRE.CORPUS` cases.

## Children

_Leaf: none._
