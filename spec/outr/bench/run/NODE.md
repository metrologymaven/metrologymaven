---
id: OUTR.BENCH.RUN
title: Benchmark recipes, release re-runs and result pages
level: 3
parent: OUTR.BENCH
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-001, REQ-OUTR.BENCH-002, REQ-OUTR.BENCH-006, REQ-OUTR.BENCH-008]
covers: []
depends_on: [OUTR.BRAND.LINT]
uses_artifacts: [ECO.REL, ECO.DATA, PLAT.HW]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BENCH.RUN: Benchmark recipes, release re-runs and result pages

## Purpose

The shared machinery under every benchmark: the recipe format that makes a result reproducible, the harness that re-runs every recipe when a release is tagged, the result-page generator that shows new results next to the previous release's, and the regression display. ACC, FIDEL and PERF write recipes; this node runs and publishes them.

## Scope

**In:**
- Recipe format: release, dataset ID and digest, machine profile, command or journal, expected output fields, tolerance
- Release re-run harness
- Result store with history across releases
- Result-page generator with previous-release comparison and regression display
- Claim lint on result pages

**Out** (and which node owns it instead):
- Choice of accuracy, fidelity and performance benchmarks (owned by `OUTR.BENCH.ACC`, `OUTR.BENCH.FIDEL`, `OUTR.BENCH.PERF`)
- Datasets (owned by `ECO.DATA`)
- Page hosting (owned by `OUTR.SITE`)

## Requirements

- **REQ-OUTR.BENCH.RUN-001** Each benchmark recipe shall state the release, dataset ID with SHA-256 digest, machine profile, the command or journal to run, the result fields, and a regression tolerance per field.
- **REQ-OUTR.BENCH.RUN-002** Every published result shall show its recipe fields and run date, and link a downloadable journal that reproduces it.
- **REQ-OUTR.BENCH.RUN-003** When a release is tagged, the harness shall run every recipe on that release and store the results with the previous releases' results.
- **REQ-OUTR.BENCH.RUN-004** Each result page shall show the current release's result next to the previous release's result for every field.
- **REQ-OUTR.BENCH.RUN-005** If a field regresses by more than its tolerance between consecutive releases, then the result page shall mark the regression visibly and the generator shall not omit or replace the newer value.
- **REQ-OUTR.BENCH.RUN-006** Every generated result page shall pass the OUTR.BRAND claim lint with zero errors.

## Interfaces

- **Offers:** recipe format; harness; result store; result pages (hosted by OUTR.SITE); stable result URLs used as evidence links by OUTR.BRAND.LINT (CL002 benchmark prefix).
- **Consumes:** ECO.REL releases; ECO.DATA datasets; PLAT.HW machine profiles; OUTR.BRAND.LINT.

## Decomposition hints

RECIPE (format and validation), HARNESS (release re-run), PAGES (generator and regression display).

## Open questions

- Where the harness runs (project CI or a donated reference machine) affects performance results; PLAT.HW owns reference machines. OUTR.BENCH.RUN.HARNESS runs `command` recipes only on a machine whose profile matches, and otherwise reuses the release-numbers bundle (ECO.DATA.BENCH, PLAT.HW.RUN).
- The former code dependencies on ECO.REL and ECO.DATA are artifact reads (release binaries, checksum file, dataset catalogue, release-numbers bundle) and are now `uses_artifacts` (CONSTITUTION 2.1, D-034).
- REQ-OUTR.BENCH.RUN-001 says a recipe states "the release"; the children read this as `since_release` in the recipe and the actual release in each result record.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.BENCH.RUN.RECIPE](recipe/NODE.md) | Benchmark recipe format and validator (leaf) | 001 | none |
| [OUTR.BENCH.RUN.HARNESS](harness/NODE.md) | Release re-run harness and result store (leaf) | 002 003 | none |
| [OUTR.BENCH.RUN.PAGES](pages/NODE.md) | Result-page generator with regression display (leaf) | 002 004 005 006 | none |
