---
id: UNC.PROP.MCM
title: Adaptive Monte Carlo propagation through the kernel pipeline
level: 3
parent: UNC.PROP
kind: module
status: split
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
updated: 2026-11-17
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

## Research hypotheses

Measurable hypotheses against public reference data (D-036 rule 4). Thresholds marked [unverified] are targets, not results. The children named in each item must state it as `AC-H` acceptance criteria.

- **H-1** End-to-end Monte Carlo reproduces the published results of the JCGM 101:2008 worked examples to the published digits (`UNC.PROP.MCM.CMD`).
- **H-2** The adaptive stop rule achieves its stated numerical tolerance against analytic or long-run references in at least 95 % [unverified] of seeds (`UNC.PROP.MCM.ADAPT`).
- **H-3** Counter-based block sampling and interval extraction reproduce known output distributions within statistical tolerance (`UNC.PROP.MCM.BLOCK`, `UNC.PROP.MCM.INTVL`).
- **H-4** Monte Carlo intervals cover at the nominal rate on ORS-UNC `nonlin-rect`, and retained sample sets are complete enough for downstream conformance probability (`UNC.PROP.MCM.CMD`, `UNC.PROP.MCM.SAMP`).

## Interfaces

- **Offers:** `propagate_mcm(model, inputs, seed, profile) -> McmResult { trials, interval, u, mc_mean, mean_minus_fx, samples_ref, flags }`; command `unc.propagate_mcm@1`; sample sets by digest.
- **Consumes:** validated models (UNC.MODEL.GRAPH); samplers (UNC.MODEL.DIST); kernel call interface and RNG (KRN).

## Decomposition hints

BLOCK (trial blocks and parallel evaluation with fixed-order reduction), ADAPT (stopping rule from the published procedure), INTERVAL (shortest and symmetric intervals from sorted samples), SAMPLES (retained sample sets), CMD.

## Open questions

- **Applied (adaptive rule):** the stopping rule is specified in `UNC.PROP.MCM.ADAPT` as `mcm-adapt@1`, paraphrased from the adaptive procedure of JCGM 101:2008 [unverified: clause 7.9 numbering and block-size rule]. A spec card in our own words is part of that leaf, and an uncertainty expert should review it.
- **Applied (memory):** retained sample sets are capped (`UNC.PROP.MCM.SAMP`, default 64 MiB [unverified]). Over the cap, no samples are retained and the result carries `samples_not_retained`; DEC and XFM consumers then fall back to interval-only use.
- **Layering:** engine functions (BLOCK, ADAPT, INTVL, SAMP) are L0 in `fid-kunc`; thread scheduling, job progress and blob storage are in the L2 command (CMD, `fid-unc`), so the kernel stays single-threaded and I/O-free.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UNC.PROP.MCM.BLOCK | Trial-block sampling and per-trial kernel evaluation (leaf) | 001 004 005 | none |
| UNC.PROP.MCM.ADAPT | Adaptive stopping rule and trial cap (leaf) | 002 003 | none |
| UNC.PROP.MCM.INTVL | Coverage intervals and estimate outputs from samples (leaf) | 006 | none |
| UNC.PROP.MCM.SAMP | Retained sample sets and their digest (leaf) | 004 | none |
| UNC.PROP.MCM.CMD | The unc.propagate_mcm@1 command and block scheduling (leaf) | 001 004 | none |
