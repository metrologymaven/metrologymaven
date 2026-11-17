---
id: OUTR.BENCH.PERF
title: Performance benchmarks
level: 3
parent: OUTR.BENCH
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-001, REQ-OUTR.BENCH-004]
covers: []
depends_on: [OUTR.BENCH.RUN]
uses_artifacts: [PLAT.HW.SCHEMA, PLAT.HW.REFMACH, PLAT.HW.RUN, ECO.DATA.BENCH]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.PERF: Performance benchmarks

## Purpose

Published processing and rendering performance on stated reference machines: time to import, align, compare and evaluate representative datasets, and viewer frame times. Performance numbers depend on hardware, so they are reported per reference-machine profile, with median and spread, never as a single best run. The runs themselves are the release benchmark runs PLAT.HW already makes; this node publishes them and refuses those that break the reporting rules.

## Scope

**In:**
- The `[performance]` recipe section, its validator and its result fragment
- One recipe per PLAT.HW suite workload and metric that the project chooses to publish
- Run-count and statistics rules (median and minimum-to-maximum spread)
- Separation of profiles and of computation and rendering results; disclosure of each machine's host

**Out** (and which node owns it instead):
- Reference machine profiles and registry (owned by `PLAT.HW.PROFILE`, `PLAT.HW.REFMACH`)
- The benchmark suite, its runs and the result document format (owned by `PLAT.HW.SUITE`, `PLAT.HW.RUN`, `PLAT.HW.SCHEMA`)
- The common recipe format, harness and pages (owned by `OUTR.BENCH.RUN`)

## Requirements

- **REQ-OUTR.BENCH.PERF-001** Each performance fragment shall report the median and the minimum and maximum of the measured metric with its unit and the number of repetitions.
- **REQ-OUTR.BENCH.PERF-002** If a result has fewer than 5 repetitions [unverified run count], then the validator of the result shall mark it `not_publishable` with reason `too_few_runs` and the fragment shall show no number.
- **REQ-OUTR.BENCH.PERF-003** Each performance recipe shall set `machine_profile` to `minimum` or `recommended`, and the fragment shall name the reference machine ID, its profile and who hosts it, as listed in the PLAT.HW.REFMACH registry (D-014).
- **REQ-OUTR.BENCH.PERF-004** Results from different machine profiles shall be published under different recipe IDs, and the validator shall reject a recipe whose `[performance]` section lists more than one profile.
- **REQ-OUTR.BENCH.PERF-005** The `[performance]` section shall state `category` as `compute` or `render`; render recipes shall also name the GPU backend and the resolution, and the fragment shall show them.
- **REQ-OUTR.BENCH.PERF-006** The performance index on the result pages shall list compute and render recipes in separate tables.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/bench/perf/ok/` (a release-numbers bundle with a PLAT.HW result document: workload `align-bestfit-1m`, 7 repetitions, median 412 ms, min 401 ms, max 433 ms, on machine `ref-min-1` hosted by "Example Lab"), when `fid-outreach bench-perf fragment --recipe perf-align.toml --bundle release-numbers.json --machines machines.toml` runs, then the fragment shows `412 ms`, `401 to 433 ms`, `7 runs`, `ref-min-1`, `minimum` and `Example Lab`.
- **AC-2** Given `few-runs/` (3 repetitions), when the fragment is written, then it shows `not publishable: too_few_runs` and no number.
- **AC-3** (error path) Given `any-profile.toml` (`machine_profile = "any"`), when `fid-outreach bench-recipe check` runs with this validator registered, then it exits 1 with `profile_required`.
- **AC-4** Given `render/` (category `render`, backend `vulkan`, resolution `2560x1440`), when the fragment is written, then backend and resolution appear.
- **AC-5** (error path) Given `render-no-backend.toml` (category `render` with no backend), when the check runs, then it exits 1 with `render_context_missing`.
- **AC-6** Given a store with one compute and one render recipe, when `fid-outreach bench-pages` builds the index with this module registered, then `bench/index.md` shows them in separate compute and render tables.

## Design

- **Command:** `fid-outreach bench-perf fragment --recipe <file> --bundle <file> --machines <file> [--out <file>]`, also reached through the harness fragment hook; kind validator `performance` registered with `bench-recipe check`; an index section provider registered with `bench-pages`. Project tooling; nearest class R0.
- **Schema `[performance]`:** `workload` (PLAT.HW suite workload ID), `metric`, `category` (`compute` or `render`), `gpu_backend` and `resolution` (required for `render`), `min_repetitions` (default 5).
- **Data read:** PLAT.HW `BenchResult` documents in the bundle (`MetricResult { name, unit, repetitions, median, min, max, status }` and the run context); the machine registry for host and profile.
- **Algorithm:** select the workload and metric; check repetitions against `min_repetitions`; copy median, min and max as given (no re-computation); render a fragment row and the machine line.
- **Errors:** `profile_required`, `render_context_missing`, `workload_missing`, `machine_unknown`.
- **Determinism:** D0 (the measured numbers are observed, class N, upstream).
- **Crate:** `fid-outreach`, module `bench::performance`; recipes under `bench/recipes/performance/`.

## Test fixtures

Synthetic bundles shaped like PLAT.HW result documents and a machine registry fixture under `fixtures/outr/bench/perf/` (`ok`, `few-runs`, `any-profile.toml`, `render`, `render-no-backend.toml`), hand-written, CC-BY-4.0; the host name is fictional.

## Build prompt

> In the Rust crate `fid-outreach`, add module `bench::performance`: a `KindValidator` for kind `performance` registered with `bench::recipe`; a fragment writer `fid-outreach bench-perf fragment --recipe <file> --bundle <file> --machines <file> [--out <file>]` exposed to `bench::harness`; and an index provider for `bench::pages` that splits performance recipes into compute and render tables. Add recipes under `bench/recipes/performance/`.
>
> `[performance]` has `workload`, `metric`, `category` (`compute` or `render`), `gpu_backend` and `resolution` (both required for `render`: `render_context_missing`), `min_repetitions` (default 5). The recipe's `machine_profile` must be `minimum` or `recommended` (`profile_required`), and one recipe covers exactly one profile.
>
> The fragment reads the PLAT.HW benchmark result document from the release-numbers bundle (metric median, min, max, unit, repetitions, run context) and the machine registry (machine ID, profile, host). Show median, `min to max`, repetitions, machine ID, profile and host; for render also backend and resolution. With fewer repetitions than `min_repetitions`, show `not publishable: too_few_runs` and no number. Copy numbers as given; never recompute statistics.
>
> Tests with fixtures under `fixtures/outr/bench/perf/` for: ok (412 ms, 401 to 433 ms, 7 runs, host shown), few runs, `machine_profile = "any"` rejected, render with backend and resolution, render without backend rejected, and separate compute and render index tables.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; BTreeMap rather than HashMap; no network; allowlisted dependency licences only.

## Open questions

- The decomposition hint (PROC, RENDER) is not needed: both use the same mechanics, separated by `category`.
- PLAT.HW runs the release benchmarks for internal gates; this node publishes those results and does not duplicate the runs (resolves the earlier open question).
- The run count of 5 is carried from REQ-OUTR.BENCH-004 [unverified]; PLAT.HW.SCHEMA's `repetitions` is a `u8`, so any count up to 255 fits.
- The former code dependency on PLAT.HW is an artifact read (result documents, machine registry) and is now `uses_artifacts` (CONSTITUTION 2.1, D-034).

## Children

_Leaf: none._
