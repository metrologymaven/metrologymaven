---
id: PLAT.HW
title: Hardware profiles, reference machines and release benchmarks
level: 2
parent: PLAT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT-004]
covers: []
depends_on: [PLAT.MATRIX, ECO.CI]
cli: [fid doctor run-benchmark]
mcp: [doctor_run_benchmark]
journal: []
updated: 2026-11-16
---

# PLAT.HW: Hardware profiles and reference benchmarks

## Purpose

Publishes the minimum and recommended hardware profiles (CPU, RAM, GPU, VRAM, storage, display) and backs them with numbers: a benchmark suite run on named reference machines for every release. It exists so that "will it run on our shop-floor PC?" has a published, measured answer that does not drift between releases.

## Scope

**In:**
- Minimum and recommended hardware profiles as a machine-readable file
- Named reference benchmark machines and their specifications
- The benchmark suite: workloads, datasets and metrics (load time, frame rate, fit and comparison throughput, memory peak)
- Running the benchmark suite on reference machines per release and publishing results
- The `fid doctor run-benchmark` command for users to run the same suite locally
- Regression thresholds between releases

**Out** (and which node owns it instead):
- OS and architecture tiers (`PLAT.MATRIX`)
- GPU backend policy and fallbacks (`PLAT.GPU`)
- Checking a machine against the minimum profile (`PLAT.DOCTOR`)
- Benchmark datasets as corpora and CI runners (`ECO`)
- Optimising the code that the benchmarks measure (`VIZ`, `KRN`, `SCAN`)

## Requirements

- **REQ-PLAT.HW-001** The project shall publish minimum and recommended hardware profiles in a versioned machine-readable file stating CPU class, core count, RAM, GPU class, VRAM, free storage and display resolution.
- **REQ-PLAT.HW-002** The project shall name at least one reference machine at the minimum profile and one at the recommended profile, with their full hardware and OS specification.
- **REQ-PLAT.HW-003** When a release is built, the benchmark suite shall run on every named reference machine and the results shall be published with the release.
- **REQ-PLAT.HW-004** The benchmark suite shall use fixed, published datasets and record the dataset digest, app version, OS, backend and driver version with each result.
- **REQ-PLAT.HW-005** If a benchmark metric on a reference machine regresses by more than 10 % against the previous release, then the release notes shall flag the regression [unverified: threshold to be tuned].
- **REQ-PLAT.HW-006** When a user runs `fid doctor run-benchmark`, the command shall run the published suite locally and report results in the same schema as the reference results, in human-readable and JSON form.
- **REQ-PLAT.HW-007** The recommended profile shall be one on which a 10-million-point cloud renders at no less than 30 frames per second [unverified: target to be measured; VIZ owns the rate].

## Interfaces

- **Offers:** hardware profile file (read by `PLAT.DOCTOR` and the docs); benchmark result schema; `doctor.run_benchmark@1` (R1, compute with scratch result; CLI `fid doctor run-benchmark`, MCP `doctor_run_benchmark`).
- **Consumes:** ECO benchmark datasets, release pipeline and runners; `PLAT.MATRIX` platform list.

## Decomposition hints

PROFILE (profile file and docs), REFMACH (reference machine registry), SUITE (workloads and metrics), RUN (release-time execution and publishing), CMD (`fid doctor run-benchmark`).

## Open questions

- Who hosts and pays for physical reference machines: raised as **D-014**, which blocks `PLAT.HW.REFMACH` and part of `PLAT.HW.RUN`.
- Journal behaviour for R1 benchmark runs follows CORE; `PLAT.HW.CMD` names the event `doctor.run_benchmark@1` and defers the R1 journaling rule to CORE.

## Split rationale

The split separates **formats, content, machines, execution and the user command**: PROFILE and SCHEMA are the two published formats (small leaves), SUITE decides what is measured, REFMACH decides where it is measured, CMD is the one runner that both users and the release pipeline call, and RUN orchestrates release-time runs and regression flags. The money question (D-014) is isolated in REFMACH and RUN, so the rest can proceed.

Alternatives considered:
- **Split by metric family** (render, compute, I/O). Rejected: each family would need its own runner, schema and publishing.
- **Separate runners for users and reference machines.** Rejected: identical results require identical code; RUN calls CMD.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.HW.PROFILE | Hardware profile file and validator | 001 | none |
| PLAT.HW.REFMACH | Reference benchmark machine registry | 002 | none |
| PLAT.HW.SCHEMA | Benchmark suite manifest and result schema | 004 | none |
| PLAT.HW.SUITE | Benchmark workloads, datasets and metrics | 004 007 | none |
| PLAT.HW.RUN | Release benchmark runs, publishing and regression flags | 003 005 | none |
| PLAT.HW.CMD | fid doctor run-benchmark command and runner | 006 | none |
