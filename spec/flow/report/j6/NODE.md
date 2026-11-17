---
id: FLOW.REPORT.J6
title: J6 SPC continuity from imported history plus new runs
level: 3
parent: FLOW.REPORT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.REPORT-001, REQ-FLOW.REPORT-008, REQ-FLOW.REPORT-010, REQ-FLOW.REPORT-011]
covers: [JOB.ACC.06]
depends_on: [IO.RES.AQDEF, IO.RES.CSV, RPT.CATALOG.IMPORT, RPT.STATS.DSET, RPT.STATS.SPC, RPT.STATS.CAP, RPT.MODEL.FMT, VIZ.OVERLAY.STATUS]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.REPORT.J6: J6 SPC continuity from imported history plus new runs

## Purpose

Owns acceptance job `JOB.ACC.06`: a quality manager imports 6 months of history from another SPC tool (AQDEF or CSV), adds 30 new runs, and produces Xbar-R charts and Cpk for 10 key characteristics, with imported points labelled RESULTS_ONLY and indices equal to independently computed reference values.

## Scope

**In:**
- Synthetic data: 6 months of history for 10 characteristics in both AQDEF and CSV form, 30 new runs, and reference control limits and indices computed by an independent script in exact rational arithmetic
- Journey map `JOB.ACC.06`; packages `j6-unassisted` and `j6-assisted`, both tracked

**Out:**
- Import, catalog, statistics and formatting (owned by `IO.RES`, `RPT.CATALOG`, `RPT.STATS`, `RPT.MODEL.FMT`)

## Requirements

- **REQ-FLOW.REPORT.J6-001** The J6 data generator shall produce, from a seed, subgroups of size 5 for 10 characteristics covering 26 weeks, written as AQDEF and as CSV with identical values, plus 30 new-run result sets.
- **REQ-FLOW.REPORT.J6-002** The J6 reference script shall compute Xbar-R control limits and Cpk per characteristic in exact rational arithmetic and write them to the truth file.
- **REQ-FLOW.REPORT.J6-003** The `j6-unassisted` package shall declare an `index_equals` check that each control limit and Cpk equals the reference value to the formatter's displayed digits.
- **REQ-FLOW.REPORT.J6-004** The package shall declare a `basis_label` check that every imported point's ResultRecord has evaluation basis `RESULTS_ONLY` and is shown with the RESULTS_ONLY badge.
- **REQ-FLOW.REPORT.J6-005** The package shall declare a task-time target of under 10 minutes [unverified] and be declared tracked, not gating; `j6-assisted` shall be tracked.

## Acceptance criteria

- **AC-1** Given seed 6, when the generator and reference script run twice, then outputs are byte-identical.
- **AC-2** Given the J6 packages, when `fid accept run-suite --job JOB.ACC.06 --json` runs, then all checks pass and the job is reported as tracked.
- **AC-3** (error path) Given a truth file with one Cpk altered in the last displayed digit, then `index_equals` fails naming the characteristic.

## Design

- **Registry command(s):** none; generator `cargo xtask accept-data report --job j6`; content under `acceptance/jobs/j6/`.
- **Data structures:** packages per `acceptance-job@1`; checks `replay_outcome`, `index_equals`, `basis_label`, `result_count`.
- **Algorithm:** normal subgroups from the counter-based RNG with two injected shifts; reference limits with tabulated control-chart constants computed in rationals [unverified constant source; derived, not copied].
- **Errors:** generator `DATA-IO`.
- **Determinism:** D0; times observed (N).
- **Performance budget:** none beyond targets.
- **Crate:** `fid-accept-data`.

## Test fixtures

Generated data, CC0, recorded in `datasets.toml`.

## Build prompt

> Add `--job j6` to `cargo xtask accept-data report`: 10 characteristics x 26 weeks of subgroups of 5 (seeded, two injected shifts) written as AQDEF and CSV with identical values, 30 new runs, and a reference script computing Xbar-R limits and Cpk in exact rationals into `truth.json`. Create `acceptance/jobs/j6/` with `map.json` (`JOB.ACC.06`) and tracked packages `j6-unassisted` (10 min) and `j6-assisted`; checks replay_outcome, index_equals (to displayed digits), basis_label (RESULTS_ONLY), result_count. Verify AC-1..AC-3 of spec node FLOW.REPORT.J6.

## Open questions

- J6 is tracked; doc 10 says it becomes gating if a design partner's contract depends on it. Changing the flag is a maintainer decision.

## Children

_Leaf: none._
