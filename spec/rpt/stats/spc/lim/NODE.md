---
id: RPT.STATS.SPC.LIM
title: Control-limit calculation
level: 4
parent: RPT.STATS.SPC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.SPC-001, REQ-RPT.STATS.SPC-002, REQ-RPT.STATS.SPC-007]
covers: [GOM.TRD.05]
depends_on: [RPT.STATS.SPC.SUB, RPT.STATS.SPC.CONST, RPT.STATS.DSET.MAT, KRN.NUM]
side_effect: R1
determinism: D0
cli: [fid stats compute-limits]
mcp: [stats_compute_limits]
journal: [stats.compute_limits@1]
updated: 2026-11-17
---

# RPT.STATS.SPC.LIM: Control-limit calculation

## Purpose

Computes centre lines and control limits for X̄-R, X̄-s and individuals/moving-range (I-MR) charts from a subgrouped data set, using the classic Shewhart three-sigma formulas and the constants table of `RPT.STATS.SPC.CONST`. It also exposes the subgroup statistics (means, ranges, standard deviations, moving ranges and their averages) as a kernel function, which the chart builder and the within-subgroup capability estimators reuse.

## Scope

**In:**
- Kernel function `subgroup_stats` (subgroup means, ranges, sample standard deviations, moving ranges; grand mean, R̄, s̄, MR̄)
- Kernel function `limits` for chart types `XBAR_R`, `XBAR_S` and `IMR`
- Command `stats.compute_limits@1` (R1) and type `ControlLimits`
- Minimum number of subgroups and `INSUFFICIENT_DATA`

**Out:**
- Subgrouping (`RPT.STATS.SPC.SUB`) and the constants (`RPT.STATS.SPC.CONST`)
- Freezing limits (`RPT.STATS.SPC.FREEZE`), run rules (`RPT.STATS.SPC.RULES`), chart series (`RPT.STATS.SPC.CHART`)

## Requirements

- **REQ-RPT.STATS.SPC.LIM-001** `subgroup_stats` shall compute, for each used subgroup, its mean, range (maximum minus minimum) and sample standard deviation (divisor n − 1, two-pass), and over all used subgroups the grand mean, R̄ and s̄, each with `fid-num` fixed-order reductions in subgroup order.
- **REQ-RPT.STATS.SPC.LIM-002** For individuals, `subgroup_stats` shall compute the moving ranges |x_i − x_(i−1)| for i ≥ 2 in data-set order and their mean MR̄.
- **REQ-RPT.STATS.SPC.LIM-003** For `XBAR_R`, the limits shall be: X̄ chart centre = grand mean, limits = grand mean ± A2·R̄; R chart centre = R̄, LCL = D3·R̄, UCL = D4·R̄.
- **REQ-RPT.STATS.SPC.LIM-004** For `XBAR_S`, the limits shall be: X̄ chart centre = grand mean, limits = grand mean ± A3·s̄; s chart centre = s̄, LCL = B3·s̄, UCL = B4·s̄.
- **REQ-RPT.STATS.SPC.LIM-005** For `IMR`, the limits shall be: I chart centre = mean of the individuals, limits = mean ± E2·MR̄; MR chart centre = MR̄, LCL = 0, UCL = D4(2)·MR̄.
- **REQ-RPT.STATS.SPC.LIM-006** Each `ControlLimits` shall record the chart type, subgroup size, number of used subgroups, the constants table ID, version and review status, the algorithm ID `stats.spc.limits@1.0.0`, the kernel version, the data-set digest, and the used and excluded subgroups with reasons.
- **REQ-RPT.STATS.SPC.LIM-007** If fewer used subgroups exist than the configured minimum (default 20 [unverified]; individuals: 20 values [unverified]), then the limits shall be `INSUFFICIENT_DATA` and no number shall be produced.
- **REQ-RPT.STATS.SPC.LIM-008** If any value is non-finite, then the command shall fail with `stats.non_finite` naming the record ID.
- **REQ-RPT.STATS.SPC.LIM-009** The method documentation shall cite the limit formulas by published reference or standard designator and clause only (for example Shewhart, 1931, or ISO 7870-2 [unverified edition and clause]), and shall embed no standards text.
- **REQ-RPT.STATS.SPC.LIM-010** Results shall be bit-identical on every supported target for the same data set, subgrouping, constants table version and algorithm version (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/spc_xbar_r_25x5` (25 subgroups of 5 hand-chosen values with hand-computed grand mean, R̄ and s̄ and their limits using the seed constants for n = 5), when `fid stats compute-limits --dataset <id> --char C1 --chart xbar-r --subgroup fixed:5 --json` runs, then centre lines and limits equal the expected values within 1 ulp-scaled tolerance of 4 × machine epsilon relative [unverified tolerance], and the result names table `spc.constants.normal@1.0.0` with status `UNVERIFIED`.
- **AC-2** Given the same data and `--chart xbar-s`, then the X̄ and s limits equal the hand-computed values with A3, B3 and B4 for n = 5.
- **AC-3** Given fixture `fixtures/rpt/stats/spc_imr_30` (30 individuals), when `--chart imr` runs, then MR̄ is the mean of the 29 moving ranges and the MR chart LCL is 0.
- **AC-4** (insufficient data) Given 19 complete subgroups of 5 and the default minimum, when the command runs, then the limits are `INSUFFICIENT_DATA` and contain no number.
- **AC-5** (exclusions) Given 23 entries and `fixed:5`, when the command runs, then the result lists the tail subgroup as `INCOMPLETE_SUBGROUP`.
- **AC-6** (determinism and replay) Given AC-1's inputs, when run on x86-64, ARM64 and wasm32, then result digests are identical; a journal that froze these limits replays `IDENTICAL`.

## Design

- **Registry command:** `stats.compute_limits@1(dataset: DataSetId, char: CharId, chart: ChartType, subgrouping: SubgroupMode, min_subgroups: Option<u32>) -> ControlLimits`, R1.
- **Kernel functions:** `subgroup_stats(values: &[f64], groups: &Subgrouping) -> Result<SubgroupStats, StatsError>`; `limits(stats: &SubgroupStats, chart: ChartType, k: &SpcConstants) -> LimitValues`.
- **Data structures:** `enum ChartType { XbarR, XbarS, Imr }`; `LimitValues { location: Lines, dispersion: Lines }` with `Lines { centre, lcl, ucl }`; `ControlLimits { char, unit, chart, n, k_used, values: LimitsOrInsufficient, constants: TableRef{id, version, status}, algorithm, kernel_version, dataset_digest, subgroups_used, subgroups_excluded }`.
- **Algorithm:** REQ-001 to REQ-005; all sums are `fid-num` fixed-order reductions; sqrt via `fid-num`; plain multiply then add (no FMA). Two-pass sample variance per subgroup as in `RPT.STATS.DESC`.
- **Errors:** `stats.non_finite`, `stats.subgroup_size_unsupported`, `stats.char_not_in_dataset`, `stats.chart_needs_subgroups` (X̄ charts with `INDIVIDUALS`), `stats.chart_needs_individuals` (I-MR with n > 1).
- **Determinism:** D0.
- **Performance budget:** 10 000 subgroups in under 10 ms after materialisation [unverified until measured].
- **Crate:** `fid-stats` (kernel graph, KNP-1), holding the kernel functions and the command descriptor, which reads the data set only through the `DataSetReader` trait (implemented in `fid-stats-cmd`), as `RPT.STATS.DESC` does.

## Test fixtures

- `fixtures/rpt/stats/spc_xbar_r_25x5` and `spc_imr_30`: project-authored synthetic values with hand-computed expected limits (spreadsheet working kept beside the fixture), CC-BY-4.0. Expected values depend on the constants table version and are regenerated if it changes.

## Build prompt

> In crate `fid-stats` (kernel graph, KNP-1: no FMA, `fid-num` reductions and sqrt, no std HashMap/HashSet, no I/O, NaN is an error), implement `subgroup_stats(values, groups)` (per-subgroup mean, range, two-pass sample standard deviation; grand mean, R̄, s̄; for individuals the moving ranges |x_i − x_(i−1)| and MR̄) and `limits(stats, chart, constants)` for `XbarR` (centre grand mean, ± A2·R̄; R chart D3·R̄, R̄, D4·R̄), `XbarS` (± A3·s̄; s chart B3·s̄, s̄, B4·s̄) and `Imr` (mean ± E2·MR̄; MR chart 0, MR̄, D4(2)·MR̄). Constants come from `constants(n)` (module SPC.CONST) and the result records table ID, version and review status.
>
> In the same crate, register the command descriptor `stats.compute_limits@1(dataset, char, chart, subgrouping, min_subgroups?) -> ControlLimits` (R1; CLI `fid stats compute-limits`, MCP `stats_compute_limits`, `--json`). Read the data set only through the `DataSetReader` trait (module DSET.MAT; use a test stub). Subgroup with `subgroup()` (module SPC.SUB). Fewer used subgroups than the minimum (default 20, configurable) -> `INSUFFICIENT_DATA` with no number. Record algorithm ID `stats.spc.limits@1.0.0`, kernel version, data-set digest, used and excluded subgroups.
>
> **Tests:** AC-1 to AC-6 above with fixtures `fixtures/rpt/stats/spc_xbar_r_25x5` and `spc_imr_30`, plus a cross-target digest test.
>
> **Constraints:** CONSTITUTION §8; C1; cite methods by reference only, never paste standards text. Spec: `spec/rpt/stats/spc/lim/NODE.md`.

## Open questions

- **Expert choice (flagged):** the default minimum number of subgroups (20 here [unverified]) and whether R̄-based or s̄-based limits are the default for n ≥ 10 [unverified].

## Children

_Leaf: none._
