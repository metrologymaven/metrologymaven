---
id: GDT.DIM.SIZEISO.RANK
title: Rank-order statistics over local sizes
level: 4
parent: GDT.DIM.SIZEISO
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-gps]
satisfies: [REQ-GDT.DIM.SIZEISO-003, REQ-GDT.DIM.SIZEISO-004]
covers: []
depends_on: [KRN.NUM]
uses_artifacts: [UNC.ORS.SIZE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DIM.SIZEISO.RANK: Rank-order statistics over local sizes

## Purpose

Reduces a list of local sizes to one number with a named statistic: maximum, minimum, mean, median, mid-range, range or standard deviation. Which statistic a given ISO rank-order modifier means, and the conventions where statistics have variants (median of an even count, standard-deviation denominator), are parameters the ISO row supplies; this leaf implements the closed set of statistics and every variant, deterministically.

## Scope

**In:** `rank_statistic(sizes: &[f64], stat: RankStat) -> f64`; `RankStat { Max, Min, Mean, Median{even: MeanOfMiddle | Lower | Upper}, MidRange, Range, StdDev{denominator: N | NMinus1} }`; fixed-order summation; total-order sort.
**Out:** producing local sizes (owned by `GDT.DIM.SIZEISO.LOCAL`); mapping modifiers to statistics (owned by DMT rows in `GDT.RES.ROWISO`); dispatch (owned by `GDT.DIM.SIZEISO.DISP`).

## Requirements

- **REQ-GDT.DIM.SIZEISO.RANK-001** The function shall compute the statistic named by the chain over the given local sizes, with every variant parameter taken from the chain and none defaulted in code.
- **REQ-GDT.DIM.SIZEISO.RANK-002** Sums shall be computed in fixed index order with compensated (Neumaier) summation through `fid-num`, and sorting shall use a total order on f64.
- **REQ-GDT.DIM.SIZEISO.RANK-003** If the list is empty, contains NaN, or `StdDev{NMinus1}` is asked for one value, then the function shall return `empty_sizes`, `nan_input` or `insufficient_count`.
- **REQ-GDT.DIM.SIZEISO.RANK-004** Statistics shall match every ORS-SIZE ISO rank-order case within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/size/rank_sizes.json` (10 dyadic values with known statistics), when each `RankStat` variant runs, then results match exactly (bit for bit) for max, min, range, mid-range and median, and within 1 ulp for mean and standard deviation.
- **AC-2** Given an even-count list, then the three median variants return the mean of the middle pair, the lower and the upper middle value.
- **AC-3** (error path) Given an empty list, one value with `StdDev{NMinus1}`, or a NaN, then the matching typed error.
- **AC-4** (determinism) Given 1 M seeded sizes (seed `0x512e`), then results are bit-identical on all CI platforms and independent of input order for max, min, median, range and mid-range.

## Design

- **Registry command(s):** none (kernel function).
- **Data structures:** `RankStat` as in Scope.
- **Algorithm:** copy and total-order sort for order statistics; Neumaier sum for mean; two-pass variance (mean first, then squared deviations) with `fid-num` square root.
- **Errors:** `empty_sizes`, `nan_input`, `insufficient_count`.
- **Determinism:** D0.
- **Performance budget:** 1 M values < 100 ms [unverified].
- **Crate:** `fid-gps` (module `dim::size::iso::rank`).

## Test fixtures

Hand-built dyadic value lists and a seeded generator (CC0); ORS-SIZE suite from `UNC.ORS.SIZE`.

## Build prompt

> In crate `fid-gps` (kernel), module `dim::size::iso::rank`, implement `rank_statistic(sizes, stat) -> Result<f64, RankError>` for `RankStat { Max, Min, Mean, Median{even}, MidRange, Range, StdDev{denominator} }`, where every variant parameter comes from the caller (no defaults). Use a total-order sort for order statistics, Neumaier summation in index order for the mean, and a two-pass variance with the `fid-num` square root. Errors: `empty_sizes`, `nan_input`, `insufficient_count`. Tests per acceptance criteria (exact results on dyadic fixtures, platform bit-identity). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` facade, fixed-order reductions, total-order min/max, NaN is an error).

## Open questions

- **Built** in `fid-gps` `dim::size::iso::rank` (`rank_statistic`, `RankStat`, `MedianEven`, `StdDenominator`, `RankError`). AC-1 to AC-4 are met on invented fixtures `fixtures/gdt/size/rank_sizes.json` and `rank_seeded.json`, whose expected values come from the oracle `fixtures/gdt/size/make_rank_fixtures.py`. Each `exact` value is the true statistic in rational arithmetic, rounded once (the square root via an integer square root with a bracketing check). Each `pinned` value is the written procedure, re-implemented in plain binary64 in the oracle. AC-1 uses `(10240 + k)·2⁻²⁰` m with a dyadic mean. The n − 1 standard deviation is 1 ulp from exact and the n one is exact. AC-2 includes a middle pair whose midpoint is a rounding tie. AC-4 pins the one-million-value SplitMix64 stream (seed 0x512e) by a SHA-256 and checks order independence under reversal and a fixed shuffle. The test runs on Linux, Windows, macOS and wasm32.
- **Rounding bounds** (stated in the module documentation). Max, min and the lower, upper and odd-count medians are exact input values. Range, mid-range and the mean-of-middle median are correctly rounded: the midpoint is `(a + b)·0.5`, with `a·0.5 + b·0.5` when the sum overflows. The mean is correctly rounded whenever the fixed-order sum is exact; otherwise its error is the sum's error divided by n, plus half an ulp. The standard deviation is not correctly rounded. Its first-order relative error of about 4u is derived but [unverified], and it does not hold in the subnormal range: the fixture `subnormal_midpoint_tie` is 1 ulp but a 100 % relative error.
- **Errors beyond the leaf's three.** An infinite size is `infinite_input{index}`, because a local size cannot be infinite and NaN-free arithmetic on infinities is undefined for mid-range and the mean. A sum, product, deviation or range that overflows is `overflow`, and no value is made up. Any other `fid-num` refusal passes through as `numeric`. The checks run in this order: `empty_sizes`, then the first non-finite element in index order, then `insufficient_count`. So a single NaN with `StdDev{NMinus1}` is `nan_input`.
- **Zero signs.** Order statistics use the total order, so −0.0 < +0.0. A zero range is +0.0. A zero mean is −0.0 only when every size is −0.0, which is the KRN.NUM.RED sum rule. A zero standard deviation is +0.0.
- The value is a number, not a conformance decision; UNC.DEC owns that. REQ-GDT.DIM.SIZEISO-004 (the ORS-SIZE cases) is not an AC of this leaf: `UNC.ORS.SIZE` publishes no cases yet.

## Children

_Leaf: none._
