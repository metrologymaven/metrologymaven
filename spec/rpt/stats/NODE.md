---
id: RPT.STATS
title: Multi-part statistics, SPC, capability and dashboards
level: 2
parent: RPT
kind: subsystem
status: split
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-RPT-004, REQ-RPT-007]
covers: [GOM.TRD.*, PW.MSP.01, PW.MSP.02, PW.MSP.04, PW.MSP.05]
depends_on: [RPT.CATALOG, RPT.MODEL, KRN]
cli: [fid stats compute-*, fid stats *-limits, fid stats *-chart, fid stats compare-*, fid stats *-dashboard, fid stats *-dataset]
mcp: [stats_compute_*, stats_*_limits, stats_*_chart, stats_compare_*, stats_*_dashboard, stats_*_dataset]
journal: [stats.compute_*, stats.*_limits, stats.*_chart, stats.compare_*, stats.*_dashboard, stats.*_dataset]
updated: 2026-11-16
---

# RPT.STATS: Multi-part statistics, SPC, capability and dashboards

## Purpose

Statistics across parts, stages and runs, per characteristic: descriptive statistics, trend series, SPC control charts with frozen control limits, capability indices, part-to-part comparison, stepping through pieces of a multi-piece set, and dashboards that summarise these. All values of record are computed by the deterministic kernel (C1). This node produces data series and indices; `RPT.MODEL` lays them out in reports.

## Scope

**In:**
- Data sets: an ordered selection of ResultRecords per characteristic, from the current project or from the catalog
- Descriptive statistics: n, mean, min, max, range, standard deviation
- Trend series across stages, runs and parts
- Control charts: X̄-R, X̄-s, individuals and moving range (I-MR); control-limit calculation and freezing; run-rule violation flags
- Capability and performance indices: Cp, Cpk, Pp, Ppk, with one- and two-sided tolerances
- Part-to-part comparison (difference of results between two parts or a part and a reference part)
- Stepping through pieces in a multi-piece set and showing one piece's results at a time
- Dashboards: saved, read-only summary views of statistics over a catalog query
- Chart series contract for report chart blocks

**Out** (and which node owns it instead):
- Storing and querying results across projects, historical import (`RPT.CATALOG`)
- Chart and table layout in reports (`RPT.MODEL`)
- Export of statistics to Q-DAS and SPC databases (`RPT.OUTPUT`)
- Surface deviation part-to-part comparison of meshes (`SCAN`)
- Rendering charts on screen (`VIZ`)

## Requirements

- **REQ-RPT.STATS-001** The system shall compute n, mean, minimum, maximum, range and sample standard deviation per characteristic over a data set, using the kernel with fixed-order reductions (KNP-1).
- **REQ-RPT.STATS-002** Statistics shall be reproducible: the same data set and algorithm version shall give bit-identical results on one platform (determinism class D1 or better).
- **REQ-RPT.STATS-003** The system shall produce trend series ordered by stage, run time or part sequence, as chosen by the user.
- **REQ-RPT.STATS-004** The system shall compute control limits for X̄-R, X̄-s and I-MR charts using constants cited by source and table number only [unverified].
- **REQ-RPT.STATS-005** When the user freezes control limits, the system shall store them with the data-set digest they were computed from, and later points shall be plotted against the frozen limits.
- **REQ-RPT.STATS-006** The system shall flag points that break the configured run rules, and the rule set shall be named and versioned.
- **REQ-RPT.STATS-007** The system shall compute Cp, Cpk, Pp and Ppk per characteristic, and shall compute only the one-sided index when the tolerance is one-sided.
- **REQ-RPT.STATS-008** If a data set has fewer values than the configured minimum for an index (default 25 [unverified]), then the system shall mark the index `INSUFFICIENT_DATA` and shall not print a number.
- **REQ-RPT.STATS-009** Every statistic shall record which ResultRecords it used and which it excluded, with the reason for each exclusion (for example `NOT_DECIDABLE` or a user exclusion).
- **REQ-RPT.STATS-010** The system shall compare two parts characteristic by characteristic and report the difference with its unit.
- **REQ-RPT.STATS-011** When the user steps to another piece of a multi-piece set, the system shall show that piece's results without recomputing any value of record.
- **REQ-RPT.STATS-012** A dashboard shall be a saved definition (catalog query plus statistics and charts), and opening it shall recompute only from journaled data.
- **REQ-RPT.STATS-013** Statistics values shown in reports or dashboards shall be formatted by the kernel's versioned formatters.
- **REQ-RPT.STATS-014** The system shall cite every statistical method by standard designator, edition and clause, or by published reference, and shall not embed standards text.

## Interfaces

- **Offers:** `stats.compute_summary@1`, `stats.compute_capability@1`, `stats.compute_limits@1`, `stats.freeze_limits@1`, `stats.build_chart@1`, `stats.compare_parts@1`, `stats.save_dashboard@1`; types `DataSet`, `StatSummary`, `ControlLimits`, `CapabilityIndex`, `ChartSeries`.
- **Consumes:** ResultRecord queries (`RPT.CATALOG`), kernel numerics (`KRN`, `fid-num`), formatters, chart blocks (`RPT.MODEL`).
- **Side effects:** compute is R1; freezing limits and saving dashboards is W2.

## Decomposition hints

DSET (data-set selection and exclusions), DESC (descriptive statistics), TREND (trend series), SPC (control charts, limits, run rules), CAP (capability indices), CMP (part-to-part comparison and piece stepping), DASH (dashboards).

## Split rationale

The split follows the **statistical method**, with one shared input: every statistic reads a digested, ordered `DataSet` (DSET), which is what makes "which results were used" answerable and results reproducible. Methods then split by what they compute: descriptive statistics (DESC), trend and histogram series plus the shared `ChartSeries` contract (TREND), control charts and run rules (SPC), capability indices (CAP), and part comparison with piece stepping (CMP). Dashboards (DASH) are saved definitions over the others and compute nothing themselves.

- DESC is finished as a leaf: the method is standard, the NIST StRD univariate datasets give certified answers, and its command is R1.
- The chart-series contract sits in TREND, the simplest producer, so SPC and LVM depend on it rather than on a separate contract child.
- REQ-RPT.STATS-002 (reproducibility) is satisfied by every computing child; REQ-RPT.STATS-014 (citations) by the three method children that cite standards.
- `*-dataset` was added to this node's CLI, MCP and journal patterns for DSET.

Alternatives considered:
- **By chart type** (trend, control, histogram). Rejected: limits and indices are not charts, and series building would be duplicated.
- **Merge CAP into SPC.** Rejected: capability needs no control limits for Pp/Ppk and ships earlier; the expert question on constants affects SPC more than CAP.

## Open questions

- Which reference (ISO 22514 parts, ISO 7870-2, AIAG SPC manual) and which edition defines the default constants and run rules? Needs a domain expert; may become a decision before the SPC leaf.
- Non-normal capability methods (percentile, Box-Cox) are not required yet.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.STATS.DSET | Data sets, ordering and exclusions | 002 009 | PW.MSP.01 |
| RPT.STATS.DESC | Descriptive statistics per characteristic (leaf) | 001 002 014 | GOM.TRD.01, PW.MSP.01 |
| RPT.STATS.TREND | Trend series and the chart-series contract | 003 | GOM.TRD.02 |
| RPT.STATS.SPC | Control charts, control limits and run rules | 002 004 005 006 014 | GOM.TRD.05, PW.MSP.02 |
| RPT.STATS.CAP | Capability and performance indices | 002 007 008 014 | GOM.TRD.03 |
| RPT.STATS.CMP | Part-to-part comparison and piece stepping | 010 011 | GOM.TRD.04, PW.MSP.05 |
| RPT.STATS.DASH | Dashboards | 012 013 | PW.MSP.04 |
