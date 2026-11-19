---
id: RPT.MODEL.BLOCKS.CHART
title: Chart block referencing ChartSeries
level: 4
parent: RPT.MODEL.BLOCKS
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL.BLOCKS-001, REQ-RPT.MODEL.BLOCKS-004, REQ-RPT.MODEL.BLOCKS-010, REQ-RPT.MODEL.BLOCKS-011]
covers: [GOM.RPT.05, PCD.RPT.03, SA.RPT.02]
depends_on: [RPT.MODEL.DOC, RPT.MODEL.BIND.RESOLVE, RPT.STATS.TREND]
side_effect: W2
determinism: D0
cli: [fid report set-chart-block]
mcp: [report_set_chart_block]
journal: [report.set_chart_block@1]
updated: 2026-11-17
---

# RPT.MODEL.BLOCKS.CHART: Chart block referencing ChartSeries

## Purpose

The `chart` block kind: places a trend chart, histogram, control chart or vector group plot in a report. It stores a chart type, a reference to a `ChartSeries` definition by stable ID and display options (title, axis labels, legend on or off). It computes nothing: series values come from `RPT.STATS` (trend, histogram, control chart) or from LVM (vector group plots), which publish `ChartSeries` under the contract owned by `RPT.STATS.TREND`.

## Scope

**In:** `chart` payload schema v1; chart types `trend`, `histogram`, `control`, `vector_group`; `BlockResolver` implementation that fetches the series at the generation head and copies it unchanged into the resolved block; `report.set_chart_block@1` (W2).

**Out:** computing series (`RPT.STATS.TREND`, `RPT.STATS.SPC`, LVM); drawing charts in PDF (`RPT.OUTPUT.PDF.CHART`) and on screen (VIZ).

## Requirements

- **REQ-RPT.MODEL.BLOCKS.CHART-001** The `chart` kind shall register payload schema version 1 holding chart type, series reference, untrusted title and axis labels of at most 128 bytes each, and a legend flag.
- **REQ-RPT.MODEL.BLOCKS.CHART-002** When resolved, the block shall obtain the `ChartSeries` for its reference at the generation head and place it in the resolved block unchanged, and the resolver shall perform no arithmetic on series values.
- **REQ-RPT.MODEL.BLOCKS.CHART-003** If the referenced series' chart type differs from the block's chart type, then `report.set_chart_block@1` shall fail with error `report.chart_type_mismatch` and journal nothing.
- **REQ-RPT.MODEL.BLOCKS.CHART-004** If no series provider is installed for the chart type, then generation shall fail with error `report.series_unavailable` naming the block.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/trend_20parts` with a trend series S, when `fid report set-chart-block --type trend --series S --title "Bore 1" --json` runs, then one event is journaled; generation copies S's canonical bytes into the resolved block byte for byte.
- **AC-2** (error path) Given S of type trend, when `--type histogram` is set, then it fails with `report.chart_type_mismatch`.
- **AC-3** (error path) Given a `vector_group` block and no LVM provider installed, when generated, then it fails with `report.series_unavailable`.
- **AC-4** (static check) Given the crate, when the KRN.NUM float-lint rule set runs on the resolver module, then it reports no arithmetic on series values.

## Design

- **Registry command:** `report.set_chart_block@1(report, block, chart_type, series: SeriesRef, title, x_label, y_label, legend: bool)` (W2).
- **Data structures:** `ChartPayload { chart_type, series: SeriesRef, title, x_label, y_label, legend }`; `trait SeriesProvider { fn series(&self, r: &SeriesRef, head) -> Result<ChartSeries, ...>; }` registered per chart type by STATS or LVM crates.
- **Errors:** `report.chart_type_mismatch`, `report.series_unavailable`, `report.invalid_block_payload`.
- **Determinism:** D0 (copy of a digested series).
- **Performance budget:** negligible beyond the provider.
- **Crate:** `fid-report`.

## Test fixtures

`fixtures/rpt/stats/trend_20parts` (a 20-part synthetic data set and its trend series, from `RPT.STATS.TREND` tests).

## Build prompt

> In crate `fid-report`, add the `chart` block kind (payload v1: chart type `trend|histogram|control|vector_group`, `SeriesRef`, untrusted title and axis labels up to 128 bytes, legend flag), a `SeriesProvider` registry keyed by chart type, and a `BlockResolver` that copies the provider's `ChartSeries` (contract from `RPT.STATS.TREND`) unchanged into the resolved block. Command `report.set_chart_block@1` (W2; CLI `fid report set-chart-block --json`); type mismatch -> `report.chart_type_mismatch`; missing provider at generation -> `report.series_unavailable`. No arithmetic on series values. Acceptance: AC-1..AC-4 in `spec/rpt/model/blocks/chart/NODE.md`. Constraints: CONSTITUTION C1, C6, §8.

## Open questions

- Phase P1b: the series sources (`RPT.STATS`) start in P1b; P1a reports ship without chart blocks, and the PDF exporter reports `report.unsupported_block` for charts until `RPT.OUTPUT.PDF.CHART` lands.
- LVM sits above RPT in the L2 order, so vector group plots arrive through the `SeriesProvider` registry, never through a code edge from RPT to LVM.

## Children

_Leaf: none._
