---
id: RPT.STATS.DASH.OPEN
title: Dashboard open and tile evaluation
level: 4
parent: RPT.STATS.DASH
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.DASH-002, REQ-RPT.STATS.DASH-003, REQ-RPT.STATS.DASH-004, REQ-RPT.STATS.DASH-005]
covers: [PW.MSP.04]
depends_on: [RPT.STATS.DASH.DEF, RPT.STATS.DSET.MAT, RPT.STATS.DESC, RPT.STATS.TREND.BUILD, RPT.STATS.SPC.CHART, RPT.STATS.CAP.CMD, RPT.MODEL.FMT]
side_effect: R1
determinism: D0
cli: [fid stats open-dashboard]
mcp: [stats_open_dashboard]
journal: [stats.open_dashboard@1]
updated: 2026-11-17
---

# RPT.STATS.DASH.OPEN: Dashboard open and tile evaluation

## Purpose

The `stats.open_dashboard@1` command: it materialises the dashboard's data set once from journaled data, evaluates every tile by calling the statistics commands of its siblings, formats every shown number through the report formatters, and returns a `DashboardView`. A tile that cannot be computed shows its typed status while the other tiles are still shown. Nothing is cached in the dashboard.

## Scope

**In:**
- `stats.open_dashboard@1` (R1) and the `DashboardView` type
- Tile evaluation for the six tile kinds, including outcome counts
- Formatting through `RPT.MODEL.FMT`
- Per-tile status isolation

**Out:**
- The definition and save/list (`RPT.STATS.DASH.DEF`)
- The statistics themselves (DESC, TREND, SPC, CAP)
- Drawing tiles and charts (VIZ)

## Requirements

- **REQ-RPT.STATS.DASH.OPEN-001** When `stats.open_dashboard@1` runs, the system shall materialise the dashboard's data set once from the current journals and catalog and evaluate every tile against that one materialisation, and the view shall record its digest.
- **REQ-RPT.STATS.DASH.OPEN-002** Tiles shall be evaluated by the sibling functions only: `SUMMARY_TABLE` by `RPT.STATS.DESC`, `TREND` and `HISTOGRAM` by `RPT.STATS.TREND.BUILD`, `CONTROL_CHART` by `RPT.STATS.SPC.CHART`, `CAPABILITY_TABLE` by `RPT.STATS.CAP.CMD`, and `OUTCOME_COUNTS` by counting the journaled decision outcomes of the used entries per characteristic.
- **REQ-RPT.STATS.DASH.OPEN-003** Every number in a table tile, and every reference-line value in a chart tile, shall be returned as formatter output (`ValueText`) from `RPT.MODEL.FMT`, with the formatter ID and version.
- **REQ-RPT.STATS.DASH.OPEN-004** If a tile's evaluation returns an error or a typed status (for example `INSUFFICIENT_DATA`), then that tile shall carry the error code or status and no number, and every other tile shall still be evaluated and returned.
- **REQ-RPT.STATS.DASH.OPEN-005** The command shall be class R1 and shall not change the dashboard or any record.
- **REQ-RPT.STATS.DASH.OPEN-006** Opening the same dashboard against the same journal heads and catalog state shall give a bit-identical view on every supported target (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/three_part_run` and the dashboard from `fixtures/rpt/stats/dashboard_basic.json`, when `fid stats open-dashboard --dashboard <id> --json` runs, then six tiles are returned, table numbers are `ValueText` with formatter IDs, and the outcome-count tile shows one `NONCONFORMS` for C3.
- **AC-2** (tile isolation) Given the same dashboard on a data set with 3 parts, when opened, then the capability and control-chart tiles show `INSUFFICIENT_DATA` and the other four tiles are complete.
- **AC-3** (no cache) Given AC-1's dashboard, when a fourth run is added to the project and the dashboard is opened again, then the summary tile shows n = 4 and the dashboard definition digest is unchanged.
- **AC-4** (determinism) Given AC-1's inputs, when opened on x86-64, ARM64 and wasm32, then the view digests are identical.
- **AC-5** (error path) Given an unknown dashboard ID, when the command runs, then it fails with `stats.dashboard_not_found`.

## Design

- **Registry command:** `stats.open_dashboard@1(dashboard: DashboardId) -> DashboardView`, R1.
- **Data structures:** `DashboardView { dashboard, def_digest, dataset_digest, tiles: Vec<TileView> }`; `TileView { title, kind, body: TileBody::{Table(Vec<Vec<Cell>>), Charts(Vec<ChartSeries>), Status(StatusOrError)}, ref_line_text: Vec<ValueText> }`; `Cell::{Text(UntrustedText), Value(ValueText), Status}`.
- **Algorithm:** load the definition (DASH.DEF); materialise once (DSET.MAT); for each tile in order, call the sibling function with the shared `DataSet` and catch typed errors into the tile; format numbers with `RPT.MODEL.FMT`; outcome counts are integer counts per outcome in the fixed order `CONFORMS`, `NONCONFORMS`, `INDETERMINATE`, `NOT_DECIDABLE` (the last from exclusions).
- **Errors:** `stats.dashboard_not_found`, `stats.dataset_not_found` (whole dashboard); tile-level errors are carried in tiles.
- **Determinism:** D0, inheriting the siblings' D0 results and formatter determinism.
- **Performance budget:** a 12-tile dashboard over 1 000 records in under 500 ms [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/three_part_run`, `dashboard_basic.json` (shared with `RPT.STATS.DASH.DEF`).

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement `stats.open_dashboard@1(dashboard) -> DashboardView`, class R1; CLI `fid stats open-dashboard`, MCP `stats_open_dashboard`, `--json`.
>
> Load the `Dashboard` (module DASH.DEF), materialise its data set once (`DataSetReader`), and evaluate each tile with the sibling functions on that one `DataSet`: `SUMMARY_TABLE` -> `stats.compute_summary`, `TREND`/`HISTOGRAM` -> `stats.build_chart`, `CONTROL_CHART` -> `stats.build_control_chart`, `CAPABILITY_TABLE` -> `stats.compute_capability`, `OUTCOME_COUNTS` -> integer counts of journaled outcomes of used entries plus `NOT_DECIDABLE` exclusions. Format every table number and reference-line value with the report formatter service (`ValueText` with formatter ID and version, from `RPT.MODEL.FMT`). A tile error or typed status goes into that tile only; the others still evaluate. Never write to the dashboard or records. Record definition and data-set digests.
>
> **Tests:** AC-1 to AC-5 above with `fixtures/rpt/stats/three_part_run` and `dashboard_basic.json`, plus a cross-target view-digest test.
>
> **Constraints:** CONSTITUTION §8; C1 (all numbers from kernel results, formatted by the kernel's formatters); REQ-RPT-007 (no nonconforming value printed as equal to a limit, which the formatter guarantees). Spec: `spec/rpt/stats/dash/open/NODE.md`.

## Open questions

- Chart axis tick labels are drawn by VIZ and are not values of record; only reference-line values are formatted here. Confirm with the VIZ owners.

## Children

_Leaf: none._
