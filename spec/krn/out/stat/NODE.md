---
id: KRN.OUT.STAT
title: Residual-statistics policies (n-sigma and robust scale)
level: 3
parent: KRN.OUT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.OUT-001]
covers: [GOM.CON.21, PCD.CON.02, SA.REL.04]
depends_on: [KRN.OUT.POL, KRN.NUM.RED]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.OUT.STAT: Residual-statistics policies (n-sigma and robust scale)

## Purpose

The two most common fit filters in incumbents: exclude points whose residual exceeds k standard deviations, and the robust variant that uses a robust scale estimate (median absolute deviation or Qn) so a few gross outliers cannot inflate the threshold. Both are deterministic functions of the residuals.

## Scope

**In:** `filter.outlier.nsigma@1.0.0` and `filter.outlier.robust_scale@1.0.0` implementing `PolicyMethod`; exact (total-order) median selection; one-sided and two-sided variants.
**Out:** iteration of reject-and-refit (`KRN.OUT.RUN`); weighted (IRLS) M-estimation, which needs weighted fits (open question).

## Requirements

- **REQ-KRN.OUT.STAT-001** The n-sigma policy shall exclude each point whose absolute signed residual exceeds k times the residual standard deviation computed with `KRN.NUM.RED`, where k is a required parameter.
- **REQ-KRN.OUT.STAT-002** The robust-scale policy shall compute the median of residuals and a robust scale (1.4826 times the median absolute deviation, or the Qn estimator) by total-order selection, and exclude each point beyond k times that scale from the median.
- **REQ-KRN.OUT.STAT-003** Each policy shall support a side parameter (both, positive only, negative only), with no default.
- **REQ-KRN.OUT.STAT-004** If the scale is zero (all residuals equal), then the policy shall exclude no points and report `zero_scale`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/out/stat-contam-v1` (seeded planes and circles with 2 % gross outliers at 50 sigma), when the robust-scale policy with k = 3 runs on the least-squares residuals, then every planted outlier is excluded and at most 1 % [unverified] of clean points are excluded.
- **AC-2** Given the same fixture, when the n-sigma policy with k = 3 runs, then the mask equals an independent test-side computation index for index.
- **AC-3** Given AC-1 inputs on x86-64, ARM64 and wasm32 under wasmtime, then masks are identical.
- **AC-4** Given 50 identical residuals, then no point is excluded and `zero_scale` is reported.

## Design

- **Registry command(s):** none; methods selected through `fit.reject_outliers@1` (`KRN.OUT.RUN`).
- **Data structures:** implementations of `PolicyMethod` for `NSigma` and `RobustScale`.
- **Algorithm:** standard deviation via `fid_num::red`; median by sorting a copy with `fid_num::f::total_cmp` (stable sort); MAD constant 1.4826 [consistency constant for the normal distribution]; Qn per Rousseeuw and Croux 1993, "Alternatives to the median absolute deviation".
- **Errors:** `PolicyError::InvalidParameter`; guard errors from `KRN.OUT.POL`.
- **Determinism:** D0.
- **Performance budget:** 1 M residuals < 120 ms including the sort [unverified until measured].
- **Crate:** `fid-filter` (module `stat`).

## Test fixtures

`fixtures/out/stat-contam-v1`: seeded synthetic generator, CC-BY-4.0.

## Build prompt

> In crate `fid-filter`, create module `stat` implementing `PolicyMethod` (module `policy`) for `NSigma{k}` (`filter.outlier.nsigma@1.0.0`) and `RobustScale{k, scale: Mad|Qn}` (`filter.outlier.robust_scale@1.0.0`), each with a required side parameter (both, positive, negative). Standard deviation via `fid_num::red`; medians by stable sort with `fid_num::f::total_cmp`; MAD scaled by 1.4826; Qn per Rousseeuw and Croux 1993. Zero scale excludes nothing and reports `zero_scale`. Apply the `policy` guards before returning. Tests: `fixtures/out/stat-contam-v1` (planted outliers found, clean false-exclusion <= 1 %), independent test-side n-sigma mask, cross-target identity on x86-64/ARM64/wasm32, zero-scale case. KNP-1: no FMA, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.OUT.STAT.

## Open questions

- True M-estimator weighting (IRLS with Huber or Tukey weights) needs per-point weights in `KRN.LSQ.TRAIT`'s `PointsView`; this leaf provides exclusion-based robust filtering only. Add weights by a later leaf if incumbents' "robust" fits are weight-based [unverified].

## Children

_Leaf: none._
