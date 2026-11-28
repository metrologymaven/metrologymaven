---
id: UNC.INST.PORT.SPH
title: Spherical observation covariance for trackers and total stations
level: 4
parent: UNC.INST.PORT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-kunc]
built_pr: 687
satisfies: [REQ-UNC.INST.PORT-001, REQ-UNC.INST.PORT-002]
covers: [SA.UNC.01]
depends_on: [KRN.NUM, UNC.MODEL.DIST]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.INST.PORT.SPH: Spherical observation covariance for trackers and total stations

## Purpose

Kernel function for instruments that measure range and two angles: the Cartesian 3x3 covariance is J·C·Jᵀ, where C is the declared covariance of (range, azimuth, elevation) and J the Jacobian of the spherical-to-Cartesian map at the observation. Range standard uncertainty may have a constant and a range-proportional part.

## Scope

**In:** parameters `sigma_r0` (m), `sigma_r1` (m/m), `sigma_az` (rad), `sigma_el` (rad), optional correlations `rho_r_az`, `rho_r_el`, `rho_az_el`; convention x = r·cos el·cos az, y = r·cos el·sin az, z = r·sin el; Jacobian in closed form.
**Out:** range check, parameter extraction and PSD check (owned by `UNC.INST.PORT.EVAL`).

## Requirements

- **REQ-UNC.INST.PORT.SPH-001** The function shall compute σ_r = sigma_r0 + sigma_r1·r and return J·C·Jᵀ with J the closed-form Jacobian of the stated convention, in m².
- **REQ-UNC.INST.PORT.SPH-002** The result shall agree with a 50-digit reference to within 1e-14 relative in Frobenius norm for observations with 0.1 m ≤ r ≤ 100 m and |el| ≤ 1.5 rad.
- **REQ-UNC.INST.PORT.SPH-003** If any σ is negative or non-finite, or any correlation lies outside [−1, 1], then the function shall fail with `invalid_parameter`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/port/sph_cases.json` (200 seeded cases, seed `0x535048`), when computed, then each meets REQ-002 against the mpmath reference.
- **AC-2** Given az = el = 0, r = 10 m and zero correlations, then the covariance is diag(σ_r², (r·sigma_az)², (r·sigma_el)²) exactly up to rounding (≤ 2 ulp per entry).
- **AC-3** (error path) Given `rho_az_el = 1.2`, then `invalid_parameter`.
- **AC-4** Outputs are bit-identical on wasm32, x86-64 and ARM64.

## Design

- **Registry command(s):** none; kernel algorithm `unc.inst.sph_cov@1.0.0`.
- **Data structures:** `SphParams`, `Cov3`.
- **Algorithm:** trigonometric functions through `fid-num`; explicit 3x3 products in fixed order; no FMA.
- **Errors:** `invalid_parameter`.
- **Determinism:** D0.
- **Performance budget:** ≤ 200 ns per observation [unverified until measured].
- **Crate:** `fid-kunc` (module `inst`).

## Test fixtures

`fixtures/unc/inst/port/sph_cases.json` with 50-digit references from an mpmath script (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc` (kernel; KNP-1: no FMA, transcendental functions only through `fid-num`, fixed-order sums, NaN is an error; no panic or `unwrap`), add `inst::sph_cov(params: SphParams, r, az, el) -> Result<Cov3, InstError>`, algorithm ID `unc.inst.sph_cov@1.0.0`. Convention x = r cos el cos az, y = r cos el sin az, z = r sin el; σ_r = sigma_r0 + sigma_r1·r; C from σ_r, sigma_az, sigma_el and optional correlations; return J·C·Jᵀ (m²). `invalid_parameter` for negative or non-finite σ or |ρ| > 1. Tests AC-1..AC-4 (`fixtures/unc/inst/port/sph_cases.json`, 1e-14 relative Frobenius; axis-aligned exact case; bit-identity on wasm32, x86-64, ARM64).

## Open questions

- Tracker angle conventions differ by maker (zenith versus elevation) [unverified]; drivers must convert to this convention before calling.
- **Build notes (fid-kunc `inst::sph`, algorithm `unc.inst.sph_cov@1.0.0`, card `crates/fid-kunc/cards/unc.inst.sph_cov@1.0.0.toml`).** Choices made where the Design is silent, for review:
  - `sph_cov` returns `SphCov`, which wraps the `Cov3` with the `sigma_r` used, the outcome of the joint correlation test and validated-domain notes; `Cov3` (shared with ARM and OPT later) is exactly symmetric (upper triangle computed and mirrored), finite, and writes `-0` as `+0`.
  - Pairwise-valid correlations that cannot hold at once (0.9, 0.9, -0.9) are refused as `invalid_parameter{correlation, not_positive_semidefinite}` by the sign of the determinant of the correlation matrix, decided exactly (fixed-width integer arithmetic on the significands and exponents, no tolerance, exact for subnormals too); an exactly zero determinant is accepted and reported as `singular`. Dekker split products were not used because they stop being error-free when a product underflows (a = 1, b = 1e-300, c = 2e-300 has the exact determinant -(b - c)^2 < 0 while every split product of b and c underflows). The final eigenvalue check of the covariance stays with `UNC.INST.PORT.EVAL` (REQ-UNC.INST.PORT.EVAL-004).
  - Observation values have their own code `invalid_observation{name}` (NaN or infinite `r`, `az`, `el`; `r < 0`); an overflow is `numeric`. Neither is in the Design's error list.
  - REQ-002 states the accuracy for 0.1 m <= r <= 100 m and |el| <= 1.5 rad; the result carries notes when the observation is outside that domain, when |az| > 2 pi (the fixtures do not test beyond), when every entry is zero, or when the largest entry is below 2^-962 (underflow could matter). The covariance is still returned, labelled.
  - The 50-digit reference of AC-1 is computed with the Python standard library `decimal` module (the 90-digit trig oracle of `fixtures/num/make_fn_fixtures.py`) rather than mpmath, so the oracle has no third-party dependency. AC-4 compares the output bits with a Python model of the documented evaluation order on every CI target.
  - The performance budget (<= 200 ns per observation) is not measured here and stays [unverified].

## Children

_Leaf: none._
