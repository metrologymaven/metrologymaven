---
id: ECO.DATA.BENCH.RUN
title: Provenance-capturing benchmark runner and history store
level: 4
parent: ECO.DATA.BENCH
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-xtask]
built_pr: 504
satisfies: [REQ-ECO.DATA.BENCH-001, REQ-ECO.DATA.BENCH-002, REQ-ECO.DATA.BENCH-003]
covers: []
depends_on: [ECO.DATA.FETCH]
uses_artifacts: [PLAT.HW.SCHEMA, PLAT.HW.SUITE, PLAT.HW.CMD, PLAT.HW.PROFILE, ECO.DATA.CAT]
side_effect: W1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.DATA.BENCH.RUN: Provenance-capturing benchmark runner and history store

## Purpose

Lets any contributor reproduce a published benchmark number: `cargo xtask bench run` reads the PLAT.HW suite manifest, verifies each workload's dataset through the fetch tool, runs the released `fid doctor run-benchmark` binary the stated number of times, and wraps its result documents in one provenance record (benchmark ID, dataset ID and digest, revision, build profile, machine profile ID, harness version, repetitions and run-to-run spread). Records are appended to a history store so later releases can be compared. Being tooling, it runs the product binary and reads files; it links no product crate.

## Scope

**In:**
- `cargo xtask bench run [--suite <toml>] [--workload <id>...] --machine-profile <id> --fid <path> --reps <n>`
- Dataset verification per workload and the `not-run` record
- Spread computation across repetitions (min, max, median, relative spread)
- `bench-record.json` v1 and the history store layout `bench-history/<revision>/<machine-profile>/<workload>.json`
- `cargo xtask bench compare <recordA> <recordB>` printing per-metric differences

**Out:**
- Workload code, metrics, suite content and result schema (owned by `PLAT.HW.SUITE`, `PLAT.HW.SCHEMA`, `PLAT.HW.CMD`)
- Regression thresholds and release runs on reference machines (owned by `PLAT.HW.RUN`)
- The release bundle (owned by `ECO.DATA.BENCH.BUNDLE`)

## Requirements

- **REQ-ECO.DATA.BENCH.RUN-001** Every record shall hold `benchmark_id` (suite workload ID), `dataset {id, sha256}`, `revision` (40-hex commit of the `fid` build), `build_profile`, `machine_profile_id`, `harness_version`, `repetitions`, and the PLAT.HW result documents of each repetition unchanged.
- **REQ-ECO.DATA.BENCH.RUN-002** If a workload's dataset ID is not in the catalogue, or fetch verification fails, then the runner shall not start that workload and shall write a record with `status = "not-run"` and `reason` (`NOT-IN-CATALOGUE` or the fetch error code).
- **REQ-ECO.DATA.BENCH.RUN-003** For every metric, the record shall give median, min, max and relative spread `(max − min) / median` over the repetitions, and the runner shall refuse `--reps` below 3 with `BENCH-REPS`.
- **REQ-ECO.DATA.BENCH.RUN-004** If the machine profile ID is not in the supplied profile file, then the runner shall exit 1 with `BENCH-PROFILE` before running anything.
- **REQ-ECO.DATA.BENCH.RUN-005** The runner shall write each record atomically to the history store path and shall never overwrite an existing record; a rerun for the same key gets a numeric suffix.
- **REQ-ECO.DATA.BENCH.RUN-006** When `compare` runs on two records of the same workload, it shall print per-metric median differences in the metric's unit and percent, and shall warn `BENCH-INCOMPARABLE` if the dataset digest, machine profile or build profile differs.

## Acceptance criteria

- **AC-1** Given suite `fixtures/bench/suite.toml` (two workloads on catalogue datasets), catalogue `fixtures/bench/datasets.toml`, profiles `fixtures/bench/profiles.toml` and the fake runner `fixtures/bench/fake-fid` (emits fixed PLAT.HW result documents with metric values 100, 102, 98 on successive calls), when `cargo xtask bench run --suite ... --machine-profile min-2026 --fid fixtures/bench/fake-fid --reps 3 --history <tmp>` runs, then two records exist, each with all REQ-001 fields, median 100, min 98, max 102 and relative spread 0.04.
- **AC-2** (error path) Given a suite workload whose dataset is absent from the catalogue, then its record has `status = "not-run"`, `reason = "NOT-IN-CATALOGUE"`, and the fake runner was never called for it; given a dataset with a wrong digest in the cache under offline mode, the reason is the fetch error code.
- **AC-3** (error path) `--reps 2` exits 1 with `BENCH-REPS`; `--machine-profile nope` exits 1 with `BENCH-PROFILE` and writes nothing.
- **AC-4** Running AC-1 twice leaves four records (the second set suffixed `-1`), none overwritten.
- **AC-5** Given two fixture records differing in machine profile, `bench compare` prints the differences and `BENCH-INCOMPARABLE machine_profile_id`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask bench run|compare`. `side_effect: W1`: writes records into a local history root. It invokes `fid doctor run-benchmark` (R1) as a subprocess.
- **Data structures:** `bench-record.json` `{schema: 1, benchmark_id, dataset, revision, build_profile, machine_profile_id, harness_version, repetitions, status, reason?, metrics: [{name, unit, median, min, max, rel_spread}], results: [<PLAT.HW BenchResult>...]}`; metric values as decimal strings copied from the result documents, spread computed in f64 and written with 6 significant digits.
- **Algorithm:** load suite (PLAT.HW.SCHEMA format); for each workload: look up dataset in the catalogue, call the fetch library's `dataset_path`; on failure write not-run; else run the binary `reps` times with `--workload <id> --dataset <path> --json`, validate each result against the PLAT.HW result schema version, aggregate; write record via temp file and rename.
- **Errors:** `BENCH-REPS`, `BENCH-PROFILE`, `BENCH-SUITE`, `BENCH-RESULT` (result document fails schema), `BENCH-IO`.
- **Determinism:** N (timings are observations); the aggregation of given values is deterministic.
- **Performance budget:** harness overhead under 1 s per workload beyond the runs [unverified until measured].
- **Crate:** `fid-xtask` (module `bench`), using the `fid-data` fetch library.

## Test fixtures

A fake `fid` script returning fixed PLAT.HW result documents, a two-workload suite, catalogue and profile files under `fixtures/bench/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `bench run --suite <toml> [--workload <id>...] --machine-profile <id> --profiles <toml> --fid <binary> --reps <n≥3> --history <dir>` and `bench compare <a> <b>`. For each suite workload (suite format from PLAT.HW.SCHEMA): resolve its dataset via the catalogue and `fid-data`'s `dataset_path` (offline respected); on failure write a `not-run` record with reason `NOT-IN-CATALOGUE` or the fetch error code and skip; else run `<fid> doctor run-benchmark --workload <id> --json` `reps` times, validate each result document against the PLAT.HW result schema, and write `bench-record.json` `{schema: 1, benchmark_id, dataset: {id, sha256}, revision, build_profile, machine_profile_id, harness_version, repetitions, status, metrics: [{name, unit, median, min, max, rel_spread}], results: [...]}` atomically to `<history>/<revision>/<profile>/<workload>.json` (never overwrite; suffix `-1`, `-2`). Errors `BENCH-REPS`, `BENCH-PROFILE` (checked before any run), `BENCH-SUITE`, `BENCH-RESULT`, `BENCH-IO`. `compare` prints per-metric median deltas and warns `BENCH-INCOMPARABLE` on differing dataset digest, machine or build profile. Do not link product crates; run the binary. Tests for AC-1..AC-5 of spec node ECO.DATA.BENCH.RUN with a fake `fid`. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- The exact CLI flags of `fid doctor run-benchmark` are owned by PLAT.HW.CMD; the harness adapts to them.

## Children

_Leaf: none._
