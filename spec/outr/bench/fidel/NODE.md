---
id: OUTR.BENCH.FIDEL
title: Format fidelity benchmarks
level: 3
parent: OUTR.BENCH
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-001]
covers: []
depends_on: [OUTR.BENCH.RUN, IO.CORP]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BENCH.FIDEL: Format fidelity benchmarks

## Purpose

Published round-trip fidelity numbers for each supported format (QIF, mesh, point cloud, CAD, DMIS and others as they land): the share of corpus files that read, round-trip and compare equal within stated rules. It makes the clean-room format work measurable, following pattern 13 of the design lessons.

## Scope

**In:**
- Fidelity recipes per format using IO corpora
- Definitions of "read", "round-trip" and "equal" per format, stated in the recipe
- Published fidelity numbers per format and release

**Out** (and which node owns it instead):
- Corpora and fuzzing (owned by `IO.CORP`)
- Running and publishing (owned by `OUTR.BENCH.RUN`)

## Requirements

- **REQ-OUTR.BENCH.FIDEL-001** For each supported format, a fidelity result shall report the number of corpus files read without error, the number that round-trip, and the number that compare equal, each against the corpus total.
- **REQ-OUTR.BENCH.FIDEL-002** Each fidelity recipe shall state its comparison rule (for example tolerance for coordinates and which entities are compared) in our own words.
- **REQ-OUTR.BENCH.FIDEL-003** Each fidelity result shall name the corpus, its version and its digest, and list every failing file by identifier.
- **REQ-OUTR.BENCH.FIDEL-004** Each fidelity recipe shall follow the OUTR.BENCH.RUN recipe format and link a reproducing journal.

## Interfaces

- **Offers:** fidelity recipes per format.
- **Consumes:** IO.CORP corpora and comparison tools; OUTR.BENCH.RUN.

## Decomposition hints

One child per format family once IO splits: QIF, MESH, CAD, DMIS.

## Open questions

- Corpus files under restrictive licences may be usable for testing but not for public listing of failures; IO.CORP records this.

## Children

_None yet: status `open`._
