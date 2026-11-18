---
id: UNC.PROP.LPU.ASET
title: Active-set sensitivities and non-smoothness flag for certified fits
level: 4
parent: UNC.PROP.LPU
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.LPU-001, REQ-UNC.PROP.LPU-004]
covers: []
depends_on: [UNC.PROP.LPU.IMPL, KRN.SOLV.CERT.CT, KRN.NUM.LA.QR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.LPU.ASET: Active-set sensitivities and non-smoothness flag for certified fits

## Purpose

Minimum-zone, minimum-circumscribed, maximum-inscribed and tangential fits are defined, at a certified solution, by a set of active constraints. Locally the solution is a smooth function of the active points, so sensitivities follow from differentiating the active equations. This is valid only while the active set does not change. This node computes those sensitivities from the kernel certificate. It returns kink hints for inactive constraints close to becoming active and for active constraints whose multipliers are close to zero, so that `UNC.PROP.LPU.COMB` can flag `lpu_not_applicable`.

## Scope

**In:**
- Sensitivities from the certificate's active set: differentiate the active constraint equations (residual of each active point equals plus or minus the zone value, or the envelope condition) with respect to parameters, zone value and point coordinates, and solve
- Kink hints: smallest inactive-constraint gap and smallest active multiplier, each with the point index
- Providers for the certified fit IDs of P1a primitives (line, plane, circle, sphere, cylinder)

**Out:**
- Least-squares fits (owned by `UNC.PROP.LPU.IMPL`, whose generic residual functions this node reuses)
- Certificates (owned by `KRN.SOLV.CERT`)
- Choosing MCM for non-smooth operators (owned by `UNC.PROP.SELECT`)

## Requirements

- **REQ-UNC.PROP.LPU.ASET-001** For a certified fit, the provider shall return d(parameters and zone value)/d(point coordinates) by solving the differentiated active-constraint system, with zero sensitivity for inactive points.
- **REQ-UNC.PROP.LPU.ASET-002** The provider shall return a kink hint carrying the smallest gap of an inactive constraint (in metres) and the smallest active multiplier, each with its point index.
- **REQ-UNC.PROP.LPU.ASET-003** If the certificate is absent, flagged `local_optimum_only`, or its active system is singular, then the provider shall return `no_derivative` with the reason.
- **REQ-UNC.PROP.LPU.ASET-004** On a constructed minimum-zone circle with 4 well-separated active points, the sensitivities shall match the closed-form derivative of the equioscillation solution to a relative 1e-10 [unverified].

## Acceptance criteria

- **AC-1** Given `fixtures/unc/lpu/aset/mz_circle_4pt.json` (certified minimum-zone circle with closed-form derivatives), when the provider runs, then the sensitivities match to relative 1e-10 and inactive points have zero rows.
- **AC-2** Given `fixtures/unc/lpu/aset/near_switch.json` where an inactive point lies 2e-9 m inside the zone, then the kink hint names that point with gap 2e-9 m.
- **AC-3** (error path) Given a certificate flagged `local_optimum_only`, then `no_derivative{reason: "local_optimum_only"}`.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32, then outputs are byte-identical.
- **AC-H1** Hypothesis: active-set sensitivities of minimum-zone, minimum-circumscribed and maximum-inscribed fits achieve maximum relative entry error ≤ 1e-10 [unverified] against the closed-form derivative of the constructed solution on the RDL tier G minimum-zone family `UNC.RDL.GEN.MZEQ` in the frozen `rdl-release-<semver>` (cases whose smallest active multiplier and smallest inactive gap exceed the kink thresholds), measured by the maximum relative error over all Jacobian entries per case; falsified if any such case exceeds the threshold. Until `UNC.RDL.GEN.MZEQ` is frozen, only `mz_circle_4pt.json` exercises this [gap].
- **AC-H2** Hypothesis: the kink hint predicts where LPU is invalid: on the same `UNC.RDL.GEN.MZEQ` cases with input uncertainty u, a kink margin below `kink_factor` · u coincides with an active-set change in more than 5 % [unverified] of MCM trials (`UNC.PROP.MCM`) for at least 95 % [unverified] of cases, measured by cross-tabulating the hint against the trial-level active-set change count; falsified if the agreement is below 95 % or any case with a margin above 10 · `kink_factor` · u [unverified] shows such a change rate.

## Design

- **Registry command(s):** none; providers registered in `TangentTable`.
- **Data structures:** `AsetOut { dtheta_dx: Mat, kink: Kink { min_gap, gap_point, min_multiplier, mult_point } }`.
- **Algorithm:** read active set and multipliers from the certificate; build the active system Jacobian from the generic residual functions of `UNC.PROP.LPU.IMPL` (with the zone value as an extra unknown); solve by pivoted QR; compute gaps of inactive points from the fit residuals in point order.
- **Errors:** `no_derivative{operator, reason}`.
- **Determinism:** D0.
- **Performance budget:** 10 000-point cylinder certificate in under 20 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::lpu::aset`; KNP-1 applies).

## Test fixtures

`fixtures/unc/lpu/aset/`: constructed equioscillation cases with closed-form derivatives from a committed generator script (seeded; CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::lpu::aset`, implement tangent providers for certified minimum-zone, minimum-circumscribed, maximum-inscribed and tangential fits of line, plane, circle, sphere and cylinder. From the kernel `Certificate` (active set, multipliers), build the differentiated active-constraint system (parameters plus zone value as unknowns) with the generic residual functions from `prop::lpu::implicit`, solve by pivoted QR from `fid-num`, and return sensitivities with zero rows for inactive points. Return a kink hint with the smallest inactive gap and smallest active multiplier and their point indices. Return `no_derivative{reason}` for a missing certificate, `local_optimum_only` or a singular system. Tests AC-1..AC-4 with `fixtures/unc/lpu/aset/`. Constraints: KNP-1, no finite differences, CONSTITUTION 8.

## Open questions

- None.

## Children

_Leaf: none._
