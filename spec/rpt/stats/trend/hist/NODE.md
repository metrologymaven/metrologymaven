---
id: RPT.STATS.TREND.HIST
title: Histogram binning kernel
level: 4
parent: RPT.STATS.TREND
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-stats]
satisfies: [REQ-RPT.STATS.TREND-003]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.STATS.TREND.HIST: Histogram binning kernel

## Purpose

The pure kernel function that turns an ordered list of values and a named binning rule into bin edges and counts. Bin edges depend only on the values and the rule, so the same data set always gives the same histogram, and the rule ID is recorded so a reader knows how the bins were chosen.

## Scope

**In:**
- Binning rules `sturges@1` (default), `fixed_count@1` and `fixed_width@1`
- Edge computation, value-to-bin assignment and counts
- Degenerate cases (no values, all values equal)

**Out:**
- The `stats.build_chart@1` command and turning bins into a `ChartSeries` (`RPT.STATS.TREND.BUILD`)
- Rules that need quantiles, such as Freedman-Diaconis (see Open questions)

## Requirements

- **REQ-RPT.STATS.TREND.HIST-001** For rule `sturges@1` and n ≥ 1 values, the function shall use k = ⌈log2 n⌉ + 1 bins, computing ⌈log2 n⌉ with integer operations only (the bit length of n − 1 for n ≥ 2, and 0 for n = 1).
- **REQ-RPT.STATS.TREND.HIST-002** For rules `sturges@1` and `fixed_count@1`, the edges shall be e_i = min + i·w for i = 0..k−1 with w = (max − min)/k, and e_k shall be set exactly to max.
- **REQ-RPT.STATS.TREND.HIST-003** For rule `fixed_width@1` with width w > 0 and anchor a, the edges shall be a + j·w for consecutive integers j from ⌊(min − a)/w⌋ to the smallest j whose edge is greater than max, computed with `fid-num` floor.
- **REQ-RPT.STATS.TREND.HIST-004** A value x shall be counted in bin i when e_i ≤ x < e_{i+1}, and the last bin shall also include x = e_k; the bin shall be found by binary search over the edges, not by dividing by the width.
- **REQ-RPT.STATS.TREND.HIST-005** If all values are equal, then the function shall return one bin with both edges equal to that value and count n, flagged `DEGENERATE`.
- **REQ-RPT.STATS.TREND.HIST-006** If n = 0, then the function shall return `INSUFFICIENT_DATA`; if the bin count would exceed 1 000 or a width is not positive and finite, then it shall fail with `stats.bad_binning` and return no bins.
- **REQ-RPT.STATS.TREND.HIST-007** The result shall record the rule ID and version, and the sum of the counts shall equal n.
- **REQ-RPT.STATS.TREND.HIST-008** The function shall be bit-identical on every supported target (D0) and follow KNP-1.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/hist_cases.json` (hand-computed cases: n = 1, 2, 3, 8, 9, 100 with `sturges@1`; values on bin edges; all-equal values; `fixed_width@1` with a negative anchor), when the function runs, then the edges and counts equal the expected values exactly.
- **AC-2** (edge assignment) Given values {0, 1, 2, 3, 4} with `fixed_count@1` k = 4, when binned, then edges are {0, 1, 2, 3, 4} and counts are {1, 1, 1, 2}.
- **AC-3** (error path) Given `fixed_width@1` with width 0, when the function runs, then it fails with `stats.bad_binning` and returns no bins.
- **AC-4** (determinism) Given fixture `fixtures/rpt/stats/hist_random_10k` (10 000 values from the `fid-num` counter-based RNG with seed 42), when binned on x86-64, ARM64 and wasm32, then the result digests are identical.

## Design

- **Registry commands:** none (kernel function). `side_effect: R1` is that of its only caller, `stats.build_chart@1`.
- **Kernel function:** `fn histogram(values: &[f64], rule: &BinRule) -> Result<Bins, StatsError>`.
- **Data structures:** `enum BinRule { Sturges, FixedCount { k: u32 }, FixedWidth { width: f64, anchor: f64 } }`; `Bins { rule: RuleId, edges: Vec<f64>, counts: Vec<u64>, degenerate: bool }`; `Stat::InsufficientData` for n = 0.
- **Algorithm:** min and max with `fid-num` total-order operations; edges as in REQ-002/003 with plain IEEE multiplication and addition (no FMA); assignment by binary search over edges (REQ-004). Sturges' rule: H. A. Sturges, 1926, Journal of the American Statistical Association 21(153) [unverified citation details].
- **Errors:** `stats.bad_binning{reason}`, `stats.non_finite{index}`.
- **Determinism:** D0: only IEEE basic operations and `fid-num` floor and min/max.
- **Performance budget:** 1 M values binned in under 50 ms [unverified until measured].
- **Crate:** `fid-stats` (kernel graph; KNP-1).

## Test fixtures

- `fixtures/rpt/stats/hist_cases.json`: hand-computed cases, CC-BY-4.0.
- `fixtures/rpt/stats/hist_random_10k`: generated at test time from the `fid-num` RNG with seed 42 (no stored data).

## Build prompt

> In crate `fid-stats` (kernel graph, so KNP-1 applies: no FMA or `mul_add`, `fid-num` for floor and total-order min/max, no std HashMap/HashSet, no I/O, NaN is an error), implement `histogram(values: &[f64], rule: &BinRule) -> Result<Bins, StatsError>`.
>
> **Rules:** `sturges@1` (default): k = ceil(log2 n) + 1 with ceil(log2 n) computed as the bit length of n − 1 (0 for n = 1), integer-only. `fixed_count@1 {k}`. For both: w = (max − min)/k, e_i = min + i·w for i < k, e_k = max exactly. `fixed_width@1 {width, anchor}`: edges a + j·w from j = floor((min − a)/w) until the first edge greater than max. Assign value x to bin i when e_i ≤ x < e_{i+1}, last bin closed, using binary search over the edges. All-equal values: one bin [v, v], count n, `degenerate = true`. n = 0: `InsufficientData`. More than 1 000 bins, or non-positive or non-finite width: `stats.bad_binning`. Record the rule ID; counts sum to n.
>
> **Tests:** AC-1 to AC-4 above with `fixtures/rpt/stats/hist_cases.json` and a seeded 10 000-value cross-target digest test (x86-64, ARM64, wasm32).
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C1. Spec: `spec/rpt/stats/trend/hist/NODE.md`.

## Open questions

- The parent asked for a default binning rule; this leaf chooses `sturges@1` because it needs no quantile definition. A Freedman-Diaconis rule (`fd@1`) needs a quantile definition (for example Hyndman and Fan type 7 [unverified as the usual default]) and can be added as a later minor version.
- Built in `fid-stats` (module `hist`). Choices made at build time, to confirm or reword in REQ-003/006: (1) `fid-num` has no floor function, so `⌊t⌋` is computed exactly by integer truncation for `|t| < 2^52`, and a larger `|t|` (or a non-finite one) is refused as `stats.bad_binning{anchor_too_far}`. (2) When `(min − a)/w` rounds up to an integer so that `a + j0·w > min` (for example a = 0, w = 0.1, min = 1.7), the literal floor would leave `min` outside every bin; the kernel steps `j0` down by one. When the quotient rounds down instead, the first bin can be empty, as REQ-003 is written. (3) Edges that are not strictly increasing because `f64` cannot resolve the width at the data's magnitude (min = 1e16, max = 1e16 + 2, k = 4) are refused as `edges_not_increasing`, and so are a range or edge that overflows (`range_not_finite`, `edge_not_finite`), a NaN or infinite anchor (`anchor_not_finite`) and `fixed_count` with k = 0 (`zero_bins`). (4) `INSUFFICIENT_DATA` (n = 0) is the error `stats.insufficient_data`, the same code `RPT.STATS.SPC.FREEZE` uses. (5) Rule parameters are checked before the data, so a bad rule is refused even for an empty data set. (6) The 50 ms per 1 M values performance budget has not been measured.

## Children

_Leaf: none._
