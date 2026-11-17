---
id: UNC.INST.PORT.EVAL
title: Observation covariance entry point, dispatch, checks and batch
level: 4
parent: UNC.INST.PORT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.PORT-001, REQ-UNC.INST.PORT-004, REQ-UNC.INST.PORT-005]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, UNC.INST.PORT.SPH, UNC.INST.PORT.ARM, UNC.INST.PORT.OPT, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.INST.PORT.EVAL: Observation covariance entry point, dispatch, checks and batch

## Purpose

`observation_covariance(model, observation)`: the one entry point that networks, transforms and Monte Carlo fields call. It reads the bound `instrument-model/v1` of kind `observation`, extracts the typed parameters for its sub-kind (`spherical | arm | optical`), runs the range check, calls the kernel function, verifies that the result is symmetric positive semi-definite, and attaches range flags. A batch form serves 10^6 observations.

## Scope

**In:** parameter extraction per sub-kind with typed errors; `check_range` call before evaluation; PSD verification of every result; batch form in input order; result type `ObsCov { cov: Cov3 (m²), frame: instrument, flags }`.
**Out:** the covariance formulas (owned by `UNC.INST.PORT.SPH`, `.ARM`, `.OPT`); weights (owned by `UNC.INST.WEIGHT`); the range check itself (owned by `UNC.INST.SCHEMA`).

## Requirements

- **REQ-UNC.INST.PORT.EVAL-001** When an observation is evaluated with a bound model of kind `observation`, the function shall return a 3x3 covariance in m² in the instrument frame from the sub-kind's kernel function.
- **REQ-UNC.INST.PORT.EVAL-002** If the model lacks a parameter its sub-kind requires, then the function shall fail with `param_missing` naming it.
- **REQ-UNC.INST.PORT.EVAL-003** If `check_range` returns `out_of_model_range`, then the function shall not evaluate the model and shall return the flag with axis and bound and no covariance.
- **REQ-UNC.INST.PORT.EVAL-004** If a computed covariance is not exactly symmetric or has an eigenvalue below −1e-12·‖Σ‖, then the function shall fail with `invalid_covariance`.
- **REQ-UNC.INST.PORT.EVAL-005** The batch form shall return results in input order, bit-identical to single calls, and evaluate 10^6 spherical observations in under 1 s single-threaded on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/port/tracker_model.json` and observation (r 5 m, az 0.3 rad, el 0.1 rad), when evaluated, then the covariance equals the `UNC.INST.PORT.SPH` reference in `fixtures/unc/inst/port/sph_ref.json` bit for bit.
- **AC-2** (range) Given an observation at r 52 m against a 40 m range axis, then the result is `out_of_model_range{range, 40 m}` with no covariance.
- **AC-3** (error paths) Given a model without `sigma_az`, then `param_missing{sigma_az}`; given a declared parameter correlation of 1.5 producing an indefinite matrix, then `invalid_covariance`.
- **AC-4** (batch) Given 10^6 seeded observations (seed `0x504F5254`), then batch results equal single calls bit for bit and the timing is logged against the budget.

## Design

- **Registry command(s):** none; library function `unc.inst.observation_covariance@1.0.0` in the algorithm registry.
- **Data structures:** `Observation { Spherical{r, az, el} | ArmPoint{p, probe_dir} | OpticalPoint{p, beam_dir, normal} }`; `ObsCov`; `ObsFlags`.
- **Algorithm:** extract → range check → kernel → symmetric eigen check through `fid-num` (fixed order).
- **Errors:** `param_missing`, `kind_mismatch`, `invalid_covariance`.
- **Determinism:** D0.
- **Performance budget:** see REQ-005.
- **Crate:** `fid-unc-inst` (module `observation`).

## Test fixtures

`fixtures/unc/inst/port/`: invented tracker, arm and scanner models; reference covariances from the kernel children's fixtures (CC-BY-4.0).

## Build prompt

> In crate `fid-unc-inst`, module `observation`, implement `observation_covariance(model, obs) -> Result<ObsCov, ObsError>` and a batch form. Extract typed parameters for sub-kind `spherical | arm | optical` from the bound `instrument-model/v1` (`param_missing{name}`, `kind_mismatch`), call `check_range` first (return `out_of_model_range{axis, bound}` without evaluating), call the `fid-kunc` kernel functions `inst::sph_cov`, `inst::arm_cov`, `inst::opt_cov`, then verify exact symmetry and eigenvalues ≥ −1e-12·‖Σ‖ via `fid-num` (`invalid_covariance`). Batch: input order, bit-identical to singles. Tests AC-1..AC-4 with `fixtures/unc/inst/port/`. No panic or `unwrap`; KNP-1 inside the kernel calls.

## Open questions

- None.

## Children

_Leaf: none._
