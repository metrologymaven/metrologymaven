---
id: RPT.STATS.DASH
title: Dashboards
level: 3
parent: RPT.STATS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-012, REQ-RPT.STATS-013]
covers: [PW.MSP.04]
depends_on: [RPT.STATS.DSET, RPT.STATS.DESC, RPT.STATS.TREND, RPT.STATS.SPC, RPT.STATS.CAP, RPT.MODEL.FMT]
cli: [fid stats save-dashboard, fid stats open-dashboard, fid stats list-dashboard]
mcp: [stats_save_dashboard, stats_open_dashboard, stats_list_dashboard]
journal: [stats.save_dashboard@1, stats.open_dashboard@1]
updated: 2026-11-17
---

# RPT.STATS.DASH: Dashboards

## Purpose

Dashboards are saved, read-only summary views over a catalog query: which data set, which statistics and which charts, arranged in tiles. A dashboard holds no data of its own; opening it recomputes everything from journaled data, and every number in it goes through the kernel's formatters.

## Scope

**In:**
- Dashboard definition: data-set definition, tiles (summary table, trend, control chart, capability table, outcome counts), refresh on open
- Save, open and list commands
- Formatting of every shown number through `RPT.MODEL.FMT`

**Out** (and which node owns it instead):
- Statistics computations (DESC, TREND, SPC, CAP)
- Dashboard UI panel and chart drawing (VIZ)
- Results database (`RPT.CATALOG`)

## Requirements

- **REQ-RPT.STATS.DASH-001** A dashboard shall be stored as a definition (data-set definition plus tiles) and shall store no computed values.
- **REQ-RPT.STATS.DASH-002** When a dashboard is opened, the system shall recompute every tile only from journaled data reachable through its data set.
- **REQ-RPT.STATS.DASH-003** Every number shown in a dashboard shall be produced by a kernel formatter through `RPT.MODEL.FMT`.
- **REQ-RPT.STATS.DASH-004** Saving a dashboard shall be a journaled W2 command; opening and listing shall be R1 and R0.
- **REQ-RPT.STATS.DASH-005** If a tile cannot be computed (for example insufficient data), then the tile shall show the typed status and the other tiles shall still be shown.

## Interfaces

- **Offers:** `stats.save_dashboard@1` (W2), `stats.open_dashboard@1` (R1), `stats.list_dashboard@1` (R0); type `Dashboard`.
- **Consumes:** data sets and statistics (siblings), formatter layer (`RPT.MODEL.FMT`), VIZ panels.

## Decomposition hints

DEF (definition and commands), TILE (tile kinds and evaluation).

## Open questions

- Tiles call DESC, SPC and CAP as well as TREND, so those siblings were added to `depends_on`.
- Shared dashboards across users on a shared catalog depend on `RPT.CATALOG.SHARE`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.STATS.DASH.DEF | Dashboard definition, save and list (leaf) | 001 004 | PW.MSP.04 |
| RPT.STATS.DASH.OPEN | Dashboard open and tile evaluation (leaf) | 002 003 004 005 | PW.MSP.04 |
