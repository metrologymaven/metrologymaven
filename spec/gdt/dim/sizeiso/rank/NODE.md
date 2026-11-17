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

- None.

## Children

_Leaf: none._
