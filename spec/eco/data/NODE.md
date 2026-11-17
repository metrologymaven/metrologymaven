---
id: ECO.DATA
title: Benchmark datasets and example projects
level: 2
parent: ECO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO-008]
covers: []
depends_on: [ECO.REPO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.DATA: Benchmark datasets and example projects

## Purpose

The shared machinery for public datasets and example projects: a catalogue with licences and digests, a fetch-and-verify tool used by tests, benchmarks and CI, a benchmark harness that produces comparable performance numbers, and example projects with journals that docs and tutorials replay. Which datasets a domain needs, and the expected results for them, stay with that domain (for example IO for format round-trips, UNC and KRN for fit reference data).

## Scope

**In:**
- Dataset catalogue format: ID, source, licence, size, SHA-256, owning domain, redistribution status
- Fetch, cache and verify tool (offline once cached)
- Benchmark harness: named benchmarks, reference machine, result format, history across releases
- Example projects with recorded journals, versioned with releases
- Policy for adding datasets: licence check, no standards text, no customer data without written release
- Published fidelity and performance numbers per release, as data

**Out** (and which node owns it instead):
- Domain-specific reference data and expected results (owned by `IO`, `KRN`, `UNC`, `SCAN`, `IND` and other domains)
- Reference hardware profiles and machines (owned by `PLAT`)
- Publishing benchmarks and case studies to the public (owned by `OUTR`)
- Tutorials built on the example projects (owned by `OUTR`)

## Requirements

- **REQ-ECO.DATA-001** Every dataset in the catalogue shall record its source, SPDX licence or terms, size, SHA-256 digest and owning domain.
- **REQ-ECO.DATA-002** If a fetched dataset's digest does not match the catalogue, then the fetch tool shall reject it and leave the cache unchanged.
- **REQ-ECO.DATA-003** When a dataset is in the local cache, the fetch tool shall run fully offline.
- **REQ-ECO.DATA-004** A dataset shall not be added to the catalogue unless its licence permits redistribution or it is marked fetch-only from its original source.
- **REQ-ECO.DATA-005** The benchmark harness shall record, for every result, the benchmark ID, dataset digest, software revision and machine profile, so any contributor can reproduce it.
- **REQ-ECO.DATA-006** Each release shall publish example projects with journals that replay with outcome `IDENTICAL` or `EQUIVALENT` on that release.
- **REQ-ECO.DATA-007** Each release shall publish its format-fidelity and benchmark numbers as machine-readable data alongside the parity report.

## Interfaces

- **Offers:** dataset catalogue; `xtask`-style fetch tool (repo tooling, not a product registry command); benchmark harness and result schema; example projects.
- **Consumes:** domain dataset requests; CORE replay for example-project checks; PLAT reference machine profiles.

## Decomposition hints

CAT (catalogue and policy), FETCH (fetch, cache, verify), BENCH (harness and results), EX (example projects).

## Open questions

- Hosting for large datasets (tens of GB of scans) without a paid account is unresolved; mirrors or academic hosting may be needed [unverified].

## Children

_None yet: status `open`._
