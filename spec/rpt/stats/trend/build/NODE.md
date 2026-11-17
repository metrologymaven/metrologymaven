---
id: RPT.STATS.TREND.BUILD
title: Trend and histogram chart builder
level: 4
parent: RPT.STATS.TREND
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.TREND-001, REQ-RPT.STATS.TREND-002, REQ-RPT.STATS.TREND-003, REQ-RPT.STATS.TREND-004]
covers: [GOM.TRD.02]
depends_on: [RPT.STATS.TREND.CONTRACT, RPT.STATS.TREND.HIST, RPT.STATS.DSET.MAT, GDT.CHAR]
side_effect: R1
determinism: D0
cli: [fid stats build-chart]
mcp: [stats_build_chart]
journal: [stats.build_chart@1]
updated: 2026-11-17
---

# RPT.STATS.TREND.BUILD: Trend and histogram chart builder

## Purpose

The `stats.build_chart@1` command: from a data set and a characteristic it builds either a trend series (one point per used record, in the chosen order, with nominal and tolerance lines and out-of-tolerance markers taken from the journaled decisions) or a histogram series (bins from `RPT.STATS.TREND.HIST`). It produces `ChartSeries` values only; layout and drawing belong to report blocks and the viewer.

## Scope

**In:**
- `stats.build_chart@1` with kinds `TREND` and `HISTOGRAM`
- Trend ordering by stage, run time or part sequence, with ties by record ID
- Reference lines (nominal, LSL, USL) and decision markers
- Provenance on every series

**Out:**
- The `ChartSeries` type (`RPT.STATS.TREND.CONTRACT`)
- Binning (`RPT.STATS.TREND.HIST`)
- Control-chart series (`RPT.STATS.SPC.CHART`)
- Ordering and digest of the data set itself (`RPT.STATS.DSET.MAT`)

## Requirements

- **REQ-RPT.STATS.TREND.BUILD-001** When `stats.build_chart@1` runs with kind `TREND`, the system shall return one point per used entry of the characteristic, with x the 1-based position and y the value, in the data set's order.
- **REQ-RPT.STATS.TREND.BUILD-002** When the request names an ordering key different from the data set's, the system shall materialise a copy of the definition with that key (ties by record ID), and the series shall carry that copy's data-set digest.
- **REQ-RPT.STATS.TREND.BUILD-003** Each trend point shall carry the record ID, the piece ID and an x label taken from the ordering key (stage name, UTC run time, or part sequence number).
- **REQ-RPT.STATS.TREND.BUILD-004** A trend series shall include a `NOMINAL` line and an `LSL` and `USL` line for each specification limit the characteristic has, in the characteristic's unit.
- **REQ-RPT.STATS.TREND.BUILD-005** A point whose journaled decision is `NONCONFORMS` shall carry an `OUT_OF_TOLERANCE` marker and one whose decision is `INDETERMINATE` an `INDETERMINATE` marker, and the builder shall not compare values with limits itself.
- **REQ-RPT.STATS.TREND.BUILD-006** When kind is `HISTOGRAM`, the system shall bin the used values with the requested rule (default `sturges@1`), return one point per bin, include the `NOMINAL`, `LSL` and `USL` lines, and record the rule ID in the provenance.
- **REQ-RPT.STATS.TREND.BUILD-007** Every series shall carry the data-set digest, the algorithm ID (`stats.trend.series@1.0.0` or `stats.trend.histogram@1.0.0`) and the kernel version.
- **REQ-RPT.STATS.TREND.BUILD-008** If the characteristic is not in the data set, then the command shall fail with `stats.char_not_in_dataset`; if it has no used entries, then the series shall have no points and status `INSUFFICIENT_DATA`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/three_part_run` (part 2 nonconforming on characteristic C3), when `fid stats build-chart --dataset <id> --char C3 --kind trend --json` runs, then the series has three points in part order, `NOMINAL`, `LSL` and `USL` lines, and an `OUT_OF_TOLERANCE` marker only on part 2.
- **AC-2** (re-order) Given the same data set and `--order run-time` where run times reverse the part order, when the command runs, then the points are reversed and the provenance digest differs from AC-1's.
- **AC-3** (one-sided) Given characteristic C5 with only an upper limit, when a trend is built, then there is a `USL` line and no `LSL` line.
- **AC-4** (histogram) Given fixture `fixtures/rpt/stats/hist_project_50` (50 parts, one characteristic), when `--kind histogram` runs with the default rule, then there are 7 bins (Sturges, n = 50), the counts sum to 50 and the provenance names `sturges@1`.
- **AC-5** (error path) Given a characteristic ID not in the data set, when the command runs, then it fails with `stats.char_not_in_dataset`.
- **AC-6** (determinism and replay) Given AC-1's series, when built on x86-64, ARM64 and wasm32 the canonical digests are identical, and a journal that used it in a report replays `IDENTICAL`.

## Design

- **Registry command:** `stats.build_chart@1(dataset: DataSetId, char: CharId, kind: ChartKind, order: Option<OrderKey>, rule: Option<BinRule>) -> ChartSeries`, R1.
- **Data structures:** `enum ChartKind { Trend, Histogram }` (control charts have their own command in `RPT.STATS.SPC.CHART`).
- **Algorithm:** materialise through `DataSetReader` (DSET.MAT); look up nominal and specification limits from the characteristic model (GDT.CHAR); map entries to points; set markers from each entry's journaled outcome; for histograms call `histogram` (TREND.HIST) and map bins to points (x = lower edge, `x_upper` = upper edge, y = count).
- **Errors:** `stats.char_not_in_dataset`, `stats.dataset_not_found`, `stats.bad_binning`, `report.chart_series_invalid` (should never surface; a validation failure is a bug).
- **Determinism:** D0; no arithmetic on values except in HIST.
- **Performance budget:** a 10 000-point trend in under 30 ms after materialisation [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/three_part_run`: shared synthetic project (C3 nonconforming on part 2, C5 upper limit only).
- `fixtures/rpt/stats/hist_project_50`: synthetic project, 50 runs, values from the `fid-num` RNG with seed 7, CC-BY-4.0.

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement `stats.build_chart@1(dataset, char, kind: Trend|Histogram, order: Option<OrderKey>, rule: Option<BinRule>) -> ChartSeries`, class R1; CLI `fid stats build-chart`, MCP `stats_build_chart`, `--json`.
>
> **Trend:** read the data set through `DataSetReader` (module DSET.MAT); if `order` differs from the definition's, materialise a copy with that key. One point per used entry: x = 1-based position, y = value, record ID, piece ID, label from the key. Reference lines `NOMINAL`, and `LSL`/`USL` only for limits the characteristic has (from the GDT characteristic model). Markers from the journaled decision only: `NONCONFORMS` -> `OUT_OF_TOLERANCE`, `INDETERMINATE` -> `INDETERMINATE`; never compare values with limits. Algorithm ID `stats.trend.series@1.0.0`.
>
> **Histogram:** call `fid_stats::histogram(values, rule)` (default `sturges@1`); one point per bin (x = lower edge, `x_upper` = upper edge, y = count); same reference lines; algorithm ID `stats.trend.histogram@1.0.0`; provenance records the rule ID.
>
> **Provenance:** data-set digest, algorithm ID, kernel version, using `fid-chart-series` types; validate before returning. Errors: `stats.char_not_in_dataset`, `stats.dataset_not_found`, `stats.bad_binning`. No used entries -> empty series with status `INSUFFICIENT_DATA`.
>
> **Tests:** AC-1 to AC-6 above with fixtures `fixtures/rpt/stats/three_part_run` and `hist_project_50`.
>
> **Constraints:** CONSTITUTION §8; C1 (no value of record computed outside the kernel). Spec: `spec/rpt/stats/trend/build/NODE.md`.

## Open questions

- The specification limits (LSL, USL) of a characteristic are read from the GDT characteristic model; a single accessor for them (shared with `RPT.STATS.CAP`) should be offered by GDT. Until then this leaf depends on `GDT.CHAR` as a whole.

## Children

_Leaf: none._
