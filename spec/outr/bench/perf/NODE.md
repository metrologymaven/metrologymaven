---
id: OUTR.BENCH.PERF
title: Performance benchmarks
level: 3
parent: OUTR.BENCH
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-001, REQ-OUTR.BENCH-004]
covers: []
depends_on: [OUTR.BENCH.RUN, PLAT.HW]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BENCH.PERF: Performance benchmarks

## Purpose

Published processing and rendering performance on stated reference machines: time to import, align, compare and evaluate representative datasets, and frame times of the viewer. It is separate because performance numbers depend on hardware and must be reported with spread, not as single best runs.

## Scope

**In:**
- Performance recipes for processing and rendering
- Run-count and statistics rules (median and spread)
- Reference machine profile per result

**Out** (and which node owns it instead):
- Reference machine profiles and release benchmarks used for internal gates (owned by `PLAT.HW`)
- Running and publishing (owned by `OUTR.BENCH.RUN`)

## Requirements

- **REQ-OUTR.BENCH.PERF-001** Each performance result shall report the median and the minimum-to-maximum spread over at least 5 runs [unverified run count].
- **REQ-OUTR.BENCH.PERF-002** Each performance result shall name the PLAT.HW reference machine profile it ran on, and results from different profiles shall never be shown as one series.
- **REQ-OUTR.BENCH.PERF-003** Rendering results shall state the GPU backend and resolution, and shall be reported separately from computation results.
- **REQ-OUTR.BENCH.PERF-004** Each performance recipe shall follow the OUTR.BENCH.RUN recipe format and link a reproducing journal.

## Interfaces

- **Offers:** performance recipes.
- **Consumes:** PLAT.HW reference machine profiles; OUTR.BENCH.RUN.

## Decomposition hints

PROC (processing), RENDER (viewer frame times).

## Open questions

- PLAT.HW may already run release benchmarks for internal gates; this node should publish those results rather than duplicate the runs.

## Children

_None yet: status `open`._
