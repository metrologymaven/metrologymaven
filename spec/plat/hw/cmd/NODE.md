---
id: PLAT.HW.CMD
title: fid doctor run-benchmark command and runner
level: 3
parent: PLAT.HW
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-006]
covers: []
depends_on: [PLAT.HW.SCHEMA, PLAT.GPU.DETECT, CORE, AUTO]
side_effect: R1
determinism: N
cli: [fid doctor run-benchmark]
mcp: [doctor_run_benchmark]
journal: [doctor.run_benchmark@1]
updated: 2026-11-16
---

# PLAT.HW.CMD: `fid doctor run-benchmark`

## Purpose

Lets any user run the published benchmark suite on their own machine and get results in exactly the schema of the reference results, so "how does my PC compare with the reference machines?" has a one-command answer. The same runner is used by the release pipeline on reference machines.

## Scope

**In:**
- Registry command `doctor.run_benchmark@1` (R1) with its generated CLI and MCP projections
- The runner: load the suite manifest, check datasets in the local cache, run workloads with repetitions and timeouts, compute statistics, fill the context block, emit the result document
- A `Workload` trait that suite workloads implement
- Human-readable and JSON output

**Out** (and which node owns it instead):
- The workloads themselves (`PLAT.HW.SUITE`)
- Manifest and result schema (`PLAT.HW.SCHEMA`)
- Fetching datasets (`ECO.DATA`; this command never fetches)
- Release-time orchestration and comparison (`PLAT.HW.RUN`)

## Requirements

- **REQ-PLAT.HW.CMD-001** When `fid doctor run-benchmark` runs, the command shall execute every workload of the installed suite manifest, or only those named by `--workload`, with the manifest's repetitions, and report results in the `PLAT.HW.SCHEMA` result format.
- **REQ-PLAT.HW.CMD-002** When run with `--json`, the command shall print the result document as JSON; without it, it shall print a table of workload, metric, median, unit and status.
- **REQ-PLAT.HW.CMD-003** If a workload's dataset is not in the local dataset cache or its SHA-256 differs from the manifest, then that workload shall be reported with status `skipped` and the reason, and the command shall make no network request.
- **REQ-PLAT.HW.CMD-004** If a workload exceeds its `timeout_s`, then the runner shall stop it, record status `timeout` and continue with the next workload.
- **REQ-PLAT.HW.CMD-005** The command shall be side-effect class R1: it shall write no project, record or configuration state and shall write files only in a temporary scratch directory that it removes before returning.
- **REQ-PLAT.HW.CMD-006** The result context shall be filled from the running machine, with `machine = "local"` unless `--machine-name` is given, and with GPU fields taken from `GpuSelection`.
- **REQ-PLAT.HW.CMD-007** The command shall exit 0 when all selected workloads are `ok`, 3 when any is `skipped` or `timeout`, and 4 when any is `failed`.

## Acceptance criteria

- **AC-1** Given fixture suite `fixtures/plat/bench/mini-suite.toml` (workloads `noop` and `spin_10ms`, 3 repetitions each) with its fixture datasets in a test cache, when `fid doctor run-benchmark --suite-fixture mini --json` runs in the test harness, then the output validates against `platform/bench-result.schema.json`, both workloads are `ok`, and `spin_10ms` median is between 10 ms and 50 ms.
- **AC-2** Given the mini suite with the `spin_10ms` dataset removed from the cache, when run, then `spin_10ms` is `skipped` with reason `dataset_not_cached`, the exit code is 3, and a network-deny test hook observes zero connection attempts.
- **AC-3** Given fixture workload `hang` with `timeout_s = 1`, when run, then it is reported `timeout` within 3 s and the next workload still runs.
- **AC-4** Given `--workload noop`, when run, then only `noop` appears in the result.
- **AC-5** Given any run, when it returns, then the scratch directory no longer exists and the project store is unchanged (digest before equals digest after).
- **AC-6** Given the MCP server, when an agent lists tools, then `doctor_run_benchmark` is present (R1 is exposed) and accepts only `workload` (list of IDs) and `machine_name` parameters, no file paths.

## Design

- **Registry command(s):** `doctor.run_benchmark@1`, class R1, params `workload: Option<Vec<WorkloadId>>`, `machine_name: Option<String>` (≤ 64 chars, `[A-Za-z0-9_-]`), `quick: bool` (1 repetition); result `BenchResult`. CLI `fid doctor run-benchmark [--workload <id>]... [--machine-name <n>] [--quick] [--json]`; MCP `doctor_run_benchmark`. `--suite-fixture` exists only in test builds.
- **Data structures:** `trait Workload { fn id(&self) -> &str; fn prepare(&mut self, dataset: &Path, scratch: &Path) -> Result<(), WorkloadError>; fn run_once(&mut self) -> Result<Vec<(MetricName, f64)>, WorkloadError>; }`; registry of workloads by ID populated by `PLAT.HW.SUITE`.
- **Algorithm:** load the installed manifest from the install directory; for each selected workload check the dataset in the ECO.DATA cache by digest; `prepare`; call `run_once` `repetitions` times on a worker thread with a deadline; compute median, min, max per metric; fill context (OS, arch, CPU, RAM from the same probes as `PLAT.DOCTOR.PROBE` if available, GPU from `fid-gpu-select`); emit result.
- **Errors:** `SuiteMissing`, `SuiteInvalid`, `UnknownWorkload { id }`; per-workload errors become statuses, not command errors.
- **Determinism:** N (timings are observed); the result's structure and field order are deterministic.
- **Journal:** one `doctor.run_benchmark@1` event per run with the result digest, following CORE's rule for R1 commands.
- **Performance budget:** runner overhead < 1 % of workload time [unverified].
- **Crate:** `fid-bench` (depends on `fid-platform`, `fid-gpu-select`; registers the command with the CORE registry).

## Test fixtures

`fixtures/plat/bench/mini-suite.toml` and tiny synthetic dataset files (a few bytes each, digests pinned) created for the test; test-only workloads `noop`, `spin_10ms` and `hang`. Licensed with the code.

## Build prompt

> Create crate `fid-bench` that registers registry command `doctor.run_benchmark@1` (side-effect class R1; CLI `fid doctor run-benchmark`, MCP `doctor_run_benchmark`, generated by AUTO from the registry; never hand-written). Parameters: optional list of workload IDs, optional machine name (≤ 64 chars, `[A-Za-z0-9_-]`), `quick` flag. No file-path parameters.
>
> The runner loads the installed suite manifest (`fid-platform::bench_schema`), checks each selected workload's dataset in the local ECO.DATA cache by SHA-256 (never fetch; missing or mismatched → `skipped`), runs `repetitions` times on a worker thread with the workload's timeout (exceeded → `timeout`, continue), computes median/min/max, fills the context block (OS, arch, CPU, RAM, GPU from `fid-gpu-select::GpuSelection`, `machine = "local"` by default) and returns a `BenchResult`. Human output is a table; `--json` prints the result document. Exit codes: 0 all ok, 3 any skipped or timeout, 4 any failed. Scratch files go in a temp dir removed before return; no project, record or configuration writes.
>
> Define the `Workload` trait for suite workloads; implement test-only workloads `noop`, `spin_10ms`, `hang`. Acceptance tests: the six criteria above. Constraints: CONSTITUTION §3 (R1, MCP exposed, agents get no raw paths), §8 (no panic, no `unwrap`, no `unsafe`), C7 (no network). Commit: `feat(fid-bench): fid doctor run-benchmark`.

## Open questions

- Long runs may need to be CORE.JOB jobs with progress and cancel once CORE.JOB is specified; until then the command is synchronous.

## Children

_Leaf: none._
