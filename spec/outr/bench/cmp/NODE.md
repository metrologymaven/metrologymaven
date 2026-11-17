---
id: OUTR.BENCH.CMP
title: Comparisons with incumbent products
level: 3
parent: OUTR.BENCH
kind: module
status: decision
decision: D-021
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-005]
covers: []
depends_on: [OUTR.BENCH.RUN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BENCH.CMP: Comparisons with incumbent products

## Purpose

Benchmark results that place the platform next to a named incumbent product on the same data. They are the strongest claims the project could make and the most exposed legally, so they are isolated here and blocked on D-021 until maintainers decide whether and how to publish them.

## Scope

**In:**
- Rules for obtaining incumbent results (version, settings, who ran it, licence basis)
- Comparison recipes on shared datasets
- Comparison pages that OUTR.BRAND.LINT accepts as evidence for CL002

**Out** (and which node owns it instead):
- Our own half of each result (owned by `OUTR.BENCH.ACC`, `OUTR.BENCH.FIDEL`, `OUTR.BENCH.PERF`)
- Running and publishing (owned by `OUTR.BENCH.RUN`)

## Requirements

- **REQ-OUTR.BENCH.CMP-001** If a result compares the platform with an incumbent product, then the page shall state the incumbent's product name, version, settings and how and by whom the result was obtained.
- **REQ-OUTR.BENCH.CMP-002** Each comparison shall be reproducible by a third party who holds a licence for the incumbent product, using the published datasets and settings.
- **REQ-OUTR.BENCH.CMP-003** No comparison shall be published until D-021 is decided and its conditions (for example licence review) are met for that incumbent.

## Interfaces

- **Offers:** comparison pages (evidence links for OUTR.BRAND.LINT CL002).
- **Consumes:** OUTR.BENCH.RUN; D-021.

## Decomposition hints

After D-021: SOURCE (how incumbent results are obtained), RECIPE (shared-dataset recipes).

## Open questions

- Blocked on D-021. Incumbent licence terms may forbid publishing benchmark comparisons [unverified] (carried from OUTR.BENCH).

## Children

_None yet: status `decision`._
