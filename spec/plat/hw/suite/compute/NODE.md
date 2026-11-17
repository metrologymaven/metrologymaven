---
id: PLAT.HW.SUITE.COMPUTE
title: Load, fit, compare and report workloads
level: 4
parent: PLAT.HW.SUITE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW.SUITE-001, REQ-PLAT.HW.SUITE-004]
covers: []
depends_on: [PLAT.HW.CMD, CORE.BUS]
uses_artifacts: [CORE.REG.MAN, PLAT.HW.SUITE.DATA]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.SUITE.COMPUTE: Compute workloads

## Purpose

Implements the four non-render workloads as `Workload` trait objects for the `fid-bench` runner. Each one drives real registry commands through the command bus on a scratch copy of its dataset, exactly as a user's session would, and measures wall time and peak memory. Workloads name commands by ID, so they need no code dependency on the domain crates.

## Scope

**In:**
- `load_10m`: import a 10 M-point cloud into a scratch project; metrics `load_s`, `peak_mem`
- `fit_1000`: fit 1 000 features by least squares on a prepared part; metric `fit_s`
- `compare_1m`: align and compare a 1 M-point scan to CAD; metrics `compare_s`, `points_per_s`
- `report_20p`: build a 20-page report to a scratch export root; metric `report_s`
- A check that every command ID a workload invokes exists in the registry manifest

**Out** (and which node owns it instead):
- Render workloads (`PLAT.HW.SUITE.RENDER`)
- The runner, timeouts, statistics (`PLAT.HW.CMD`)
- The commands themselves (IO, ALN, KRN, SCAN, RPT)

## Requirements

- **REQ-PLAT.HW.SUITE.COMPUTE-001** Each workload shall perform its setup in `prepare` (copy dataset into scratch, open scratch project) and time only the measured commands in `run_once`.
- **REQ-PLAT.HW.SUITE.COMPUTE-002** Each workload shall invoke commands only through the bus by command ID with a `script` principal and shall write only inside the runner's scratch directory and scratch export root.
- **REQ-PLAT.HW.SUITE.COMPUTE-003** `peak_mem` shall be the process peak resident set size during `run_once`, in MiB.
- **REQ-PLAT.HW.SUITE.COMPUTE-004** If a command returns an error, then `run_once` shall return `WorkloadError::Command { id, code }` and the runner shall record `failed`.
- **REQ-PLAT.HW.SUITE.COMPUTE-005** A build-time test shall fail if any command ID used by a workload is absent from the registry manifest of the same build.

## Acceptance criteria

- **AC-1** Given miniature fixture datasets (`fixtures/plat/bench/mini-cloud.ply` 10 000 points, `mini-part/`, `mini-scan.ply`, `mini-report-project/`) and `quick` mode, when each workload runs through `fid-bench`, then each returns its metrics with status `ok`.
- **AC-2** Given a run, when the project store outside scratch is digested before and after, then the digests are equal.
- **AC-3** (error path) Given a fault-injected bus that fails `io.import_points@1` [unverified: command ID until IO names it], then `load_10m` is `failed` with that command ID.
- **AC-4** Given the manifest fixture missing one used command ID, then the command-existence test fails naming it.

## Design

- **Registry command(s):** none of its own; runs under `doctor.run_benchmark@1` (R1). Command IDs used are listed in one table `WORKLOAD_COMMANDS` checked against the manifest.
- **Data structures:** four structs implementing `Workload`; `PeakMem` sampler (OS API per platform).
- **Errors:** `WorkloadError::{Prepare, Command { id, code }, Io}`.
- **Determinism:** N (timings); the commands' results are deterministic per their own classes.
- **Crate:** `fid-bench` (module `workloads::compute`).

## Test fixtures

Miniature datasets under `fixtures/plat/bench/` created by a fixed-seed generator; licensed with the code.

## Build prompt

> In crate `fid-bench`, add module `workloads::compute` with `load_10m`, `fit_1000`, `compare_1m` and `report_20p` implementing the `Workload` trait from PLAT.HW.CMD. Each `prepare` copies its dataset into the scratch directory and opens a scratch project; each `run_once` invokes registry commands through the CORE bus by command ID as a `script` principal (import points; fit features; align and compare to CAD; build report to a scratch export root), times them, and samples peak RSS for `peak_mem` (MiB). Command errors return `WorkloadError::Command { id, code }`. Keep all used command IDs in `WORKLOAD_COMMANDS` and add a test that each exists in the build's registry manifest. Tests: AC-1..AC-4. Constraints: CONSTITUTION §3 (R1 scratch only; no project or record writes), §8. Commit: `feat(fid-bench): compute workloads`.

## Open questions

- Exact command IDs for import, fit, compare and report come from IO, KRN/GEO, SCAN and RPT leaves; the table is filled from the manifest when built.

## Children

_Leaf: none._
