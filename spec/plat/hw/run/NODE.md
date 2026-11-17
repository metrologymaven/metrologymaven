---
id: PLAT.HW.RUN
title: Release benchmark runs, publishing and regression flags
level: 3
parent: PLAT.HW
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-003, REQ-PLAT.HW-005]
covers: []
depends_on: [PLAT.HW.SCHEMA, PLAT.HW.REFMACH]
uses_artifacts: [ECO.DATA.BENCH.RUN, ECO.DATA.BENCH.BUNDLE, PLAT.HW.CMD, PLAT.HW.SUITE, ECO.REL.PROC.FLOW, ECO.REL.PROC.NOTES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.RUN: Release benchmark runs

## Purpose

Runs the benchmark suite on every reference machine for every release, publishes the results with the release, and flags regressions against the previous release in the release notes. It turns the profiles into a measured, release-by-release promise.

## Scope

**In:**
- Triggering `fid doctor run-benchmark` on each reference machine from the release pipeline
- Collecting and publishing the result documents with the release
- Comparing each metric with the previous release's result on the same machine
- Writing regression flags into the release notes input

**Out** (and which node owns it instead):
- The machines (`PLAT.HW.REFMACH`)
- The suite and runner (`PLAT.HW.SUITE`, `PLAT.HW.CMD`); provenance capture and history (`ECO.DATA.BENCH.RUN`)
- The release pipeline and notes publishing (`ECO.REL`)

## Requirements

- **REQ-PLAT.HW.RUN-001** When a release is built, the pipeline shall run the full suite on every reference machine in the registry and attach each result document to the release.
- **REQ-PLAT.HW.RUN-002** If a reference machine's run is missing or failed, then the release notes shall say so and name the machine.
- **REQ-PLAT.HW.RUN-003** When results are published, the comparison tool shall flag every metric whose median regressed by more than 10 % against the previous release on the same machine [unverified: threshold to be tuned].
- **REQ-PLAT.HW.RUN-004** If the registry records a comparability break for a machine, then the comparison for that machine shall be reported as `not_comparable` instead of a regression.
- **REQ-PLAT.HW.RUN-005** The comparison tool shall be a pure function of two result documents and produce byte-identical output for the same inputs.

## Interfaces

- **Offers:** `bench-regressions.json` v1 per release (read by `ECO.REL.PROC.NOTES` as release-notes input); job entries `bench-<machine>` in the release pipeline.
- **Consumes:** `PLAT.HW.SCHEMA` result types; `PLAT.HW.REFMACH` registry and `is_comparable`. Artifacts: `cargo xtask bench run` and its history records (`ECO.DATA.BENCH.RUN`), the release-numbers bundle (`ECO.DATA.BENCH.BUNDLE`), the `fid doctor run-benchmark` binary (`PLAT.HW.CMD`), the suite manifest (`PLAT.HW.SUITE`), the release workflow (`ECO.REL.PROC.FLOW`).

## Decomposition hints

See Children.

## Open questions

- D-014 is accepted: the minimum-profile machine is maintainer-owned (self-hosted runner), the recommended profile is rented per release. `PLAT.HW.RUN.PIPE` covers both.
- Resolved boundary with `ECO.DATA.BENCH`: release runs invoke `cargo xtask bench run` so published numbers carry its provenance; PLAT.HW.RUN adds only dispatch, missing-run reporting and regression judgement.
- `ECO.DATA.BENCH.RUN` keys history by machine profile ID, not reference machine name; with two machines per profile the records collide into suffixed files. It should pass `--machine-name` through to `fid doctor run-benchmark` and key by machine (cross-domain).

## Split rationale

Split into a **pure judgement** (COMPARE: two result documents plus the registry in, regression report out) and **orchestration** (PIPE: dispatching runs to machines, collecting results, reporting missing runs). The pure part is fully testable from fixtures; the orchestration part depends on runners and rentals.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.HW.RUN.COMPARE | Regression comparison against the previous release | 003 004 005 | none |
| PLAT.HW.RUN.PIPE | Release-time dispatch to reference machines and missing-run report | 001 002 | none |
