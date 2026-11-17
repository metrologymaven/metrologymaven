---
id: RPT.STATS.SPC
title: Control charts, control limits and run rules
level: 3
parent: RPT.STATS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-002, REQ-RPT.STATS-004, REQ-RPT.STATS-005, REQ-RPT.STATS-006, REQ-RPT.STATS-014]
covers: [GOM.TRD.05, PW.MSP.02]
depends_on: [RPT.STATS.DSET, RPT.STATS.TREND, KRN.NUM]
cli: [fid stats *-limits, fid stats build-control-chart]
mcp: [stats_*_limits, stats_build_control_chart]
journal: [stats.*_limits@1, stats.build_control_chart@1]
updated: 2026-11-17
---

# RPT.STATS.SPC: Control charts, control limits and run rules

## Purpose

Statistical process control per characteristic: X̄-R, X̄-s and individuals/moving-range (I-MR) charts, control-limit calculation from a data set, freezing limits so later points are judged against them, and named, versioned run-rule sets that flag out-of-control patterns. Limits and flags are computed in the kernel; series are handed out as `ChartSeries`.

## Scope

**In:**
- Subgrouping of a data set (by run or fixed subgroup size)
- Limit calculation for X̄-R, X̄-s and I-MR with a versioned constants table cited by source and table number only
- Freezing limits with the data-set digest they came from; plotting later points against frozen limits
- Named, versioned run-rule sets and violation flags
- Control-chart `ChartSeries`

**Out** (and which node owns it instead):
- Capability indices (`RPT.STATS.CAP`)
- The `ChartSeries` contract (`RPT.STATS.TREND`)
- Export to SPC systems (`RPT.OUTPUT.SPC`)

## Requirements

- **REQ-RPT.STATS.SPC-001** The system shall compute centre lines and control limits for X̄-R, X̄-s and I-MR charts in the kernel, with fixed-order reductions (KNP-1).
- **REQ-RPT.STATS.SPC-002** Control-chart constants shall come from a versioned table whose every value cites its source and table number, and the limits shall record the table ID and version [unverified which source is default].
- **REQ-RPT.STATS.SPC-003** When the user freezes control limits, the system shall store them with the data-set digest and algorithm ID they came from as a journaled W2 command.
- **REQ-RPT.STATS.SPC-004** While limits are frozen for a characteristic, new points shall be judged against the frozen limits and not against recomputed ones.
- **REQ-RPT.STATS.SPC-005** The system shall flag every point that breaks a rule of the configured run-rule set, and each flag shall name the rule set ID, version and rule.
- **REQ-RPT.STATS.SPC-006** If a subgroup has fewer values than the chart type requires, then the subgroup shall be excluded with reason `INCOMPLETE_SUBGROUP` and listed.
- **REQ-RPT.STATS.SPC-007** Limits and flags shall be bit-identical for the same data set and algorithm version on one platform or better (D1 or better).

## Interfaces

- **Offers:** `stats.compute_limits@1` (R1), `stats.freeze_limits@1` (W2); types `ControlLimits`, `RunRuleSet`, `RuleViolation`; control-chart `ChartSeries`.
- **Consumes:** data sets (`RPT.STATS.DSET`), chart contract (`RPT.STATS.TREND`), numerics (`KRN.NUM`).

## Decomposition hints

SUB (subgrouping), LIM (limits and constants table), FREEZE (frozen limits), RULES (run-rule sets).

## Open questions

- Which reference (ISO 7870-2, AIAG SPC manual, other) and edition defines the default constants and run rules needs a domain expert [unverified]. The split does not wait for it: `RPT.STATS.SPC.CONST` ships a versioned table with every row `UNVERIFIED`, `RPT.STATS.SPC.RULES` ships the plain `beyond_limits@1` default and the published sets as `UNVERIFIED` data, and results carry those flags. Raise a decision before release if no maintainer with SPC expertise can choose.
- Control-chart series use their own command, `stats.build_control_chart@1` (`RPT.STATS.SPC.CHART`), and unfreezing is `stats.unfreeze_limits@1`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.STATS.SPC.SUB | Subgrouping of a data set (leaf) | 006 007 | |
| RPT.STATS.SPC.CONST | Versioned control-chart constants table (leaf; values UNVERIFIED) | 002 007 | |
| RPT.STATS.SPC.LIM | Control-limit calculation (leaf) | 001 002 007 | GOM.TRD.05 |
| RPT.STATS.SPC.FREEZE | Frozen control limits (leaf) | 003 004 | GOM.TRD.05 |
| RPT.STATS.SPC.RULES | Run-rule sets and violation detection (leaf; published sets UNVERIFIED) | 005 007 | |
| RPT.STATS.SPC.CHART | Control-chart series builder (leaf) | 004 005 007 | PW.MSP.02 |
