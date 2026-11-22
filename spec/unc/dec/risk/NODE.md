---
id: UNC.DEC.RISK
title: Specific and global conformity risk
level: 3
parent: UNC.DEC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.DEC-006]
covers: []
depends_on: [UNC.DEC.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-kdec]
updated: 2026-11-22
---

# UNC.DEC.RISK: Specific and global conformity risk

## Purpose

Computes the risk figures that JCGM 106:2012 describes for a conformity decision: the specific consumer risk of an accepted item and the specific producer risk of a rejected item, from the same output distribution the evaluation used; and, when a process prior is declared, the global consumer and producer risks of the acceptance rule. Results are attached to the decision record when the rule's `risk_reporting` asks for them.

## Scope

**In:**
- Specific risk: for CONFORMS, 1 − p; for NONCONFORMS, p; for INDETERMINATE, both
- Global risk under a declared normal process prior N(μ₀, σ₀) and normal measurement model with standard uncertainty u, for the acceptance zone the rule defines
- Fixed-node numerical integration with stated error bound

**Out:**
- The decision itself and p (owned by `UNC.DEC.EVAL`)
- Producing the prior from SPC data (owned by `RPT`); the prior is a declared input here

## Requirements

- **REQ-UNC.DEC.RISK-001** When a rule requests specific risk, the function shall return the specific consumer risk 1 − p for CONFORMS outcomes and the specific producer risk p for NONCONFORMS outcomes, from the p computed by the evaluation.
- **REQ-UNC.DEC.RISK-002** When a rule requests global risk and a normal process prior with mean and standard deviation is declared with a source, the function shall return global consumer and producer risk for the rule's acceptance zone.
- **REQ-UNC.DEC.RISK-003** If global risk is requested without a declared prior, then the function shall return no global figures and flag `prior_missing`, never assuming a prior.
- **REQ-UNC.DEC.RISK-004** Global risk integrals shall be computed with a fixed-node quadrature whose absolute error is below 1e-9 on the fixture cases.
- **REQ-UNC.DEC.RISK-005** The declared prior's source shall be recorded with the risk figures.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/dec/risk/specific.csv` (outcomes and p), when computed, then the specific risks equal 1 − p or p exactly as f64 subtraction.
- **AC-2** Given `fixtures/unc/dec/risk/global.csv` (tolerance, guard band, μ₀, σ₀, u; references computed by 50-digit adaptive quadrature in mpmath), when computed, then global consumer and producer risks are within 1e-9 absolute of the references.
- **AC-3** (error path) Given global risk requested with no prior, then the record has no global figures and the flag `prior_missing`.
- **AC-4** (sanity) Given u → small (u = 1e-6 · tolerance) on a centred process, then both global risks are below 1e-9.
- **AC-5** (portability) Outputs bit-identical on wasm32, x86-64 and ARM64.

## Design

- **Registry command(s):** none; kernel algorithm `dec.risk.compute@1.0.0`, called by `dec.rule.evaluate@1.0.0` when `risk_reporting` asks.
- **Data structures:** `Prior { mu, sigma, source: UntrustedString }`; `RiskFigures { specific_consumer: Option<f64>, specific_producer: Option<f64>, global_consumer: Option<f64>, global_producer: Option<f64>, prior_source, flags }`.
- **Algorithm:** inner integral over measured values in closed form with the normal CDF from `UNC.DEC.EVAL`; outer integral over true values by composite Gauss–Legendre with fixed panels over μ₀ ± 10σ₀ split at the tolerance limits; fixed-order sums.
- **Errors:** `invalid_prior` (σ₀ ≤ 0 or non-finite), numeric errors.
- **Determinism:** D0.
- **Performance budget:** one global-risk evaluation in under 1 ms [unverified until measured].
- **Crate:** `fid-kdec`.

## Test fixtures

`fixtures/unc/dec/risk/`: CSV cases with 50-digit references from an mpmath script (project-authored, CC-BY-4.0). No values are copied from JCGM 106 examples.

## Build prompt

> In crate `fid-kdec` (kernel; KNP-1; no panics, no `unwrap`), add `risk::compute(decision, rule, u, prior: Option<Prior>) -> Result<RiskFigures, RiskError>`, algorithm ID `dec.risk.compute@1.0.0`.
> Specific risk: CONFORMS -> consumer risk 1 − p; NONCONFORMS -> producer risk p; INDETERMINATE -> both. Global risk (only with a declared normal prior N(μ₀, σ₀) and its source): consumer risk = probability that a true value lies outside the tolerance and its measured value inside the rule's acceptance zone; producer risk = true value inside, measured value outside; inner integral via the normal CDF from module `evaluate`, outer integral by fixed composite Gauss–Legendre panels over μ₀ ± 10σ₀ split at the limits; absolute error < 1e-9. No prior -> no global figures, flag `prior_missing`. Record the prior source (untrusted string).
> Tests: `fixtures/unc/dec/risk/specific.csv`; `global.csv` within 1e-9 of 50-digit references; missing prior; tiny-u sanity; bit-identical on wasm32, x86-64, ARM64.

## Open questions

- Non-normal priors (for example from a fitted process distribution) are deferred.

## Children

_Leaf: none._
