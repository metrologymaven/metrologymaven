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
built_in: [fid-kunc]
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
- **Build notes (fid-kunc `inst::arm`, algorithm `unc.inst.arm_cov@1.0.0`, card `crates/fid-kunc/cards/unc.inst.arm_cov@1.0.0.toml`).** Choices made where the Design is silent, for review:
  - `arm_cov(&ArmParams, p: [f64; 3], u: Option<[f64; 3]>)` returns `ArmCov`, which wraps the shared `Cov3` with the `sigma_lat`, `sigma_ax` and axial factor used, the unit direction used and flags. `ArmParams::axial_factor` is an `Option` so an absent factor (1) stays distinguishable from a declared one. Plain arrays stand in for the Build prompt's `Vec3`, as no shared `Vec3` type exists in the kernel crates.
  - REQ-002: `u` is always normalised, not only when `||u||` differs from 1 by more than 1e-9; using a near-unit `u` as given would put a relative error of up to 2e-9 into `sigma_ax^2`, above AC-1's 1e-14. A `u` outside the 1e-9 window is flagged `direction_off_unit`. Both norm tests (below 1e-12, outside the 1e-9 window) are decided exactly in fixed-width integer arithmetic, with no rounding (subnormals included).
  - Without a direction, an `axial_factor` other than 1 is refused as `invalid_direction{u, missing}`: the Scope's isotropic fallback would silently drop the declared axial term. An absent factor, or exactly 1, gives `sigma_lat^2 I`.
  - Error codes beyond the Design's two: `invalid_observation{p_x|p_y|p_z, not_finite}` for a non-finite point, `invalid_direction{u_x|u_y|u_z, not_finite}` for a non-finite direction, and `numeric` for an overflow (point norm, `sigma_lat`, `sigma_ax` or an entry).
  - `sigma_ax^2 - sigma_lat^2` is evaluated as `((f - 1) sigma_lat)((f + 1) sigma_lat)`, which is algebraically equal but has no cancellation near f = 1. Norms use an exact power-of-two scaling and the correctly rounded square root, so they neither overflow nor underflow on the way. The stated relative Frobenius bound (37 * 2^-53) holds unless the result is flagged `scale_below_validated` (0 < sigma_lat < 2^-480, or a subnormal point norm) or `zero_covariance`. `axial_factor = 0` with a direction is accepted (the eigenvalues are still non-negative) and flagged `axial_zero`.
  - AC-2 is tested as: every off-diagonal entry is exactly +0, and each diagonal entry is the correctly rounded square of the returned `sigma_lat`. For points and parameters where `sigma_lat` is exact (dyadic cases), the entry is also within 1 ulp of the exact `sigma_lat^2`. With a general `||p||`, `sigma_lat` itself carries a few ulp of rounding, so 1 ulp against the exact value cannot hold for every input.
  - The 50-digit reference of AC-1 is computed with the Python standard library `decimal` module rather than mpmath, so the oracle has no third-party dependency (as for SPH). The bit patterns are compared with a Python model of the documented order on every CI target.
  - The performance budget (<= 100 ns per point) is not measured here and stays [unverified]; the exact norm tests cost a few hundred integer operations per directed point.

## Children

_Leaf: none._
