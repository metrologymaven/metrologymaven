---
id: RPT.STATS.SPC.CHART
title: Control-chart series builder
level: 4
parent: RPT.STATS.SPC
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.SPC-004, REQ-RPT.STATS.SPC-005, REQ-RPT.STATS.SPC-007]
covers: [PW.MSP.02]
depends_on: [RPT.STATS.SPC.LIM, RPT.STATS.SPC.FREEZE, RPT.STATS.SPC.RULES, RPT.STATS.SPC.SUB, RPT.STATS.TREND.CONTRACT, RPT.STATS.DSET.MAT]
side_effect: R1
determinism: D0
cli: [fid stats build-control-chart]
mcp: [stats_build_control_chart]
journal: [stats.build_control_chart@1]
updated: 2026-11-17
---

# RPT.STATS.SPC.CHART: Control-chart series builder

## Purpose

The `stats.build_control_chart@1` command: for one characteristic it plots the subgroup statistics of a data set against the active limits (frozen if a frozen record exists, otherwise computed from the same data), evaluates the run-rule set, and returns two `ChartSeries` (location and dispersion) with reference lines and violation markers. This is where "later points are judged against frozen limits" happens.

## Scope

**In:**
- `stats.build_control_chart@1` for `XBAR_R`, `XBAR_S` and `IMR`
- Choice between frozen and computed limits
- Rule evaluation on the plotted points and `RULE_VIOLATION` markers
- Two `ChartSeries` per call

**Out:**
- Limit formulas (`RPT.STATS.SPC.LIM`), frozen-limit storage (`RPT.STATS.SPC.FREEZE`), rule semantics (`RPT.STATS.SPC.RULES`)
- The `ChartSeries` type (`RPT.STATS.TREND.CONTRACT`)

## Requirements

- **REQ-RPT.STATS.SPC.CHART-001** When `stats.build_control_chart@1` runs, the system shall return a location series (`CONTROL_XBAR` or `CONTROL_I`) and a dispersion series (`CONTROL_R`, `CONTROL_S` or `CONTROL_MR`) with one point per used subgroup (or per moving range) in data-set order.
- **REQ-RPT.STATS.SPC.CHART-002** While frozen limits are active for the characteristic and chart type and the request does not set `ignore_frozen`, the system shall draw `CENTRE`, `LCL` and `UCL` from the frozen record, shall not recompute limits, and shall put the frozen record ID in the series provenance.
- **REQ-RPT.STATS.SPC.CHART-003** If frozen limits are active but their subgroup size differs from the requested subgrouping, then the command shall fail with `stats.frozen_subgroup_mismatch`.
- **REQ-RPT.STATS.SPC.CHART-004** When no frozen limits are active, the system shall compute limits with `RPT.STATS.SPC.LIM` from the same data set; if they are `INSUFFICIENT_DATA`, the series shall carry points without limit lines and with status `INSUFFICIENT_DATA`, and no rules shall be evaluated.
- **REQ-RPT.STATS.SPC.CHART-005** The system shall evaluate the requested rule set (default `beyond_limits@1`) on the location series and `beyond_limits@1` on the dispersion series, and add one `RULE_VIOLATION` marker per violation naming the set ID, version and rule ID.
- **REQ-RPT.STATS.SPC.CHART-006** Each series shall carry the data-set digest, algorithm ID `stats.spc.chart@1.0.0`, kernel version and the constants table reference, and a series built on `UNVERIFIED` constants or rule sets shall say so in its provenance.
- **REQ-RPT.STATS.SPC.CHART-007** The result shall be bit-identical on every supported target for the same inputs (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/spc_xbar_r_25x5`, when `fid stats build-control-chart --dataset <id> --char C1 --chart xbar-r --subgroup fixed:5 --json` runs, then two series of 25 points are returned with lines equal to `compute-limits` output.
- **AC-2** (frozen) Given limits frozen from the first 25 subgroups and a data set extended with 5 more subgroups, one beyond the frozen UCL, when the command runs, then the lines equal the frozen values bit for bit, the provenance names the frozen record, and that subgroup has a `RULE_VIOLATION` marker for `beyond_limits@1`.
- **AC-3** (mismatch) Given frozen limits for n = 5, when the command runs with `fixed:4`, then it fails with `stats.frozen_subgroup_mismatch`.
- **AC-4** (insufficient) Given 10 subgroups and no frozen limits, when the command runs, then points are returned without limit lines, status `INSUFFICIENT_DATA` and no markers.
- **AC-5** (replay) Given a journal that put AC-2's series in a report, when replayed in exact mode, then the outcome is `IDENTICAL`; when replayed after `stats.unfreeze_limits@1` is inserted, then it is `DIVERGENT`.

## Design

- **Registry command:** `stats.build_control_chart@1(dataset: DataSetId, char: CharId, chart: ChartType, subgrouping: SubgroupMode, rule_set: Option<RuleSetId>, ignore_frozen: bool = false) -> ControlChart { location: ChartSeries, dispersion: ChartSeries, violations: Vec<RuleViolation> }`, R1.
- **Algorithm:** materialise (DSET.MAT); `subgroup` (SPC.SUB); `subgroup_stats` (SPC.LIM); `active_limits` (SPC.FREEZE) or `limits` (SPC.LIM); `evaluate_rules` (SPC.RULES); assemble series with `fid-chart-series` types and validate.
- **Errors:** `stats.frozen_subgroup_mismatch`, `stats.char_not_in_dataset`, `stats.ruleset_invalid`, `stats.subgroup_size_unsupported`.
- **Determinism:** D0.
- **Performance budget:** 10 000 subgroups in under 30 ms after materialisation [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/spc_xbar_r_25x5` and an extended variant `spc_xbar_r_30x5` (5 more subgroups, one high), CC-BY-4.0.

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement `stats.build_control_chart@1(dataset, char, chart, subgrouping, rule_set?, ignore_frozen = false) -> ControlChart`, class R1; CLI `fid stats build-control-chart`, MCP `stats_build_control_chart`, `--json`.
>
> Steps: materialise the data set (`DataSetReader`); subgroup (`fid_stats::subgroup`); compute subgroup statistics (`fid_stats::subgroup_stats`); if `active_limits(char, chart)` returns a frozen record and `ignore_frozen` is false, use its lines (fail with `stats.frozen_subgroup_mismatch` if its n differs) and put its ID in provenance; else compute with `fid_stats::limits` and, if `INSUFFICIENT_DATA`, return points without lines and no rule evaluation. Evaluate the rule set (default `beyond_limits@1`) on the location points and `beyond_limits@1` on the dispersion points; add `RULE_VIOLATION` markers. Build two `ChartSeries` (`fid-chart-series`; kinds `CONTROL_XBAR`/`CONTROL_I` and `CONTROL_R`/`CONTROL_S`/`CONTROL_MR`) with `CENTRE`, `LCL`, `UCL` lines, data-set digest, algorithm ID `stats.spc.chart@1.0.0`, kernel version, constants table reference and `UNVERIFIED` flags where they apply.
>
> **Tests:** AC-1 to AC-5 above with `fixtures/rpt/stats/spc_xbar_r_25x5` and `spc_xbar_r_30x5`.
>
> **Constraints:** CONSTITUTION §8; C1 (values only from the kernel functions). Spec: `spec/rpt/stats/spc/chart/NODE.md`.

## Open questions

- Applying only `beyond_limits@1` to the dispersion chart is a conservative default pending SPC expert review [unverified].

## Children

_Leaf: none._
