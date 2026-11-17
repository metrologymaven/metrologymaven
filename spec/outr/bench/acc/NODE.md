---
id: OUTR.BENCH.ACC
title: Accuracy benchmarks against reference data
level: 3
parent: OUTR.BENCH
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-001, REQ-OUTR.BENCH-003]
covers: []
depends_on: [OUTR.BENCH.RUN, UNC.RDL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BENCH.ACC: Accuracy benchmarks against reference data

## Purpose

Published accuracy results: how far the platform's fits and evaluations deviate from reference values in published reference datasets (for example NIST and PTB reference data for feature fitting [unverified dataset availability]). It turns UNC's internal validation into public, reproducible numbers.

## Scope

**In:**
- Selection of reference datasets that may be redistributed or referenced
- Accuracy recipes for fits and evaluations
- Reported deviations, reference source citation and UEC status per value

**Out** (and which node owns it instead):
- The reference data library and kernel validation (owned by `UNC.RDL`)
- Running and publishing (owned by `OUTR.BENCH.RUN`)

## Requirements

- **REQ-OUTR.BENCH.ACC-001** Each accuracy result shall report the deviation of each computed value from its reference value, with units.
- **REQ-OUTR.BENCH.ACC-002** Each accuracy result shall cite its reference source by name, edition or version, and dataset identifier, and shall never reproduce standards text.
- **REQ-OUTR.BENCH.ACC-003** Each reported value shall state its uncertainty status as a UEC level, and UEC-0 values shall be labelled "not evaluated".
- **REQ-OUTR.BENCH.ACC-004** Accuracy recipes shall use only reference datasets whose licence allows the project to reference or redistribute them, with the licence recorded in the recipe.
- **REQ-OUTR.BENCH.ACC-005** Each accuracy recipe shall follow the OUTR.BENCH.RUN recipe format and link a reproducing journal.

## Interfaces

- **Offers:** accuracy recipes.
- **Consumes:** UNC.RDL reference datasets and validation results; OUTR.BENCH.RUN.

## Decomposition hints

FIT (feature-fit accuracy), EVAL (GD&T evaluation accuracy, once UNC.ORS reference suites exist).

## Open questions

- Which reference datasets allow redistribution is for UNC.RDL to record [unverified].

## Children

_None yet: status `open`._
