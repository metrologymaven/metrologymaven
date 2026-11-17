---
id: UNC.INST.PORT.ARM
title: Articulated-arm point covariance without joint data
level: 4
parent: UNC.INST.PORT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.PORT-001]
covers: [SA.UNC.01]
depends_on: [KRN.NUM, UNC.MODEL.DIST]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.INST.PORT.ARM: Articulated-arm point covariance without joint data

## Purpose

Kernel function for articulated-arm probing points when joint angles are not available: an isotropic probing term plus a term growing with the point's distance from the arm base, optionally split into axial and lateral parts along the probe direction. Pose-dependent (joint-angle) models wait until HAL confirms that drivers expose joint data.

## Scope

**In:** parameters `sigma_p` (m), `sigma_d1` (m/m), optional `axial_factor` (dimensionless, default 1); base origin at the instrument frame origin; covariance = σ_lat²·I + (σ_ax² − σ_lat²)·u·uᵀ with σ_lat = sigma_p + sigma_d1·‖p‖ and σ_ax = axial_factor·σ_lat, u the unit probe direction (or isotropic when absent).
**Out:** joint-angle models (not in this leaf); range check and PSD check (owned by `UNC.INST.PORT.EVAL`).

## Requirements

- **REQ-UNC.INST.PORT.ARM-001** The function shall return the covariance in m² defined in Scope for a point p and optional probe direction u.
- **REQ-UNC.INST.PORT.ARM-002** If u is given and its norm differs from 1 by more than 1e-9, then the function shall normalise it; if its norm is below 1e-12, then it shall fail with `invalid_direction`.
- **REQ-UNC.INST.PORT.ARM-003** If any parameter is negative or non-finite, then the function shall fail with `invalid_parameter`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/port/arm_cases.json` (100 seeded cases, seed `0x41524D`), then each result matches the reference within 1e-14 relative.
- **AC-2** Given no probe direction, then the result is σ²·I to ≤ 1 ulp per entry.
- **AC-3** Given u = (0, 0, 1) and axial_factor 2, then the eigenvalues are σ_lat², σ_lat², 4σ_lat² within 1e-15 relative.
- **AC-4** (error paths) zero direction → `invalid_direction`; `sigma_p = -1e-6` → `invalid_parameter`.

## Design

- **Registry command(s):** none; kernel algorithm `unc.inst.arm_cov@1.0.0`.
- **Algorithm:** norm and outer product in fixed order through `fid-num`.
- **Errors:** `invalid_parameter`, `invalid_direction`.
- **Determinism:** D0.
- **Performance budget:** ≤ 100 ns per point [unverified until measured].
- **Crate:** `fid-kunc` (module `inst`).

## Test fixtures

`fixtures/unc/inst/port/arm_cases.json` with mpmath references (CC-BY-4.0); invented parameter values.

## Build prompt

> In crate `fid-kunc` (kernel; KNP-1; no panic or `unwrap`), add `inst::arm_cov(params: ArmParams, p: Vec3, u: Option<Vec3>) -> Result<Cov3, InstError>`, algorithm ID `unc.inst.arm_cov@1.0.0`: σ_lat = sigma_p + sigma_d1·‖p‖, σ_ax = axial_factor·σ_lat (default factor 1), Σ = σ_lat²·I + (σ_ax² − σ_lat²)·u·uᵀ (isotropic without u). Normalise u (fail `invalid_direction` if ‖u‖ < 1e-12); `invalid_parameter` for negative or non-finite inputs. Tests AC-1..AC-4 with `fixtures/unc/inst/port/arm_cases.json`.

## Open questions

- Arm pose-dependent models need joint-angle data from HAL; whether drivers expose them is unconfirmed. A later sibling or version adds them.
- Whether a distance-proportional term is a sound default for arms needs an instrument-metrology review [unverified].

## Children

_Leaf: none._
