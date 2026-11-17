---
id: PLAT.HW.SUITE.DATA
title: Suite manifest content, pinned datasets and time budget
level: 4
parent: PLAT.HW.SUITE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW.SUITE-001, REQ-PLAT.HW.SUITE-002, REQ-PLAT.HW.SUITE-005]
covers: []
depends_on: [PLAT.HW.SCHEMA]
uses_artifacts: [ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.SUITE.DATA: Suite manifest content

## Purpose

Writes the shipped suite manifest: the six workloads, the catalogue dataset each one uses pinned by SHA-256, their metrics with units and `better` directions, repetitions and timeouts. A check proves every dataset is in the ECO.DATA catalogue with a matching digest and redistributable licence, and that the worst-case run fits the 15-minute budget.

## Scope

**In:**
- `platform/bench-suite.toml` content
- `xtask plat-hw check-suite`: catalogue cross-check and time-budget check
- Dataset choices (synthetic generated datasets where no public dataset fits, published through the catalogue)

**Out** (and which node owns it instead):
- The manifest schema (`PLAT.HW.SCHEMA`)
- Dataset hosting and admission (`ECO.DATA.CAT`)
- Workload code (`PLAT.HW.SUITE.COMPUTE`, `PLAT.HW.SUITE.RENDER`)

## Requirements

- **REQ-PLAT.HW.SUITE.DATA-001** The manifest shall list workloads `load_10m` (metrics `load_s` s lower, `peak_mem` MiB lower), `render_10m` (`fps` higher), `render_software_1m` (`fps` higher), `fit_1000` (`fit_s` s lower), `compare_1m` (`compare_s` s lower, `points_per_s` higher) and `report_20p` (`report_s` s lower).
- **REQ-PLAT.HW.SUITE.DATA-002** If a workload's dataset ID is not in the catalogue, its digest differs from the catalogue's, or its catalogue licence is not redistributable, then the check shall fail naming the workload.
- **REQ-PLAT.HW.SUITE.DATA-003** The sum over workloads of `repetitions × timeout_s` shall be at most 900 seconds, and the check shall fail otherwise [unverified: budget].
- **REQ-PLAT.HW.SUITE.DATA-004** Any change to a pinned digest shall bump `suite_version`, and the check shall fail if digests changed against the previous release's manifest without a version bump.

## Acceptance criteria

- **AC-1** Given the shipped manifest and the catalogue, when `cargo xtask plat-hw check-suite` runs, then it exits 0 and prints the budget used.
- **AC-2** (error path) Given `fixtures/plat/bench/suite-not-in-catalogue.toml`, then `NotInCatalogue { workload }`.
- **AC-3** (error path) Given a manifest whose timeouts sum to 1 200 s, then `OverBudget { total_s: 1200 }`.
- **AC-4** (error path) Given a changed digest with the same `suite_version`, then `DigestChangedWithoutBump`.

## Design

- **Registry command(s):** none; data and `xtask plat-hw check-suite --catalogue <file> [--previous <manifest>]`.
- **Initial proposal [unverified until first runs]:** repetitions 3 each; timeouts load 60 s, render_10m 30 s, render_software_1m 30 s, fit 30 s, compare 60 s, report 60 s (total 810 s). Datasets: a synthetic 10 M-point cloud and 1 M-point scan with a matching CAD part from a fixed-seed generator published in the catalogue; a 1 000-feature synthetic part; a report template project from the P1a examples.
- **Errors:** `NotInCatalogue`, `DigestMismatch`, `NotRedistributable`, `OverBudget`, `DigestChangedWithoutBump`.
- **Determinism:** D0.
- **Crate:** `xtask` (module `plat_hw_suite`), using `fid-platform::bench_schema`.

## Test fixtures

Fixture manifests and a fixture catalogue under `fixtures/plat/bench/`. Licensed with the code.

## Build prompt

> Write `platform/bench-suite.toml` (PLAT.HW.SCHEMA format) with workloads load_10m, render_10m, render_software_1m, fit_1000, compare_1m, report_20p, their metrics (units and better directions as in REQ-001), 3 repetitions and the proposal timeouts, each pinned to a catalogue dataset by SHA-256. Add `xtask plat-hw check-suite --catalogue <file> [--previous <manifest>]`: every dataset in the catalogue with matching digest and redistributable licence, Σ repetitions × timeout_s ≤ 900, and digest changes require a `suite_version` bump. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8. Commit: `feat(platform): benchmark suite manifest`.

## Open questions

- Synthetic datasets must first be admitted to the catalogue through `ECO.DATA.CAT` (cross-domain step).

## Children

_Leaf: none._
