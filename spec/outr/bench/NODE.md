---
id: OUTR.BENCH
title: Public benchmarks and case studies
level: 2
parent: OUTR
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-001, REQ-OUTR-007]
covers: []
depends_on: [ECO, UNC, OUTR.BRAND]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BENCH: Public benchmarks and case studies

## Purpose

Published numbers about the platform (fit accuracy against reference data, format round-trip fidelity, rendering and processing performance) and user case studies. It exists separately because these are the project's strongest public claims, so each one must be reproducible by anyone from published data and journals, and must never claim more than the measurement shows.

## Scope

**In:**
- Benchmark result pages: accuracy (against published reference datasets), fidelity (format round-trips), performance (rendering, processing time) on stated reference machines
- The benchmark run recipe: dataset, release, machine profile, journal, command to reproduce
- Comparisons with incumbents, only where measured and reproducible
- Case studies from users, with their written consent and verifiable results

**Out** (and which node owns it instead):
- Benchmark datasets and example corpora (owned by `ECO`)
- Reference test data and fit validation (owned by `UNC` / `KRN`)
- Reference hardware profiles (owned by `PLAT`)
- Claim lint rules (owned by `OUTR.BRAND`)
- Hosting of result pages (owned by `OUTR.SITE`)

## Requirements

- **REQ-OUTR.BENCH-001** Every published benchmark result shall state the release, dataset with digest, machine profile, command or journal used, and date, and shall link a downloadable journal that reproduces it.
- **REQ-OUTR.BENCH-002** When a new release is tagged, the benchmark suite shall be re-run and the result pages shall show the new results next to the previous release's results.
- **REQ-OUTR.BENCH-003** Accuracy benchmarks shall report deviations from the reference values with the reference source cited by name and edition, and shall state the uncertainty status (UEC level) of each reported value.
- **REQ-OUTR.BENCH-004** Performance results shall report the median and the spread over at least 5 runs [unverified run count] on a PLAT reference machine profile.
- **REQ-OUTR.BENCH-005** If a benchmark result compares the platform with an incumbent product, then the page shall state the incumbent's version, settings and how it was obtained, and the comparison shall be reproducible by a third party who has that product.
- **REQ-OUTR.BENCH-006** If a published result regresses by more than its stated tolerance between releases, then the result page shall show the regression instead of hiding the newer number.
- **REQ-OUTR.BENCH-007** A case study shall be published only with the named organisation's written consent on file, and every quantitative claim in it shall be backed by a result package verifiable with `fid-verify` or marked as the organisation's own statement.
- **REQ-OUTR.BENCH-008** Every benchmark and case-study page shall pass the OUTR.BRAND claim lint with zero errors.

## Interfaces

- **Offers:** benchmark result pages and case studies (hosted by OUTR.SITE); result links that OUTR.BRAND accepts as evidence for comparisons.
- **Consumes:** ECO benchmark datasets and releases, UNC/KRN reference data, PLAT reference machine profiles, TRUST `fid-verify`, OUTR.BRAND lint.

## Decomposition hints

ACC (accuracy benchmarks), FID (format fidelity), PERF (performance), CMP (incumbent comparisons), CASE (case studies).

## Open questions

- Incumbent licence terms may forbid publishing benchmark comparisons [unverified]; check each EULA before REQ-OUTR.BENCH-005 is used.
- ECO's REQ-ECO-008 publishes benchmark **datasets**; this node publishes **results**. A reviewer should confirm the boundary when ECO is split.

## Children

_None yet: status `open`._
