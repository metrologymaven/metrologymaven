---
id: UNC.INST.PORT.OPT
title: Scanner point covariance from range and incidence
level: 4
parent: UNC.INST.PORT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.PORT-001, REQ-UNC.INST.PORT-003]
covers: [SA.UNC.01]
depends_on: [KRN.NUM, UNC.MODEL.DIST]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.INST.PORT.OPT: Scanner point covariance from range and incidence

## Purpose

Kernel function for optical scanner points: an along-beam standard uncertainty that grows with range as a declared polynomial and with incidence angle as a declared power of 1/cos(incidence), and a lateral term, assembled into a 3x3 covariance oriented along the beam. Points beyond the model's incidence limit are flagged, never extrapolated.

## Scope

**In:** parameters `beam_c0` (m), `beam_c1` (m/m), `beam_c2` (m/m²), `inc_power` (0 to 2), `inc_max` (rad, < π/2), `lat_c0` (m), `lat_c1` (m/m); range r = ‖p − sensor origin‖, beam direction b, surface normal n, incidence θ = angle between −b and n; σ_b = (c0 + c1·r + c2·r²)/cos(θ)^inc_power; σ_l = lat_c0 + lat_c1·r; Σ = σ_l²·I + (σ_b² − σ_l²)·b·bᵀ.
**Out:** range axis check and PSD check (owned by `UNC.INST.PORT.EVAL`); normal estimation (caller supplies n).

## Requirements

- **REQ-UNC.INST.PORT.OPT-001** The function shall return the covariance in m² defined in Scope for point p, sensor origin, beam direction b and normal n.
- **REQ-UNC.INST.PORT.OPT-002** If θ exceeds `inc_max`, then the function shall return `out_of_model_range` with axis `incidence` and bound `inc_max`, and no covariance.
- **REQ-UNC.INST.PORT.OPT-003** If `inc_power` lies outside [0, 2], `inc_max` ≥ π/2, or any coefficient is non-finite or makes σ_b or σ_l negative in the model's range, then the function shall fail with `invalid_parameter`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/port/opt_cases.json` (150 seeded cases, seed `0x4F5054`), then each matches the mpmath reference within 1e-14 relative.
- **AC-2** Given θ = 0, then σ_b = c0 + c1·r + c2·r² within 1 ulp.
- **AC-3** (range) Given inc_max = 1.2 rad and θ = 1.25 rad, then `out_of_model_range{incidence, 1.2}`.
- **AC-4** (error path) Given inc_power = 3, then `invalid_parameter`.

## Design

- **Registry command(s):** none; kernel algorithm `unc.inst.opt_cov@1.0.0`.
- **Algorithm:** θ via `fid-num` acos of the clamped dot product; power via `fid-num` pow; fixed-order outer product.
- **Errors:** `invalid_parameter`, `invalid_direction`, `out_of_model_range`.
- **Determinism:** D0.
- **Performance budget:** ≤ 200 ns per point [unverified until measured].
- **Crate:** `fid-kunc` (module `inst`).

## Test fixtures

`fixtures/unc/inst/port/opt_cases.json` with mpmath references (CC-BY-4.0); invented parameters.

## Build prompt

> In crate `fid-kunc` (kernel; KNP-1; no panic or `unwrap`), add `inst::opt_cov(params: OptParams, p, origin, b, n) -> Result<OptCov, InstError>`, algorithm ID `unc.inst.opt_cov@1.0.0`: r = ‖p − origin‖, θ = angle(−b, n), σ_b = (c0 + c1 r + c2 r²)/cos(θ)^inc_power, σ_l = lat_c0 + lat_c1 r, Σ = σ_l² I + (σ_b² − σ_l²) b bᵀ. Return `OutOfModelRange{incidence, inc_max}` when θ > inc_max; `invalid_parameter` for inc_power ∉ [0, 2], inc_max ≥ π/2 or non-finite coefficients; `invalid_direction` for zero b or n. Tests AC-1..AC-4 with `fixtures/unc/inst/port/opt_cases.json`.

## Open questions

- The polynomial-and-cosine form is a project parameterisation, not taken from a standard; whether it can represent published scanner noise data needs checking against public sources [unverified].

## Children

_Leaf: none._
