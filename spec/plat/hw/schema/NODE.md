---
id: PLAT.HW.SCHEMA
title: Benchmark suite manifest and result schema
level: 3
parent: PLAT.HW
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-004]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.HW.SCHEMA: Benchmark manifest and result schema

## Purpose

Defines the two formats every benchmark tool shares: the suite manifest (which workloads, on which pinned datasets, measuring which metrics) and the result document (values plus the full context needed to compare them: dataset digests, app version, OS, backend, driver). Reference runs and local `fid doctor run-benchmark` runs emit the same result document, so a user's number can be compared directly with a published one.

## Scope

**In:**
- Suite manifest schema (TOML): workload IDs, dataset references with SHA-256 digests, metrics with units, repetitions, timeouts
- Result schema (JSON, versioned): context block and per-metric statistics
- Parsers, validators and a JSON Schema export for the result

**Out** (and which node owns it instead):
- Which workloads and datasets the suite contains (`PLAT.HW.SUITE`)
- Running the suite (`PLAT.HW.CMD`, `PLAT.HW.RUN`)
- Dataset fetching and caching (`ECO.DATA`)

## Requirements

- **REQ-PLAT.HW.SCHEMA-001** The suite manifest schema shall define workloads with `id`, `dataset` (dataset ID and SHA-256 digest), `metrics` (name, unit and `better`: `lower` or `higher`), `repetitions` (1–100) and `timeout_s`.
- **REQ-PLAT.HW.SCHEMA-002** The result schema shall record, once per result document, the suite version, app version, OS and version, architecture, CPU model, RAM, GPU backend, adapter name, GPU driver version and machine name (`local` for user runs).
- **REQ-PLAT.HW.SCHEMA-003** The result schema shall record, per workload and metric, the dataset digest, unit, repetitions, median, minimum, maximum and status (`ok`, `timeout`, `failed`, `skipped`).
- **REQ-PLAT.HW.SCHEMA-004** If a manifest workload lacks a dataset digest or names a metric unit outside the allowed set (`s`, `ms`, `fps`, `MiB`, `points_per_s`, `triangles_per_s`), then validation shall fail naming the workload.
- **REQ-PLAT.HW.SCHEMA-005** The crate shall export a JSON Schema for the result document, and a result emitted by any tool shall validate against it.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/bench/mini-suite.toml` (2 workloads), when `cargo xtask plat-hw validate-suite` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/plat/bench/suite-no-digest.toml`, when validated, then it exits 1 with `MissingDigest { workload }`.
- **AC-3** (error path) Given `fixtures/plat/bench/suite-bad-unit.toml` with unit `furlongs`, when validated, then it exits 1 with `BadUnit`.
- **AC-4** Given `fixtures/plat/bench/result-sample.json`, when validated against the exported JSON Schema, then it is valid; removing `context.gpu_driver_version` makes it invalid.
- **AC-5** (determinism) Given a `Result` value built in a test, when serialised twice, then the outputs are byte-identical (keys in a fixed order).

## Design

- **Registry command(s):** none; library shared by `PLAT.HW.CMD` and `PLAT.HW.RUN`.
- **Data structures:** `Suite { schema_version, suite_version, workloads: Vec<Workload> }`; `Workload { id, dataset: DatasetRef { id, sha256 }, metrics: Vec<Metric { name, unit, better: lower | higher }>, repetitions: u8, timeout_s: u32 }`; `BenchResult { schema_version: 1, context: Context, workloads: Vec<WorkloadResult> }`; `MetricResult { name, unit, dataset_sha256, repetitions, median, min, max, status }`.
- **Algorithm:** serde with `deny_unknown_fields`; field order fixed by struct order; JSON Schema generated from the types.
- **Errors:** `Parse`, `UnsupportedSchema`, `MissingDigest`, `BadUnit`, `BadRepetitions`, `DuplicateWorkload`.
- **Determinism:** D0 for serialisation; measured values themselves are observed (`N`).
- **Performance budget:** trivial.
- **Crate:** `fid-platform` (module `bench_schema`).

## Test fixtures

Hand-written files under `fixtures/plat/bench/` with dummy dataset digests; licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `bench_schema` and `xtask plat-hw validate-suite [--file <path>]`. Define the suite manifest (TOML) and the benchmark result document (JSON, `schema_version` 1) exactly as in the Design above, with `deny_unknown_fields`, a fixed field order and a JSON Schema export (`xtask plat-hw result-schema` writes it to `platform/bench-result.schema.json`). Allowed units: s, ms, fps, MiB, points_per_s, triangles_per_s. Each metric declares `better = "lower" | "higher"` so regressions can be judged. Repetitions 1–100.
>
> Acceptance tests: the five criteria above with fixtures in `fixtures/plat/bench/`. Constraints: CONSTITUTION §8; no hash-ordered maps in serialised output. Commit: `feat(fid-platform): benchmark manifest and result schema`.

## Open questions

- None.

## Children

_Leaf: none._
