---
id: RPT
title: Reporting, statistics and results data
level: 1
parent: FID
kind: domain
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-001, REQ-FID-012]
covers: [GOM.RPT.*, GOM.TRD.*, PW.MSP.*, PW.RPT.*, PCD.RPT.*, SA.RPT.*]
depends_on: [CORE, GDT, UNC, VIZ, IO]
cli: [fid report *, fid stats *]
mcp: [report_*, stats_*]
journal: [report.*, stats.*]
updated: 2026-11-16
---

# RPT: Reporting, statistics and results data

## Purpose

Turns results into deliverables:
- report pages and templates with restorable 3D views, tables and charts;
- industry forms (AS9102 FAI, PPAP dimensional);
- multi-part statistics (trend, SPC, Cp/Cpk);
- a local results catalog across projects;
- ballooned drawings with manual entry of readings.

## Scope

**In:**
- Report model and templates
- Report designer
- Restorable 3D views (from VIZ)
- Tables and charts
- PDF and spreadsheet output
- AS9102 forms 1–3 and PPAP dimensional
- Drawing ballooning and manual entry (DIRECT_READING basis)
- Multi-part and stage statistics, trend charts, SPC control charts and limits, Cp/Cpk
- Part-to-part comparison
- Results catalog (SQLite index across projects) and historical import
- Dashboards

**Out:**
- File format writers (IO)
- Rendering (VIZ)
- Signing reports (TRUST)

## Requirements

- **REQ-RPT-001** Reports shall be generated from templates that bind to ResultRecords and saved views, and regenerating a report from the same journal shall give identical content.
- **REQ-RPT-002** Only journaled ResultRecords shall appear in reports. Transcription records shall be labelled as evaluated by their source software.
- **REQ-RPT-003** The system shall produce AS9102 first-article forms 1–3 and a PPAP dimensional report from fixed templates.
- **REQ-RPT-004** The system shall compute multi-part statistics (mean, range, standard deviation), trend charts, SPC control limits and Cp/Cpk per characteristic across stages and runs.
- **REQ-RPT-005** The system shall keep a local results catalog across projects and import historical results from CSV, Q-DAS and QIF Statistics.
- **REQ-RPT-006** Users shall balloon a drawing and enter manual readings, which are recorded with basis DIRECT_READING and the gauge they cite.
- **REQ-RPT-007** Report numbers shall be formatted by the kernel's versioned formatters, and a nonconforming value shall never be printed as equal to a limit.

## Interfaces

- **Offers:** `report.*` and `stats.*` commands; the report template format; the results catalog.
- **Consumes:** GDT and UNC results, VIZ saved views, IO writers, CORE.

## Decomposition hints

MODEL (report model and templates), DESIGN (designer), FORMS (AS9102, PPAP), BALLOON (ballooning and manual entry), STATS (statistics, SPC, capability), CATALOG (results catalog, historical import), OUT (PDF and spreadsheet output).

## Split rationale

The split follows the **deliverable pipeline**: a report model that binds journaled results (MODEL), the ways people shape it (DESIGN) or must not shape it (FORMS), the one place results are created inside RPT (BALLOON, manual readings), the cross-part data path (CATALOG feeding STATS), and the single place bytes leave the report (OUTPUT). Each child has its own command family under `report.*` or `stats.*`, so registry ownership is unambiguous.

- MODEL is separate from DESIGN because the model and generation are needed in P1a for transcription reports, while the designer is P1b (CONSTITUTION §7).
- STATS depends on CATALOG, not the reverse: the catalog only stores and queries; every computed value lives in STATS.
- Seed PW.MSP.04 ("results database and dashboards") is covered twice on purpose and split by responsibility: the database part in CATALOG and the dashboards in STATS.
- Chart series are computed in STATS and laid out by MODEL's chart blocks; there is no separate charts child.

Alternatives considered:
- **By output kind** (PDF, spreadsheet, forms, SPC export). Rejected: it duplicates binding and formatting in every child.
- **Reporting vs. statistics only** (two children). Rejected: each would be XXL, and FAI forms and ballooning have distinct users and phases.
- **A separate DASH child.** Rejected for now: dashboards are saved statistics definitions with no logic of their own.

## Open questions

- The kernel's versioned number formatters (REQ-RPT-007) are not yet named by KRN or CORE. Raised in RPT.MODEL.
- Copyright exposure of fixed AS9102 and PPAP form layouts. Raised in RPT.FORMS; may need a decision.
- Central server-based results management (PW.MSP.03) beyond a shared-folder catalog. Raised in RPT.CATALOG.
- Default SPC constants, run rules and capability references need a domain expert. Raised in RPT.STATS.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.MODEL | Report model, templates and content blocks | 001 002 007 | GOM.RPT.01-05/07, PCD.RPT.01/03, PW.RPT.02/03, SA.RPT.01/02/04 |
| RPT.DESIGN | Report designer | 001 | PCD.RPT.02, PW.RPT.01 |
| RPT.FORMS | Industry forms: AS9102 FAI and PPAP dimensional | 003 | none |
| RPT.BALLOON | Drawing ballooning and manual readings | 002 006 | none |
| RPT.STATS | Multi-part statistics, SPC, capability and dashboards | 004 007 | GOM.TRD.*, PW.MSP.01/02/04/05 |
| RPT.CATALOG | Results catalog and historical import | 005 | PW.MSP.03/04, PCD.RPT.07 |
| RPT.OUTPUT | Report and statistics output | 001 007 | GOM.RPT.06, PCD.RPT.04-06, PW.RPT.04, SA.RPT.03 |
