---
id: OUTR.COMM.METRIC.SNAP
title: Metric snapshot store and quarterly values
level: 4
parent: OUTR.COMM.METRIC
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 441
satisfies: [REQ-OUTR.COMM.METRIC-004]
covers: []
depends_on: [OUTR.COMM.METRIC.DEF]
uses_artifacts: []
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.METRIC.SNAP: Metric snapshot store and quarterly values

## Purpose

The format and folder layout of raw metric snapshots, and the pure function that turns a quarter's snapshots into one value per metric. Because every quarterly number is recomputed from stored raw responses, anyone with the snapshot archive can check a published adoption figure.

## Scope

**In:** the snapshot record format; the store layout; the `metric-quarter` command that computes quarterly values and re-extracts them from raw responses; the quarterly values file read by the report.
**Out:** fetching (`OUTR.COMM.METRIC.COLLECT`); definitions (`OUTR.COMM.METRIC.DEF`); the report (`OUTR.COMM.REPORT`).

## Requirements

- **REQ-OUTR.COMM.METRIC.SNAP-001** Each snapshot record shall carry the metric ID, the definition's SHA-256, the collection timestamp (UTC), the source URL, the stored raw response file name and its SHA-256, and the extracted value with unit.
- **REQ-OUTR.COMM.METRIC.SNAP-002** When `metric-quarter` runs for a quarter, it shall compute each metric's value from the snapshots whose timestamps fall in that quarter using the definition's aggregation, and write `quarters/<YYYY>Q<n>.json` with value, unit, snapshot count and the digests of the snapshots used.
- **REQ-OUTR.COMM.METRIC.SNAP-003** When computing, the command shall re-extract each value from its stored raw response with the definition's selector and fail with `snapshot_mismatch` if the stored value differs or the raw file's SHA-256 does not match.
- **REQ-OUTR.COMM.METRIC.SNAP-004** If a metric has no snapshot in the quarter, then its entry shall be `unavailable` with reason `no_snapshot`, never zero.
- **REQ-OUTR.COMM.METRIC.SNAP-005** The store shall be append-only: writing a snapshot whose file name exists shall fail with `snapshot_exists`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/metric/snap/q1/` (stars snapshots on 2027-01-15 = 120, 2027-03-30 = 180; downloads snapshots with `sum` aggregation 40 and 60), when `fid-outreach metric-quarter --store q1/ --defs defs.toml --quarter 2027Q1 --out out/` runs, then `out/2027Q1.json` has stars 180 (`last`) and downloads 100 (`sum`) with snapshot counts 2 and 2.
- **AC-2** (error path) Given `tampered/` (a raw response edited after collection), when the command runs, then it exits 1 with `snapshot_mismatch` naming the snapshot.
- **AC-3** Given `gap/` (no forks snapshot in the quarter), when the command runs, then forks is `unavailable` with `no_snapshot`.
- **AC-4** (error path) Given an attempt to write a snapshot under an existing name, when the store writer runs, then it fails with `snapshot_exists`.
- **AC-5** (determinism) Given the same store, when the command runs twice, then the output files are byte-identical.

## Design

- **Command:** `fid-outreach metric-quarter --store <dir> --defs <file> --quarter <YYYYQn> --out <dir> [--json]`; a library `SnapshotStore::append(record, raw_bytes)` used by the collector. Project tooling; nearest class W1 (writes only inside the given folders).
- **Layout:** `snapshots/<metric-id>/<UTC timestamp>.json` (record) and `.raw` (raw response bytes); `quarters/<YYYY>Q<n>.json`.
- **Arithmetic:** values are non-negative integers (counts); `sum` and `max` in u64 with overflow treated as an error; timestamps compared as RFC 3339 UTC strings after validation.
- **Errors:** `snapshot_mismatch`, `snapshot_exists`, `definition_changed` (definition digest in a snapshot differs from the current one: reported, value kept, flagged in output), `quarter_invalid`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `metrics::snap`.

## Test fixtures

Synthetic snapshot stores under `fixtures/outr/comm/metric/snap/` (`q1`, `tampered`, `gap`) with small JSON raw responses, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-outreach`, add module `metrics::snap` with a `SnapshotStore` (append-only; layout `snapshots/<metric-id>/<UTC timestamp>.json` plus `.raw`; `snapshot_exists` on name clash) and subcommand `fid-outreach metric-quarter --store <dir> --defs <file> --quarter <YYYYQn> --out <dir> [--json]`.
>
> Snapshot record: `metric_id`, `definition_sha256`, `collected_at` (RFC 3339 UTC), `source_url`, `raw_file`, `raw_sha256`, `value` (non-negative integer), `unit`. For the quarter: select snapshots by timestamp, verify each raw file's SHA-256 and re-extract the value with the definition's selector (JSON Pointer for http_json; the stored count for git_history and manual_public_count), failing with `snapshot_mismatch` on any difference; aggregate with `last`, `sum` or `max` per definition (u64, overflow is an error); write `quarters/<YYYY>Q<n>.json` with per metric value, unit, snapshot count and snapshot digests, or `unavailable` with `no_snapshot`. Flag `definition_changed` when a snapshot's definition digest differs from the current definition.
>
> Tests with fixtures under `fixtures/outr/comm/metric/snap/`: q1 aggregation (stars 180 last, downloads 100 sum), tampered raw file, gap gives unavailable, duplicate write refused, byte-identical output.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no clock reads (the quarter is an argument); no network; BTreeMap rather than HashMap.

## Open questions

- Where the snapshot archive is published (with the reports, or in its own repository) is for maintainers; it must be public for REQ-OUTR.COMM.REPORT-003 regeneration by third parties.

## Children

_Leaf: none._
