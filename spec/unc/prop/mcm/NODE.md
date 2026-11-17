---
id: UNC.PROP.MCM
title: Adaptive Monte Carlo propagation through the kernel pipeline
level: 3
parent: UNC.PROP
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-003, REQ-UNC.PROP-004]
covers: []
depends_on: [UNC.MODEL.GRAPH, UNC.MODEL.DIST, KRN]
cli: [fid unc propagate-mcm]
mcp: [unc_propagate_mcm]
journal: [unc.propagate_mcm@1]
updated: 2026-11-16
---

# UNC.PROP.MCM: Adaptive Monte Carlo propagation through the kernel pipeline

## Purpose

Propagation of distributions by Monte Carlo (JCGM 101:2008): each trial draws every input from its distribution and re-runs the full kernel pipeline of the model, including fits, so non-smooth and bounded quantities are handled correctly. The adaptive procedure stops when the coverage interval endpoints are numerically stable. Trials are organised in fixed blocks addressed by the counter-based RNG, so the trial count and interval are bit-identical for the same model, inputs and seed.

## Scope

**In:**
- Trial blocks of fixed size, sampling by (seed, stream = input, index = trial) from `UNC.MODEL.DIST`
- Re-evaluation of the model graph per trial through the kernel call interface
- Adaptive stopping on the numerical tolerance of the interval endpoints and standard uncertainty, with a trial cap (default 10^6 [unverified])
- Coverage interval (shortest and probabilistically symmetric), estimate convention outputs (MC mean, mean minus f(x) for bias disclosure)
- Retained sample sets for `UNC.DEC` and `UNC.XFM` (bounded size, digest-addressed)
- A `gum_suite_profile` field so a revised Supplement 1 (GUM-7) can be adopted as a profile change [unverified]
- `unc.propagate_mcm@1` (R1)

**Out:**
- Selection of MCM versus LPU and the reported value policy (owned by `UNC.PROP.SELECT`)
- Spatial fields (owned by `UNC.XFM.FIELD`)

## Requirements

- **REQ-UNC.PROP.MCM-001** For each trial, the engine shall evaluate every operator of the model through the kernel call interface on that trial's sampled inputs.
- **REQ-UNC.PROP.MCM-002** The engine shall run trials in fixed blocks and stop after the first block at which the numerical tolerance condition on both interval endpoints and the standard uncertainty is met, or at the trial cap.
- **REQ-UNC.PROP.MCM-003** If the trial cap is reached before the tolerance is met, then the engine shall return the result with flag `not_converged` and the achieved tolerance.
- **REQ-UNC.PROP.MCM-004** Given the same model digest, inputs, seed and profile, the engine shall return a bit-identical trial count, interval and sample-set digest on the same platform class, regardless of thread count (D2).
- **REQ-UNC.PROP.MCM-005** If a trial's kernel evaluation fails (for example a fit does not converge), then the engine shall count it, exclude it, and fail with `trial_failures` if failures exceed 0.1 % of trials [unverified], naming the first failing trial index.
- **REQ-UNC.PROP.MCM-006** The output shall include the MC mean and the difference between the MC mean and the kernel value at the measured inputs, for use by the estimator policy.

## Interfaces

- **Offers:** `propagate_mcm(model, inputs, seed, profile) -> McmResult { trials, interval, u, mc_mean, mean_minus_fx, samples_ref, flags }`; command `unc.propagate_mcm@1`; sample sets by digest.
- **Consumes:** validated models (UNC.MODEL.GRAPH); samplers (UNC.MODEL.DIST); kernel call interface and RNG (KRN).

## Decomposition hints

BLOCK (trial blocks and parallel evaluation with fixed-order reduction), ADAPT (stopping rule from the published procedure), INTERVAL (shortest and symmetric intervals from sorted samples), SAMPLES (retained sample sets), CMD.

## Open questions

- The adaptive stopping rule must be written from the published JCGM 101 text into a spec card before leaves are written [unverified: details of the procedure].
- Memory budget for retained sample sets on large models is unmeasured.

## Children

_None yet: status `open`._
