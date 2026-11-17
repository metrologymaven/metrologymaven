---
id: UX.REPORT.CHART
title: Report chart styles and colour-map snapshot legend
level: 3
parent: UX.REPORT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.REPORT-003, REQ-UX.REPORT-004]
covers: []
depends_on: [UX.REPORT.STYLE, UX.TOKENS.DATAPAL, UX.COMP.DATA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.REPORT.CHART: Report chart styles and colour-map snapshot legend

## Purpose

How charts look in reports: control chart, run chart, histogram, capability chart, and the deviation colour-map snapshot with its legend. Each style defines axes, labels, limit-line and specification-line styles and point markers, and states that every plotted value and limit comes from RPT, never from the chart. Colour-map snapshots use the same palette IDs as the viewer.

## Scope

**In:** `report/charts/<kind>.json` (`chart-style@1`) and a short `report/charts/README.md`; `fid-design chart-style-check`.
**Out:** computing statistics, limits and capability indices (RPT.STATS); rendering (RPT.OUTPUT); the viewer legend (VIZ.OVERLAY.LEGEND); palette values (`UX.TOKENS.DATAPAL`).

## Requirements

- **REQ-UX.REPORT.CHART-001** The styles shall cover `control`, `run`, `histogram`, `capability` and `cmap-snapshot`, each defining axis, tick, label, grid, data line, point marker, out-of-limit marker, control-limit line, specification-limit line and nominal line styles as token references.
- **REQ-UX.REPORT.CHART-002** Limit, specification and nominal lines shall differ by dash pattern and label text, not only colour, and out-of-limit points shall use a distinct marker shape.
- **REQ-UX.REPORT.CHART-003** Each style file shall carry `values_from = "rpt"` and the README shall state that charts draw only values and limits supplied by RPT and never compute or round them.
- **REQ-UX.REPORT.CHART-004** The `cmap-snapshot` style shall reference palettes only by `pal.*` ID from the catalogue, and its legend shall show the palette ID and version, the unit, bin edges as supplied, and under- and over-range swatches.
- **REQ-UX.REPORT.CHART-005** If a style uses a literal value, an unknown palette ID or lacks a required element, then `fid-design chart-style-check` shall fail and name it.

## Acceptance criteria

- **AC-1** Given the shipped styles, when `fid-design chart-style-check --root design --json` runs, then it exits 0 for all five kinds.
- **AC-2** (error path) Given fixture `fixtures/ux/report/chart/unknown-pal/` (`pal.cont.jet@1`), when the check runs, then it exits 1 with `CHART-PALETTE-UNKNOWN`.
- **AC-3** (error path) Given `same-dash/` (control and specification limits with the same dash and no label), when the check runs, then it exits 1 with `CHART-LINE-CUE`.
- **AC-4** (error path) Given `no-values-from/`, when the check runs, then it exits 1 with `CHART-VALUES-FROM`.

## Design

- **Command:** `fid-design chart-style-check --root <dir> [--json]`. Tooling, nearest class R0.
- **Errors:** `CHART-SCHEMA`, `CHART-LITERAL`, `CHART-ELEMENT-MISSING`, `CHART-LINE-CUE`, `CHART-PALETTE-UNKNOWN`, `CHART-VALUES-FROM`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `chart_style`.

## Test fixtures

`fixtures/ux/report/chart/`: `unknown-pal/`, `same-dash/`, `no-values-from/`; synthetic, CC-BY-4.0.

## Build prompt

> Author `report/charts/{control,run,histogram,capability,cmap-snapshot}.json` (`chart-style@1`, token references only, `values_from = "rpt"`) and `report/charts/README.md` (charts draw only RPT-supplied values and limits). Limit, specification and nominal lines differ by dash and label; out-of-limit points use a distinct marker shape; the colour-map snapshot legend shows palette ID and version, unit, supplied bin edges, under and over swatches, and uses only catalogue `pal.*` IDs. In the Rust tooling crate `fid-design`, add `fid-design chart-style-check --root <dir> [--json]` with errors `CHART-SCHEMA|CHART-LITERAL|CHART-ELEMENT-MISSING|CHART-LINE-CUE|CHART-PALETTE-UNKNOWN|CHART-VALUES-FROM`. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.REPORT.CHART.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- RPT.STATS chart output should follow these styles (`uses_artifacts: [UX.REPORT.CHART]` on the RPT side, not editable from here).

## Children

_Leaf: none._
