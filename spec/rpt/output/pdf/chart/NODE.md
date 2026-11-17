---
id: RPT.OUTPUT.PDF.CHART
title: Vector chart painter for ChartSeries
level: 4
parent: RPT.OUTPUT.PDF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT.PDF-004, REQ-RPT.OUTPUT.PDF-005]
covers: [GOM.RPT.06]
depends_on: [RPT.OUTPUT.PDF.DRAW, RPT.OUTPUT.PDF.LAYOUT, RPT.STATS.TREND]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.OUTPUT.PDF.CHART: Vector chart painter for ChartSeries

## Purpose

Draws `chart` blocks into the PDF as vectors: axes, grid, tick labels, series lines, points, bars, limit lines and out-of-control markers, for trend charts, histograms, control charts and vector group plots. It maps series data to page coordinates with integer arithmetic and shows only labels supplied in the `ChartSeries`, so no number on a chart is formatted outside the formatter layer.

## Scope

**In:** painter and `Measure` implementation for kind `chart`; the data-to-page mapping; drawing of the four chart types; tick and limit labels taken from the series.

**Out:** computing series, limits, ticks and their labels (`RPT.STATS.TREND`, `RPT.STATS.SPC`, LVM); the chart block payload (`RPT.MODEL.BLOCKS.CHART`).

## Requirements

- **REQ-RPT.OUTPUT.PDF.CHART-001** The chart painter shall register for block kind `chart` so that, once installed, chart blocks no longer fail with `report.unsupported_block`.
- **REQ-RPT.OUTPUT.PDF.CHART-002** Every text label on a chart (ticks, limits, values) shall be a `ValueText` display string supplied in the `ChartSeries`, painted unchanged.
- **REQ-RPT.OUTPUT.PDF.CHART-003** The mapping from series coordinates to page coordinates shall use the series' exact decimal axis range and integer µm page units with one half-even rounding per coordinate, so the same series gives byte-identical paths on every platform.
- **REQ-RPT.OUTPUT.PDF.CHART-004** Control charts shall draw centre line and control limits as distinct line styles and mark every point that the series flags as a rule violation.
- **REQ-RPT.OUTPUT.PDF.CHART-005** If a series has more than 100 000 points [unverified limit], then painting shall fail with error `report.chart_too_large` naming the block.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/trend_20parts` in a resolved report, when exported, then the chart's paths equal the golden content stream `fixtures/rpt/output/chart/trend_20parts.ops` on x86-64, ARM64 and wasm32.
- **AC-2** Given a control-chart series with two flagged points, when painted, then two violation markers appear at those points and the limits use the limit line style.
- **AC-3** Given the painted chart, when the text layer is extracted, then every number present is one of the series' supplied labels.
- **AC-4** (error path) Given a 100 001-point series, then painting fails with `report.chart_too_large`.

## Design

- **Algorithm:** axis range as exact decimals from the series; coordinate = round_half_even((v - min) * extent_um / (max - min)) using `kdec`-style integer rationals via the series' decimal strings; no float.
- **Errors:** `report.chart_too_large`.
- **Determinism:** D0.
- **Performance budget:** a 10 000-point chart in under 50 ms [unverified until measured].
- **Crate:** `fid-report-pdf` (module `chart`).

## Test fixtures

`fixtures/rpt/stats/trend_20parts` and a control-chart fixture from `RPT.STATS.SPC` tests; golden operator streams. CC-BY-4.0.

## Build prompt

> In crate `fid-report-pdf`, add module `chart`: painter and measurer for block kind `chart` drawing trend, histogram, control and vector-group series (contract `ChartSeries` from `RPT.STATS.TREND`) as vectors. Map series coordinates to integer µm with exact decimal ranges and one half-even rounding; no floats. Paint only labels supplied by the series. Control charts: centre line and limits in distinct styles, markers on flagged points. More than 100 000 points -> `report.chart_too_large`. Acceptance: AC-1..AC-4 in `spec/rpt/output/pdf/chart/NODE.md`. Constraints: CONSTITUTION C1, §4, §8.

## Open questions

- REQ-002 needs the `ChartSeries` contract to carry formatted labels and exact decimal ranges; raise at the `RPT.STATS.TREND` expansion.

## Children

_Leaf: none._
