---
id: PLAT.HW.RUN
title: Release benchmark runs, publishing and regression flags
level: 3
parent: PLAT.HW
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-003, REQ-PLAT.HW-005]
covers: []
depends_on: [PLAT.HW.REFMACH, PLAT.HW.SUITE, PLAT.HW.CMD, ECO.CI, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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
- The machines (`PLAT.HW.REFMACH`) and who hosts them (D-014)
- The suite and runner (`PLAT.HW.SUITE`, `PLAT.HW.CMD`)
- The release pipeline and notes publishing (`ECO.REL`)

## Requirements

- **REQ-PLAT.HW.RUN-001** When a release is built, the pipeline shall run the full suite on every reference machine in the registry and attach each result document to the release.
- **REQ-PLAT.HW.RUN-002** If a reference machine's run is missing or failed, then the release notes shall say so and name the machine.
- **REQ-PLAT.HW.RUN-003** When results are published, the comparison tool shall flag every metric whose median regressed by more than 10 % against the previous release on the same machine [unverified: threshold to be tuned].
- **REQ-PLAT.HW.RUN-004** If the registry records a comparability break for a machine, then the comparison for that machine shall be reported as `not_comparable` instead of a regression.
- **REQ-PLAT.HW.RUN-005** The comparison tool shall be a pure function of two result documents and produce byte-identical output for the same inputs.

## Interfaces

- **Offers:** published result documents per release; regression report for release notes.
- **Consumes:** `PLAT.HW.REFMACH` registry; `PLAT.HW.CMD` runner; ECO.REL pipeline; ECO.CI self-hosted runner hooks.

## Decomposition hints

COMPARE (pure regression comparison in `fid-platform`; leaf candidate now), PIPE (release pipeline integration; after D-014).

## Open questions

- Blocked in part on **D-014**; COMPARE can proceed independently.
- The direction of a regression comes from each metric's `better` field in the suite manifest (`PLAT.HW.SCHEMA`).

## Children

_None yet: status `open`._
