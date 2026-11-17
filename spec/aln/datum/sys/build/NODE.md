---
id: ALN.DATUM.SYS.BUILD
title: Datum-system frame build through GDT.DRF and unconstrained-DoF policy
level: 4
parent: ALN.DATUM.SYS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.DATUM.SYS-003, REQ-ALN.DATUM.SYS-005, REQ-ALN.DATUM.SYS-006, REQ-ALN.DATUM.SYS-007]
covers: []
depends_on: [ALN.DATUM.SYS.GATE, GDT.DRF.SIM, GDT.DRF.BUILD, ALN.QA.REPORT, GEO.FRAME.XFORM, KRN.NUM]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.DATUM.SYS.BUILD: Datum-system frame build through GDT.DRF and unconstrained-DoF policy

## Purpose

Turns a resolved datum chain and the measured datum features into an alignment transform. It simulates each datum feature with the simulator the DMT row names (`GDT.DRF.SIM`), builds the measured DRF and the nominal DRF through `GDT.DRF.BUILD`'s pure construction (the single owner of the DoF table), maps one onto the other, and applies the user's policy to the DoF the DRF leaves free.

## Scope

**In:** per-datum simulation calls; measured and nominal DRF construction via GDT.DRF; the transform from measured DRF to nominal DRF; reading the DoF table and free DoF from the DRF; `keep_parent` and `reject` on free DoF; the precedence post-check; per-datum residual entries; the effective determinism class of the run.
**Out:** the DoF table, precedence logic and degeneracy detection (`GDT.DRF.BUILD`); simulators and rocking candidates (`GDT.DRF.SIM`); resolution (`ALN.DATUM.SYS.GATE`); element wrapping (`ALN.DATUM.SYS.CMD`); priority-ordered element alignment without a dialect (`ALN.DATUM.ELEM`).

## Requirements

- **REQ-ALN.DATUM.SYS.BUILD-001** For each datum in precedence order, the function shall simulate the measured datum feature with the simulator named in its resolved chain and pass the simulated datums, in that order, to GDT.DRF's construction to obtain the measured DRF.
- **REQ-ALN.DATUM.SYS.BUILD-002** The function shall build the nominal DRF from the nominal datum features by the same construction, and the alignment transform shall be T = F_nominal · F_measured^-1, where F maps DRF coordinates to the respective frame.
- **REQ-ALN.DATUM.SYS.BUILD-003** The function shall take the per-datum constrained DoF and the free DoF from the measured DRF's DoF table, and shall not compute DoF accounting of its own.
- **REQ-ALN.DATUM.SYS.BUILD-004** When free DoF remain and the policy is `keep_parent`, the function shall replace the components of T along the free DoF (expressed in DRF axes at the DRF origin) by the parent pose's components; when the policy is `reject`, it shall fail with `underconstrained` listing them.
- **REQ-ALN.DATUM.SYS.BUILD-005** After the build, the function shall verify that adding each lower-precedence datum changed no DoF constrained by a higher-precedence datum by more than 1e-12 m or 1e-12 rad, and otherwise fail with `precedence_violation` (an internal-consistency error that must never occur).
- **REQ-ALN.DATUM.SYS.BUILD-006** Given the `ALN.DATUM.SYS.FIXT` noise-free ASME fixtures, the recovered transform shall match the known one within 1e-9 m and 1e-9 rad [unverified].
- **REQ-ALN.DATUM.SYS.BUILD-007** The function shall return the effective determinism class of the run: D0 when every simulator used is closed-form or least squares, D3 when any is an iterative envelope fit.
- **REQ-ALN.DATUM.SYS.BUILD-008** GDT.DRF errors (`degenerate_datum_pair`, simulator errors, `unsupported_datum_type_p1a`) shall pass through unchanged with the datum labels.

## Acceptance criteria

- **AC-1** Given `ALN.DATUM.SYS.FIXT` case `block_abc` (noise-free, known transform), when `build_datum_alignment` runs, then the transform matches within 1e-9 m and 1e-9 rad and the free DoF list is empty.
- **AC-2** Given case `plate_bore` with policy `keep_parent` and a parent pose rotated 5 degrees about the bore axis, then the free DoF is {rz} and the rotation about the bore axis equals the parent's within 1e-12 rad.
- **AC-3** Given `plate_bore` with policy `reject`, then the call fails with `underconstrained{[rz]}`.
- **AC-4** Given case `block_abc_noisy` (seed 17), then the primary's constrained components are identical (within 1e-12) with and without datums B and C.
- **AC-5** (error path) Given a secondary plane parallel to the primary, then `degenerate_datum_pair{A,B}` from GDT.DRF is returned.
- **AC-6** Given LSQ simulators only, then the effective class is D0.

## Design

- **Registry command(s):** none; R1 library function `build_datum_alignment(chain: &ResolvedDatumChain, measured: &[DatumFeatureData], nominal: &[NominalDatumFeature], policy: Unconstrained, parent: &Rigid) -> Result<DatumBuildOutput, BuildError>`.
- **Data structures:** `DatumBuildOutput { transform, drf_measured: Drf, drf_nominal: Drf, free_dof: DofSet, per_datum: [{ label, simulator_alg, constrained: DofSet, sim_residual_summary }], effective_class: D0 | D3, report: ResidualReport }`.
- **Algorithm:** simulate each datum (GDT.DRF.SIM `simulate`); construct both DRFs (GDT.DRF.BUILD pure construction, the R1 path of `gdt.build_drf@1`); T = F_nom · F_meas^-1; free-DoF replacement via the logarithm of T^-1·T_parent projected on the free DoF coordinates and exponentiated; post-check per precedence step.
- **Errors:** `underconstrained`, `precedence_violation`, GDT.DRF errors passed through.
- **Determinism:** declared D3 (the weakest class a supported simulator can produce); the effective class is recorded per run.
- **Performance budget:** under 5 ms for three datums of 10,000 points each, excluding envelope simulators [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name), calling `fid-gps`.

## Test fixtures

Cases from `ALN.DATUM.SYS.FIXT` (`block_abc`, `block_abc_noisy`, `plate_bore`, `shaft_face`), CC0.

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `build_datum_alignment(chain, measured, nominal, policy, parent) -> Result<DatumBuildOutput, BuildError>`. For each datum in precedence order call GDT's `simulate` (GDT.DRF.SIM, crate `fid-gps`) with the simulator named by the resolved chain; pass the simulated datums to GDT's pure DRF construction (GDT.DRF.BUILD) to get the measured DRF; build the nominal DRF from nominal datum features the same way. Transform T = F_nominal · F_measured^-1. Take constrained and free DoF only from the DRF's DoF table (do not re-implement DoF accounting). Policy `keep_parent`: replace T's components along free DoF (DRF axes, DRF origin) with the parent pose's, via log/exp in `fid-num`; `reject`: fail `underconstrained`. Post-check that each lower datum changed no higher-constrained DoF by more than 1e-12 (`precedence_violation`). Pass GDT errors through. Return the effective determinism class (D0 if all simulators are closed-form or LSQ, else D3) and a residual report with per-datum simulator residual summaries. Tests: AC-1..AC-6. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- GDT.DRF.SIM is still open; its `simulate` signature here follows GDT.DRF's Interfaces section and may need adjusting when SIM is expanded.
- Layering: depends on GDT.DRF (see the parent's open questions).

## Children

_Leaf: none._
