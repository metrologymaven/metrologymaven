---
id: RPT.STATS.TREND
title: Trend series and the chart-series contract
level: 3
parent: RPT.STATS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-003]
covers: [GOM.TRD.02]
depends_on: [RPT.STATS.DSET]
cli: [fid stats build-chart]
mcp: [stats_build_chart]
journal: [stats.build_chart@1]
updated: 2026-11-17
---

# RPT.STATS.TREND: Trend series and the chart-series contract

## Purpose

Builds trend series (value per characteristic over stages, runs or parts, with nominal and tolerance limits) and histograms, and defines the `ChartSeries` contract that every chart source (trend, histogram, control chart from SPC, vector group plot from LVM) hands to report chart blocks and to VIZ. Owning the contract here keeps chart blocks free of computation.

## Scope

**In:**
- Trend series ordered by stage, run time or part sequence, with nominal and limit lines
- Histogram series with deterministic bin edges
- `ChartSeries` type: series kind, axis units, points, reference lines, markers (e.g. out-of-tolerance, run-rule violation), provenance (data-set digest, algorithm ID)
- `stats.build_chart@1` command

**Out** (and which node owns it instead):
- Control-chart limits and run rules (`RPT.STATS.SPC`)
- Chart layout in reports (`RPT.MODEL.BLOCKS`) and on screen (VIZ)

## Requirements

- **REQ-RPT.STATS.TREND-001** The system shall produce a trend series per characteristic ordered by stage, run time or part sequence as chosen by the user, with ties broken by record ID.
- **REQ-RPT.STATS.TREND-002** A trend series shall include the nominal and the tolerance limits as reference lines and mark each point that is out of tolerance according to its journaled decision.
- **REQ-RPT.STATS.TREND-003** Histogram bin edges shall be computed deterministically from the data set and a stated binning rule, and the rule ID shall be recorded in the series.
- **REQ-RPT.STATS.TREND-004** Every `ChartSeries` shall carry the data-set digest and the algorithm ID that produced it.
- **REQ-RPT.STATS.TREND-005** The `ChartSeries` contract shall be versioned, and chart blocks shall reject a series whose major version they do not support with error `report.chart_series_unsupported`.

## Interfaces

- **Offers:** `stats.build_chart@1` (R1); type `ChartSeries` (contract used by `RPT.MODEL.BLOCKS`, `RPT.STATS.SPC`, LVM and VIZ).
- **Consumes:** data sets (`RPT.STATS.DSET`).

## Decomposition hints

CONTRACT (ChartSeries type and versioning), TREND (trend builder), HIST (histogram builder).

## Open questions

- Default histogram binning rule: chosen at leaf level as `sturges@1` (`RPT.STATS.TREND.HIST`); Freedman-Diaconis is deferred until a quantile definition is chosen.
- Control charts are built by a separate command, `stats.build_control_chart@1`, owned by `RPT.STATS.SPC.CHART`; `stats.build_chart@1` covers trend and histogram kinds only.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.STATS.TREND.CONTRACT | ChartSeries contract and versioning (leaf) | 004 005 | |
| RPT.STATS.TREND.HIST | Histogram binning kernel (leaf) | 003 | |
| RPT.STATS.TREND.BUILD | Trend and histogram chart builder (leaf) | 001 002 003 004 | GOM.TRD.02 |
