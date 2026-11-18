---
id: UNC.PROP.VALID
title: Validation of LPU against Monte Carlo
level: 3
parent: UNC.PROP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-005]
covers: []
depends_on: [UNC.UEC.STMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.PROP.VALID: Validation of LPU against Monte Carlo

## Purpose

The kernel function that compares an LPU coverage interval with a Monte Carlo coverage interval for the same model and decides whether the LPU result is validated, in the manner described by JCGM 101:2008 (its validation clause): both interval endpoints must agree within a numerical tolerance derived from the number of significant digits of the standard uncertainty. The outcome sets `lpu_validated_by_mc`, which the decision engine needs before it may use a normal approximation.

## Scope

**In:**
- Numerical tolerance from a stated number of significant decimal digits of u(y) (default 2)
- Endpoint comparison and the boolean outcome with both endpoint differences
- Algorithm ID `unc.prop.validate_lpu@1.0.0`

**Out:**
- Running LPU and MCM (owned by `UNC.PROP.LPU` and `UNC.PROP.MCM`)
- The `unc.compare_methods@1` command (owned by `UNC.PROP.SELECT`)

## Requirements

- **REQ-UNC.PROP.VALID-001** Given an LPU estimate y with expanded uncertainty U_p at coverage probability p, an MCM interval [y_low, y_high] at the same p, and the MCM standard uncertainty u(y), the function shall compute δ as half a unit in the last of `n_dig` significant decimal digits of u(y) (default `n_dig` = 2).
- **REQ-UNC.PROP.VALID-002** The function shall return `validated = true` only when |y − U_p − y_low| ≤ δ and |y + U_p − y_high| ≤ δ, and shall return both differences and δ.
- **REQ-UNC.PROP.VALID-003** If the coverage probabilities differ, either interval is empty or reversed, or any input is non-finite, then the function shall fail with `incomparable` and return no outcome.
- **REQ-UNC.PROP.VALID-004** The computation of δ shall use exact decimal digit extraction through the `fid-num` decimal facilities, so that the outcome is bit-identical on every target.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/valid/linear_sum.json` (Y = X1 + X2, both normal; LPU and MCM results precomputed with seed `0xA11CE`), when validated, then `validated = true` and both differences are below δ.
- **AC-2** Given `fixtures/unc/valid/square_near_zero.json` (Y = X², X ~ N(0, 1)), when validated, then `validated = false`.
- **AC-3** Given `fixtures/unc/valid/delta_cases.csv` (u(y) values such as 0.0123, 1.0e-6, 9.96e-7 with `n_dig` 1, 2, 3 and expected δ), when δ is computed, then it equals the expected value exactly.
- **AC-4** (error path) Given p = 0.95 for LPU and p = 0.99 for MCM, then the call fails with `incomparable`.
- **AC-5** (portability) Given AC-1..3, when run on wasm32, x86-64 and ARM64, then outputs are bit-identical.
- **AC-H1** Hypothesis: the validation outcome reproduces the validation conclusions published for the JCGM 101:2008 worked examples (additive model, mass calibration, comparison loss, gauge-block calibration [unverified: clause 9 numbering]), encoded from their published input definitions with the published results as reference values (validated or not at the stated number of significant digits), measured by running this function on the published LPU and MCM results; falsified if any conclusion differs.
- **AC-H2** Hypothesis: `validated = true` implies an honest LPU interval: for every case of the ORS-UNC known-truth cases `lin-gauss`, `nonlin-rect`, `mvar-datum` of `UNC.ORS.UNC.BASE` (the `fid-ors` release named in the run record) where the function returns `validated = true`, the LPU interval's coverage lies within the exact two-sided 99 % binomial band around p = 0.95 computed by `UNC.ORS.UNC.HARNESS` with N ≥ 1000 seeded repetitions, measured by running the harness with LPU forced; falsified by any validated case outside its band.

## Design

- **Registry command(s):** none (kernel algorithm `unc.prop.validate_lpu@1.0.0`, called by `UNC.PROP.SELECT`).
- **Data structures:** `ValidateInput { y, U_p, p_lpu, y_low, y_high, p_mcm, u_mcm, n_dig: u8 }`; `ValidateOutput { validated: bool, d_low, d_high, delta }`.
- **Algorithm:** write u(y) as c × 10^l with c an integer of `n_dig` digits (using exact decimal extraction, not `log10`); δ = ½ × 10^l; compare absolute endpoint differences (computed with plain f64 subtraction, no FMA) against δ.
- **Errors:** `incomparable{reason}`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-kunc`.

## Test fixtures

`fixtures/unc/valid/`: two model result files produced once by a reference script (synthetic models, CC-BY-4.0) and `delta_cases.csv` with hand-computed δ values.

## Build prompt

> In crate `fid-kunc` (kernel; KNP-1; no panics or `unwrap`), add `prop::validate_lpu(input) -> Result<ValidateOutput, ValidateError>`, algorithm ID `unc.prop.validate_lpu@1.0.0`. Inputs: LPU estimate y, expanded U_p, coverage probability p_lpu; MCM interval [y_low, y_high], p_mcm and standard uncertainty u_mcm; `n_dig` (default 2).
> Compute δ: express u_mcm as c × 10^l with c an integer of exactly `n_dig` significant digits using the `fid-num` exact decimal facilities (no `log10`, no f64 power loops that could round differently by target); δ = 0.5 × 10^l. Return `validated = |y − U_p − y_low| ≤ δ && |y + U_p − y_high| ≤ δ` plus d_low, d_high and δ. Fail with `incomparable` if p_lpu ≠ p_mcm, an interval is empty or reversed, or any input is non-finite.
> Tests: `fixtures/unc/valid/linear_sum.json` validates; `square_near_zero.json` does not; every row of `delta_cases.csv` gives the exact δ; mismatched p fails; all outputs bit-identical on wasm32, x86-64 and ARM64.

## Open questions

- The default `n_dig` = 2 follows common practice for reporting u [unverified against JCGM 101 guidance].

## Children

_Leaf: none._
