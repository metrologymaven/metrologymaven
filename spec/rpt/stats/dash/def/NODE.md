---
id: RPT.STATS.DASH.DEF
title: Dashboard definition, save and list
level: 4
parent: RPT.STATS.DASH
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.DASH-001, REQ-RPT.STATS.DASH-004]
covers: [PW.MSP.04]
depends_on: [RPT.STATS.DSET.DEF, CORE.PRJ.CONT, CORE.JRN.CANON]
side_effect: [W2, R0]
determinism: D0
cli: [fid stats save-dashboard, fid stats list-dashboard]
mcp: [stats_save_dashboard, stats_list_dashboard]
journal: [stats.save_dashboard@1]
updated: 2026-11-17
---

# RPT.STATS.DASH.DEF: Dashboard definition, save and list

## Purpose

Defines the `Dashboard` object (a data-set reference plus an ordered list of tiles, each a statistic or chart with its parameters) and the commands to save and list dashboards in the project. A dashboard stores no computed values, so it can never show stale numbers; `RPT.STATS.DASH.OPEN` recomputes every tile when the dashboard is opened.

## Scope

**In:**
- `Dashboard` and `Tile` types, tile kinds and their parameters, validation
- `stats.save_dashboard@1` (W2) and `stats.list_dashboard@1` (R0)

**Out:**
- Evaluating tiles (`RPT.STATS.DASH.OPEN`)
- Data-set definitions (`RPT.STATS.DSET.DEF`)
- Drawing (VIZ)

## Requirements

- **REQ-RPT.STATS.DASH.DEF-001** A `Dashboard` shall hold an ID, a name, one data-set reference (a saved data-set ID) and an ordered list of 1 to 64 tiles, each with a kind (`SUMMARY_TABLE`, `TREND`, `HISTOGRAM`, `CONTROL_CHART`, `CAPABILITY_TABLE` or `OUTCOME_COUNTS`), an optional characteristic filter and the kind's parameters.
- **REQ-RPT.STATS.DASH.DEF-002** A dashboard shall store no result values, statistics, chart points or formatted text, and a save request that carries any shall fail with `stats.dashboard_invalid`.
- **REQ-RPT.STATS.DASH.DEF-003** When `stats.save_dashboard@1` runs with a valid dashboard, the system shall store it in the project as a journaled W2 command, assigning a UUIDv7 ID to a new dashboard and keeping the ID of an existing one.
- **REQ-RPT.STATS.DASH.DEF-004** If the data-set reference does not exist, a tile parameter is out of range, or the tile count is outside 1 to 64, then the save shall fail with `stats.dashboard_invalid` naming the field, and nothing shall be journaled.
- **REQ-RPT.STATS.DASH.DEF-005** When `stats.list_dashboard@1` runs, the system shall return every dashboard's ID, name, data-set ID and tile count, ordered by name then ID, as an R0 read.
- **REQ-RPT.STATS.DASH.DEF-006** Dashboard and tile titles shall be typed as untrusted text (C6), at most 200 characters each.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/three_part_run` with a saved data set, when `fid stats save-dashboard --file fixtures/rpt/stats/dashboard_basic.json --json` runs (one tile of each kind), then a dashboard ID is returned, one event is journaled, and `fid stats list-dashboard --json` lists it with 6 tiles.
- **AC-2** (no values) Given `fixtures/rpt/stats/dashboard_with_values.json` (a tile carrying a cached mean), when saved, then it fails with `stats.dashboard_invalid` naming that tile.
- **AC-3** (error path) Given a dashboard referencing a missing data-set ID, when saved, then it fails with `stats.dashboard_invalid` naming `dataset` and journals nothing.
- **AC-4** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `stats.save_dashboard@1(dashboard: Dashboard) -> DashboardRef`, W2 (agents write to an agent branch); `stats.list_dashboard@1() -> Vec<DashboardSummary>`, R0. The CLI reads the dashboard JSON from an approved import root (CONSTITUTION §3).
- **Data structures:** `Dashboard { id, schema: 1, name, dataset: DataSetId, tiles: Vec<Tile> }`; `Tile { title, kind: TileKind, chars: Option<Vec<CharId>>, params: TileParams }`; `TileParams` per kind: trend order key; histogram binning rule; control chart type, subgrouping and rule-set ID; capability subgrouping, estimator and minimum n. Serde with `deny_unknown_fields` enforces REQ-002.
- **Storage:** project element type `stats.dashboard`, canonicalised with `fid-canon`.
- **Errors:** `stats.dashboard_invalid{field}`, `stats.dashboard_not_found`.
- **Determinism:** D0 (no arithmetic).
- **Performance budget:** save and list under 50 ms for 500 dashboards [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/dashboard_basic.json` and `dashboard_with_values.json`: hand-written, CC-BY-4.0; `three_part_run` (shared).

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement dashboard definitions: `Dashboard { id, schema: 1, name, dataset: DataSetId, tiles: Vec<Tile> }` with 1..=64 tiles of kinds `SUMMARY_TABLE`, `TREND`, `HISTOGRAM`, `CONTROL_CHART`, `CAPABILITY_TABLE`, `OUTCOME_COUNTS`, each with optional characteristic filter and kind-specific parameters (order key; binning rule; chart type, subgrouping, rule set; subgrouping, estimator, min n). Titles are untrusted text, at most 200 characters. Use `deny_unknown_fields` so no computed values can be stored.
>
> **Commands:** `stats.save_dashboard@1(dashboard) -> DashboardRef` (W2, journaled; UUIDv7 for new; project element type `stats.dashboard`), `stats.list_dashboard@1()` (R0, ordered by name then ID). CLI `fid stats save-dashboard`, `fid stats list-dashboard`; MCP `stats_save_dashboard`, `stats_list_dashboard`; `--json`. Errors `stats.dashboard_invalid{field}` (missing data set, bad parameter, tile count, stored values), nothing journaled on error.
>
> **Tests:** AC-1 to AC-4 above with the fixtures named there.
>
> **Constraints:** CONSTITUTION §8; C6; raw file paths only for human principals, approved roots otherwise. Spec: `spec/rpt/stats/dash/def/NODE.md`.

## Open questions

- Dashboards shared across users on a shared catalog depend on `RPT.CATALOG.SHARE` and are not covered here.

## Children

_Leaf: none._
