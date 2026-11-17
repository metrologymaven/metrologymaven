---
id: PLAT.HW.RUN.COMPARE
title: Regression comparison against the previous release
level: 4
parent: PLAT.HW.RUN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW.RUN-003, REQ-PLAT.HW.RUN-004, REQ-PLAT.HW.RUN-005]
covers: []
depends_on: [PLAT.HW.SCHEMA, PLAT.HW.REFMACH]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.RUN.COMPARE: Regression comparison

## Purpose

Decides, metric by metric, whether a release got slower or heavier than the previous one on the same reference machine, using each metric's `better` direction from the suite manifest and the registry's comparability breaks. The output feeds the release notes.

## Scope

**In:**
- `xtask plat-hw compare --current <dir> --previous <dir> --machines <file> --suite <file> --threshold 0.10`
- `bench-regressions.json` v1: per machine and metric, previous and current median, relative change, verdict
- Verdicts: `ok`, `improved`, `regressed`, `not_comparable`, `new_metric`, `missing_previous`, `not_ok` (status not `ok` in either run)

**Out** (and which node owns it instead):
- Running benchmarks (`PLAT.HW.RUN.PIPE`)
- Rendering notes (`ECO.REL.PROC.NOTES`)

## Requirements

- **REQ-PLAT.HW.RUN.COMPARE-001** For a metric with `better = lower`, the verdict shall be `regressed` when `(current − previous) / previous > threshold`; for `better = higher`, when `(previous − current) / previous > threshold`; the default threshold shall be 0.10 [unverified: to be tuned].
- **REQ-PLAT.HW.RUN.COMPARE-002** If `is_comparable(machine, previous_date, current_date)` is false, or the dataset digest of the metric differs between runs, then every verdict for that machine and metric shall be `not_comparable`.
- **REQ-PLAT.HW.RUN.COMPARE-003** If either run's metric status is not `ok`, then the verdict shall be `not_ok` and no change shall be computed.
- **REQ-PLAT.HW.RUN.COMPARE-004** The output shall be RFC 8785 canonical JSON sorted by machine, workload and metric, byte-identical for the same inputs, and shall include a `regressed_count`.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/hw/run/prev/` and `cur/` (machine `min-1`, load time 10.0 s → 11.5 s, fps 30 → 31) with `better` from `fixtures/plat/bench/mini-suite.toml`, when compare runs, then load time is `regressed` (+15 %), fps `improved`, and `regressed_count` is 1.
- **AC-2** Given a registry fixture with a driver change between the two dates, then all verdicts for `min-1` are `not_comparable`.
- **AC-3** Given a current metric with status `timeout`, then `not_ok`.
- **AC-4** (determinism) Given AC-1 twice, then output bytes are identical.

## Design

- **Registry command(s):** none; `xtask` tooling.
- **Data structures:** `Regressions { schema_version: 1, release, previous_release, threshold, rows: Vec<{ machine, workload, metric, unit, previous, current, change, verdict }>, regressed_count }`.
- **Algorithm:** index both result sets by (machine, workload, metric) in `BTreeMap`s; apply the rules in order: comparability, status, missing, threshold.
- **Errors:** input parse errors; `UnknownMachine`.
- **Determinism:** D0 (fixed-order arithmetic in f64; results rounded to 4 decimals in output).
- **Crate:** `fid-platform` (module `bench_compare`) plus `xtask`.

## Test fixtures

Result documents under `fixtures/plat/hw/run/`, registry fixtures from `PLAT.HW.REFMACH`. Licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `bench_compare` and `xtask plat-hw compare --current <dir> --previous <dir> --machines <file> --suite <file> [--threshold 0.10]`. Read PLAT.HW.SCHEMA result documents, key by (machine, workload, metric), and emit `bench-regressions.json` v1 (RFC 8785, sorted) with verdicts ok | improved | regressed | not_comparable | new_metric | missing_previous | not_ok, using each metric's `better` direction, the registry's `is_comparable`, and dataset digests. Include `regressed_count`. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8; `BTreeMap` only. Commit: `feat(fid-platform): benchmark regression comparison`.

## Open questions

- Noise: a 10 % threshold may be inside run-to-run spread on rented machines [unverified]; ECO.DATA.BENCH.RUN reports spread, which a later revision could use.

## Children

_Leaf: none._
