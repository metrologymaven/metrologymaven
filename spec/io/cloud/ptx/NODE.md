---
id: IO.CLOUD.PTX
title: PTX reader with per-scan header transforms
level: 3
parent: IO.CLOUD
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD-006]
covers: [PW.IMP.01]
depends_on: [IO.CLOUD.STREAM, IO.REG.ENV, GEO.FRAME]
side_effect: W2
determinism: D0
cli: [fid import cloud-ptx]
mcp: [import_cloud_ptx]
journal: [import.cloud_ptx@1]
updated: 2026-11-16
---

# IO.CLOUD.PTX: PTX reader

## Purpose

Imports PTX text files (one or more gridded scans, each with a header holding grid size, scanner position and axes, and a 4×4 transform) as one point set per scan, recording each header transform as a frame instead of baking it into coordinates.

## Scope

**In:**
- Per-scan header: columns, rows, scanner position, three scanner axes, 4×4 row-major transform
- Point lines `x y z intensity [r g b]`; `0 0 0` points treated as missing returns, counted and skipped
- Grid row and column kept as per-point attributes
- `import.cloud_ptx@1` (W2)

**Out:**
- PTX export (not required by IO.CLOUD)
- Registration between scans (SCAN)

## Requirements

- **REQ-IO.CLOUD.PTX-001** When a PTX file is imported, the system shall create one point set per scan, each in a frame equal to that scan's header transform, with raw coordinates stored as written.
- **REQ-IO.CLOUD.PTX-002** If the scanner-position-and-axes block and the 4×4 transform disagree by more than 1e-9 in any component, then the import report shall warn and the 4×4 transform shall be used.
- **REQ-IO.CLOUD.PTX-003** Points equal to exactly `0 0 0` shall be skipped and counted as missing returns in the import report.
- **REQ-IO.CLOUD.PTX-004** If a scan holds fewer point lines than columns × rows, then the import shall fail with `truncated_file` naming the scan index.
- **REQ-IO.CLOUD.PTX-005** If the 4×4 transform's upper-left 3×3 block is not orthonormal within 1e-9, then the import shall fail with `malformed` naming the scan index.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cloud/ptx/two_scans.ptx` (scan 1: 3×2 grid, identity; scan 2: 2×2 grid, translation (1,2,3) m) and `--unit m`, when `fid import cloud-ptx --json` runs, then two point sets exist, scan 2's frame has translation (1,2,3), and its raw coordinates equal the file's.
- **AC-2** Given the same file with two `0 0 0` lines in scan 1, then scan 1 holds 4 points and the report counts 2 missing returns.
- **AC-3** (error path) Given `fixtures/io/cloud/ptx/short_scan.ptx`, then fails `truncated_file{scan: 2}`.
- **AC-4** (error path) Given `fixtures/io/cloud/ptx/shear.ptx` (non-orthonormal rotation), then fails `malformed{scan: 1}`.
- **AC-5** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.cloud_ptx@1` (W2; `source`, `unit`, `stage?`, `dry_run?`; result `ImportReport`).
- **Algorithm:** line reader with 4 KiB cap; header parsed as 10 lines; grid size charged to the budget before reading points; orthonormality check in f64 with fixed-order sums.
- **Errors:** `truncated_file`, `malformed`, `unit_required`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** 10^7 points in < 10 s [unverified until measured].
- **Crate:** `fid-io-cloud`.

## Test fixtures

Hand-written PTX files under `fixtures/io/cloud/ptx/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-cloud`, implement `import.cloud_ptx@1` (W2; CLI `fid import cloud-ptx`; MCP `import_cloud_ptx`) on the `fid-io-reg` skeleton. Per scan: read columns, rows, scanner position, three axes, 4×4 transform; check orthonormality (1e-9) else `malformed{scan}`; warn if position/axes disagree with the transform. Create one point set per scan in a GEO frame equal to the transform; never bake it into coordinates. Skip and count exact `0 0 0` points; keep grid row/column attributes; fewer than columns × rows lines → `truncated_file{scan}`. Unit required (PTX declares none). Stream through `PointSink`. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- PTX unit is conventionally metres [unverified]; the leaf still requires `unit` because the format does not declare it.

## Children

_Leaf: none._
