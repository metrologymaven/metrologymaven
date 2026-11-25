---
id: RPT.STATS.CAP.INDEX
title: Capability index kernel
level: 4
parent: RPT.STATS.CAP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-stats]
built_pr: 598
satisfies: [REQ-RPT.STATS.CAP-001, REQ-RPT.STATS.CAP-002, REQ-RPT.STATS.CAP-003, REQ-RPT.STATS.CAP-004, REQ-RPT.STATS.CAP-005, REQ-RPT.STATS.CAP-006]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.STATS.CAP.INDEX: Capability index kernel

## Purpose

The pure function that turns a mean, a spread estimate, the specification limits and the sample size into a potential index (Cp or Pp) and a centred index (Cpk or Ppk, or the one-sided Cpu/Cpl, Ppu/Ppl). The same formulas serve both families; only the spread differs (within-subgroup for C indices, overall for P indices), and the caller names it. Every typed non-result (`NOT_APPLICABLE`, `INSUFFICIENT_DATA`, `UNDEFINED_ZERO_SPREAD`) is decided here, so no caller prints a number it should not.

## Scope

**In:**
- Normal-theory formulas for two-sided and one-sided tolerances
- Typed statuses and their precedence
- Estimator ID passthrough and the method citation

**Out:**
- Within-subgroup spread estimators (`RPT.STATS.CAP.SIGMA`)
- Overall standard deviation (`RPT.STATS.DESC`)
- The command and tolerance lookup (`RPT.STATS.CAP.CMD`)
- Non-normal methods (not required yet)

## Requirements

- **REQ-RPT.STATS.CAP.INDEX-001** With both limits, the function shall return the potential index (USL − LSL)/(6σ) and the centred index min(USL − μ, μ − LSL)/(3σ).
- **REQ-RPT.STATS.CAP.INDEX-002** With only an upper limit, it shall return the upper index (USL − μ)/(3σ) as the centred index and `NOT_APPLICABLE` for the potential index; with only a lower limit, it shall return (μ − LSL)/(3σ) and `NOT_APPLICABLE`.
- **REQ-RPT.STATS.CAP.INDEX-003** If n is below the configured minimum (default 25 [unverified]), then every index shall be `INSUFFICIENT_DATA` and no number shall be produced.
- **REQ-RPT.STATS.CAP.INDEX-004** If σ is zero, then every applicable index shall be `UNDEFINED_ZERO_SPREAD` and no number shall be produced; `INSUFFICIENT_DATA` shall take precedence over `UNDEFINED_ZERO_SPREAD`, which shall take precedence over a number.
- **REQ-RPT.STATS.CAP.INDEX-005** If USL ≤ LSL, or any input is non-finite, or σ is negative, then the function shall fail with `stats.bad_capability_input`.
- **REQ-RPT.STATS.CAP.INDEX-006** The result shall carry the estimator ID supplied by the caller, the index family (`C` or `P`) and the algorithm ID `stats.cap.index@1.0.0`.
- **REQ-RPT.STATS.CAP.INDEX-007** The function shall use only IEEE basic operations without FMA and total-order min, and shall be bit-identical on every supported target (D0).
- **REQ-RPT.STATS.CAP.INDEX-008** The method documentation shall cite the index definitions by designator and clause only (for example the ISO 22514 series [unverified part, edition and clause]) and embed no standards text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/cap_cases.json` (hand-computed cases: centred, off-centre, mean outside the limits giving a negative centred index, upper-only, lower-only), when the function runs, then each result equals the expected value exactly or within 1 ulp as stated per case.
- **AC-2** (one-sided) Given USL = 10, no LSL, μ = 7, σ = 1, n = 30, when the function runs, then the centred index is 1.0 and the potential index is `NOT_APPLICABLE`.
- **AC-3** (statuses) Given n = 24 with σ = 0, when the function runs, then every index is `INSUFFICIENT_DATA`; given n = 30 with σ = 0, every applicable index is `UNDEFINED_ZERO_SPREAD`.
- **AC-4** (error path) Given USL = 1 and LSL = 2, when the function runs, then it fails with `stats.bad_capability_input`.

## Design

- **Registry commands:** none (kernel function). `side_effect: R1` is that of `stats.compute_capability@1`.
- **Kernel function:** `fn capability(mean: f64, sigma: f64, n: u64, lsl: Option<f64>, usl: Option<f64>, min_n: u64, family: Family, estimator: EstimatorId) -> Result<IndexPair, StatsError>`.
- **Data structures:** `IndexPair { potential: IndexValue, centred: IndexValue, side: Side::{Both, UpperOnly, LowerOnly}, family, estimator, algorithm }`; `enum IndexValue { Value(f64), NotApplicable, InsufficientData, UndefinedZeroSpread }`.
- **Algorithm:** REQ-001 to REQ-004; compute 3σ and 6σ as `3.0 * sigma` and `6.0 * sigma`, then divide; min by `fid-num` total order.
- **Errors:** `stats.bad_capability_input{reason}`.
- **Determinism:** D0.
- **Crate:** `fid-stats` (kernel graph; KNP-1).

## Test fixtures

- `fixtures/rpt/stats/cap_cases.json`: hand-computed cases, CC-BY-4.0.

## Build prompt

> In crate `fid-stats` (kernel graph, KNP-1: no FMA, total-order min via `fid-num`, no I/O, NaN is an error), implement `capability(mean, sigma, n, lsl, usl, min_n, family, estimator) -> Result<IndexPair, StatsError>`.
>
> Two-sided: potential = (USL − LSL)/(6σ), centred = min(USL − μ, μ − LSL)/(3σ). Upper only: centred = (USL − μ)/(3σ), potential `NOT_APPLICABLE`; lower only: centred = (μ − LSL)/(3σ), potential `NOT_APPLICABLE`. n < min_n -> all `INSUFFICIENT_DATA`; else σ = 0 -> applicable indices `UNDEFINED_ZERO_SPREAD`. USL ≤ LSL, negative σ or non-finite input -> `stats.bad_capability_input`. Record family (C or P), estimator ID and algorithm ID `stats.cap.index@1.0.0`. Cite definitions by designator only in rustdoc.
>
> **Tests:** AC-1 to AC-4 above with `fixtures/rpt/stats/cap_cases.json`, plus a cross-target digest test.
>
> **Constraints:** CONSTITUTION §8; C1. Spec: `spec/rpt/stats/cap/index/NODE.md`.

## Open questions

- **Expert choice (flagged):** the default minimum sample size (25 [unverified]); and how to treat a one-sided tolerance with a natural boundary (for example a form tolerance bounded below by zero), which some references handle differently [unverified].

## Children

_Leaf: none._
