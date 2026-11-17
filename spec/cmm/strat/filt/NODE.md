---
id: CMM.STRAT.FILT
title: Scan filter settings and filter provenance
level: 3
parent: CMM.STRAT
kind: feature
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-006]
covers: [PCD.SCN.05]
depends_on: [KRN]
side_effect: W2
determinism: D0
cli: [fid cmm set-scan-filter]
mcp: [cmm_set_scan_filter]
journal: [cmm.set_scan_filter@1]
updated: 2026-11-16
---

# CMM.STRAT.FILT: Scan filter settings and filter provenance

## Purpose

Stores the filter to apply to acquired scan data as part of a scan strategy (algorithm ID and typed parameters, validated against the algorithm's declared parameter schema), and stamps every filtered scan result with the algorithm ID and version that produced it. The filter algorithms themselves live in the kernel.

## Scope

**In:**
- `ScanFilterSetting { algorithm_id, params }` attached to a scan strategy
- Validation against the kernel filter catalogue (algorithm exists, parameters match its schema and bounds)
- `cmm.set_scan_filter@1` (W2)
- `apply_scan_filter(setting, points) -> FilteredScan` that calls the kernel and records algorithm ID and version in the output

**Out** (and which node owns it instead):
- Filter algorithms and their catalogue (owned by `KRN`)
- Scan strategy types (owned by `CMM.STRAT.SCAN`)

## Requirements

- **REQ-CMM.STRAT.FILT-001** When a filter setting is set on a scan strategy, the system shall reject it with `FILTER_UNKNOWN` if the algorithm ID is not in the kernel filter catalogue, and with `FILTER_PARAM_INVALID` naming the parameter if a parameter violates the algorithm's declared schema.
- **REQ-CMM.STRAT.FILT-002** The system shall store the filter setting as part of the scan strategy, so the strategy digest changes when the filter or any parameter changes.
- **REQ-CMM.STRAT.FILT-003** When scan data is filtered, the system shall record the algorithm ID with its semantic version, the parameter values and the input point-set digest in the filtered result.
- **REQ-CMM.STRAT.FILT-004** The system shall allow `none` as an explicit filter setting, recorded as such, so an unfiltered scan is distinguishable from a missing setting.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/strat/scan_line_strategy.json` and the kernel test filter `test.filter.identity@1.0.0`, when `fid cmm set-scan-filter --strategy <id> --algorithm test.filter.identity --json` runs, then the setting is stored and the strategy digest changes.
- **AC-2** Given the AC-1 strategy and fixture `fixtures/cmm/strat/scan_points_100.json`, when the filter is applied, then the result carries `algorithm: test.filter.identity@1.0.0`, the parameters and the input digest from the fixture.
- **AC-3** (error path) Given algorithm ID `no.such.filter`, when set, then the command fails with `FILTER_UNKNOWN` and journals nothing.
- **AC-4** (error path) Given a filter whose schema requires a positive cutoff and a cutoff of −1, when set, then the command fails with `FILTER_PARAM_INVALID` naming `cutoff`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `cmm.set_scan_filter@1`, W2. Params: `strategy_id`, `algorithm_id: String | "none"`, `params: Map` (ordered). Result: `{strategy_id, strategy_digest}`.
- **Library function:** `apply_scan_filter(&ScanFilterSetting, &PointSet) -> Result<FilteredScan, FilterError>`; `FilteredScan { points, algorithm_id_version, params, input_digest }`.
- **Errors:** `FILTER_UNKNOWN`, `FILTER_PARAM_INVALID { param }`, `STRATEGY_NOT_FOUND`.
- **Determinism:** D0 for the setting; filtered output inherits the kernel filter's class.
- **Crate:** `fid-cmm-strat`.

## Test fixtures

`fixtures/cmm/strat/scan_line_strategy.json` and `scan_points_100.json` (seeded synthetic line scan, seed in file), CC-BY-4.0; the kernel's registered test filter `test.filter.identity@1.0.0`.

## Build prompt

> In the Rust crate `fid-cmm-strat`, implement scan filter settings: command `cmm.set_scan_filter@1` (W2, journaled; CLI `fid cmm set-scan-filter --json`; MCP `cmm_set_scan_filter`, generated) and library function `apply_scan_filter`.
> Validate the algorithm ID against the kernel filter catalogue (`FILTER_UNKNOWN`) and its parameters against the declared schema (`FILTER_PARAM_INVALID {param}`); `none` is an explicit setting. The setting is part of the scan strategy, so its digest changes. Filtered results record algorithm ID with semver, parameters and input point-set digest.
> Acceptance tests: the five criteria in spec node `CMM.STRAT.FILT`, fixtures in `fixtures/cmm/strat/`.
> Constraints: filters run only through the kernel; no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0.

## Open questions

- The kernel filter catalogue's exact IDs are owned by `KRN`; the test filter name is a placeholder until `KRN` fixes it.

## Children

_Leaf: none._
