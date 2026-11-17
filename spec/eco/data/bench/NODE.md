---
id: ECO.DATA.BENCH
title: Benchmark harness, provenance and release numbers
level: 3
parent: ECO.DATA
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.DATA-005, REQ-ECO.DATA-007]
covers: []
depends_on: [ECO.DATA.FETCH]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.DATA.BENCH: Benchmark harness, provenance and release numbers

## Purpose

The developer-side harness that runs named benchmarks against catalogue datasets and records every result with enough provenance to reproduce it, plus the per-release bundle that publishes performance and format-fidelity numbers as machine-readable data beside the parity report. Which workloads make up the release benchmark suite, the reference machines and the regression thresholds belong to PLAT.HW; fidelity scoring belongs to IO.CORP.

## Scope

**In:**
- Harness: benchmark registration by ID, dataset binding through ECO.DATA.FETCH, warm-up and repetition policy, provenance capture
- Result storage and history across releases
- The release-numbers bundle: collects PLAT.HW benchmark results and domain fidelity reports into one versioned JSON document per release

**Out** (and which node owns it instead):
- Suite contents, reference machines, result schema fields for hardware and regression thresholds (owned by `PLAT.HW`)
- Fidelity scoring of format round-trips (owned by `IO.CORP`)
- Publishing and charting numbers on the website (owned by `OUTR`)

## Requirements

- **REQ-ECO.DATA.BENCH-001** Every benchmark result shall record the benchmark ID, the dataset ID and SHA-256, the software revision, the build profile, the machine profile ID and the harness version.
- **REQ-ECO.DATA.BENCH-002** If a benchmark's dataset is not in the catalogue or fails verification, then the harness shall not run it and shall record the benchmark as `not-run` with the reason.
- **REQ-ECO.DATA.BENCH-003** When the same benchmark is rerun on the same machine profile and revision, the harness shall report the run-to-run spread, and results shall state the number of repetitions.
- **REQ-ECO.DATA.BENCH-004** When a release is built, the release-numbers bundle shall include every benchmark result and every fidelity report produced for that release, each with its provenance, in one JSON document.
- **REQ-ECO.DATA.BENCH-005** The release-numbers bundle shall be generated, never hand-edited, and shall carry a "generated, do not edit" marker and its input digests.

## Interfaces

- **Offers:** harness API and runner; result store; `release-numbers.json` per release (consumed by ECO.PARITY, ECO.REL and OUTR).
- **Consumes:** ECO.DATA.FETCH; PLAT.HW suite definition, machine profiles and result schema; IO.CORP fidelity reports.

## Decomposition hints

HARN (runner and provenance), STORE (history), BUNDLE (release-numbers document).

## Open questions

- Boundary with PLAT.HW: PLAT.HW lists "the benchmark suite: workloads, datasets and metrics" and a result schema, while REQ-ECO-008 and REQ-ECO.DATA-005 give ECO a harness. This node takes the runner and provenance and defers suite content and schema fields to PLAT.HW; a reviewer of both domains should confirm (cross-domain).
- PLAT.HW.SUITE depends on ECO.DATA, so this node cannot declare a dependency on PLAT.HW without a cycle; PLAT.HW consumes the harness and this node reads PLAT.HW's suite definition and schema as data.
- Reference machine noise and repetition counts need measurement before thresholds are meaningful [unverified].

## Children

_None yet: status `open`._
