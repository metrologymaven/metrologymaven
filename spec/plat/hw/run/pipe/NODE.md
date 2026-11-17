---
id: PLAT.HW.RUN.PIPE
title: Release-time dispatch to reference machines and missing-run report
level: 4
parent: PLAT.HW.RUN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW.RUN-001, REQ-PLAT.HW.RUN-002]
covers: []
depends_on: [PLAT.HW.REFMACH, PLAT.HW.RUN.COMPARE]
uses_artifacts: [ECO.DATA.BENCH.RUN, PLAT.HW.CMD, PLAT.HW.SUITE, ECO.REL.PROC.FLOW]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.RUN.PIPE: Release benchmark dispatch

## Purpose

At release time, sends one benchmark job to every machine in the reference registry: the maintainer-owned minimum machine through its self-hosted runner, and the recommended profile through a per-release cloud rental (D-014). Each job runs `cargo xtask bench run` with the release binary, collects the result documents, and the pipeline attaches them to the release. A machine that did not deliver is named in the notes, never silently skipped.

## Scope

**In:**
- Generating one job per registry machine: `bench-<machine>` with runner label (self-hosted) or rental spec (instance type)
- The host-rule pre-check (idle period, no other workload) recorded in the job output
- Collecting result documents and writing `bench-runs.json` (per machine: `ok`, `failed`, `missing`, with reason)
- Calling the comparison and handing `bench-regressions.json` and `bench-runs.json` to the release notes input

**Out** (and which node owns it instead):
- Provisioning cloud accounts, credentials and spending (maintainers; ECO.CI secrets)
- Provenance capture (`ECO.DATA.BENCH.RUN`)
- Regression rules (`PLAT.HW.RUN.COMPARE`)

## Requirements

- **REQ-PLAT.HW.RUN.PIPE-001** When a release is built, the generator shall emit exactly one benchmark job per machine in `platform/ref-machines.toml`, using the machine's self-hosted runner label for `maintainer_owned` and its instance type for `cloud_rented`.
- **REQ-PLAT.HW.RUN.PIPE-002** Each job shall run the host-rule pre-check, then `cargo xtask bench run` with the release `fid` binary, the shipped suite and `--machine-profile` from the registry, and upload the records and result documents.
- **REQ-PLAT.HW.RUN.PIPE-003** If a job fails, times out after 60 minutes or uploads no result, then `bench-runs.json` shall mark that machine `failed` or `missing` with the reason, and the notes input shall name it.
- **REQ-PLAT.HW.RUN.PIPE-004** A cloud rental shall be started for the job and released when the job ends, whatever its outcome [unverified: provider tooling].
- **REQ-PLAT.HW.RUN.PIPE-005** Benchmark jobs shall not block the release; their outcome is reported, per REQ-PLAT.HW.RUN-002.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/hw/ref-machines-valid.toml`, when `cargo xtask plat-hw bench-jobs` runs, then it emits two jobs equal to `fixtures/plat/hw/run/expected-bench-jobs.json`.
- **AC-2** Given collected results for `min-1` only, when `cargo xtask plat-hw bench-collect` runs, then `bench-runs.json` marks `rec-1` `missing` and the notes input names it.
- **AC-3** Given a job whose pre-check finds another workload, then the job records `host_busy` and fails, and the machine is reported `failed`.
- **AC-4** (determinism) Given AC-2 inputs twice, then outputs are byte-identical.

## Design

- **Registry command(s):** none; release tooling (`xtask plat-hw bench-jobs`, `xtask plat-hw bench-collect`), plus workflow YAML owned by ECO.CI that calls them.
- **Data structures:** `BenchJob { machine, runner: Option<label>, instance_type: Option<String>, timeout_min: 60 }`; `BenchRuns { release, machines: Vec<{ machine, status: ok | failed | missing, reason? }> }`.
- **Algorithm:** generate jobs from the registry → (workflow) run → collect artifacts → `bench_compare` against the previous release's published results → write both files.
- **Errors:** `NoMachines`, input errors; job failures are statuses.
- **Determinism:** N for runs; D0 for generation and collection.
- **Crate:** `xtask` (module `plat_hw_bench`).

## Test fixtures

Registry and result fixtures under `fixtures/plat/hw/`. Licensed with the code.

## Build prompt

> Add `xtask plat-hw bench-jobs` (one job per machine in `platform/ref-machines.toml`: self-hosted runner label for maintainer_owned, instance type for cloud_rented, 60-minute timeout) and `xtask plat-hw bench-collect --results <dir> --previous <dir>` that writes `bench-runs.json` (ok | failed | missing with reason per machine), runs `fid-platform::bench_compare` to write `bench-regressions.json`, and produces the notes input naming every failed or missing machine. Each job runs the host-rule pre-check (`host_busy` on failure), then `cargo xtask bench run` (ECO.DATA.BENCH.RUN) with the release `fid`, the shipped suite and the machine's profile ID. Rentals start and stop with the job. Jobs never block the release. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8; tooling reads artifacts only; no credentials in the repository. Commit: `feat(xtask): release benchmark dispatch`.

## Open questions

- Rental provider and credentials are maintainer and ECO.CI operations decisions (spending money).

## Children

_Leaf: none._
