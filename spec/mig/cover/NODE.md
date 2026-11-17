---
id: MIG.COVER
title: Migration test corpora and generated coverage tables
level: 2
parent: MIG
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG-005]
covers: []
depends_on: [ECO.DATA.CAT, ECO.PARITY.DRIFT, IO.DMIS.CONF, MIG.TEMPL, MIG.PROJ, MIG.PROGEQ]
cli: [fid migrate generate-coverage]
mcp: [migrate_generate_coverage]
journal: []
updated: 2026-11-16
---

# MIG.COVER: Migration test corpora and generated coverage tables

## Purpose

Every conversion path in MIG (incumbent project exports, customer forms, DMIS and later incumbent programs) must publish an honest coverage table: which constructs are supported, partial or unsupported, counted over a real test corpus. This node owns the per-format migration corpora and the generator that turns the per-item reports of `MIG.PROJ`, `MIG.TEMPL` and `MIG.PROGEQ` into published tables. Tables are generated, never hand-edited (CONSTITUTION §8, honest status).

## Scope

**In:**
- Migration corpora per format: admission into the dataset catalogue with provenance and licence, and a construct inventory per corpus
- Construct taxonomy per format (e.g. DMIS statement types, export record types, form features)
- The coverage generator: aggregate mapping, fill and equivalence reports into `SUPPORTED`, `PARTIAL`, `UNSUPPORTED` per construct, with counts and corpus digests
- Drift check: committed tables must equal a fresh generation
- `migrate.generate_coverage@1` (R1; the release build writes pages through the docs pipeline)

**Out** (and which node owns it instead):
- DMIS parser conformance tables (owned by `IO.DMIS.CONF`; consumed here, not duplicated)
- Producing per-item reports (owned by `MIG.PROJ`, `MIG.TEMPL`, `MIG.PROGEQ`)
- Dataset hosting and fetch (owned by `ECO.DATA`)
- Correlation statistics (owned by `MIG.STUDY`)

## Requirements

- **REQ-MIG.COVER-001** For every format that a MIG import or conversion command accepts, the project shall publish a coverage table listing each construct of that format's taxonomy as `SUPPORTED`, `PARTIAL` or `UNSUPPORTED`, with the number of corpus occurrences behind each status.
- **REQ-MIG.COVER-002** A construct shall be listed as `SUPPORTED` only if every corpus occurrence was `MAPPED` and, for programs, every corpus program containing it reached an `EQUIVALENT` verdict.
- **REQ-MIG.COVER-003** Each coverage table shall state the release, the corpus name and digest, and the generator version.
- **REQ-MIG.COVER-004** If a committed coverage table differs from a fresh generation from the same corpus and release, then the build shall fail and name the table.
- **REQ-MIG.COVER-005** Every corpus file shall be admitted to the dataset catalogue with source, licence and permission to redistribute, and files without redistribution permission shall be counted only in a private run whose table is marked as such.
- **REQ-MIG.COVER-006** A construct with zero corpus occurrences shall be listed as `UNTESTED`, never as `SUPPORTED`.

## Interfaces

- **Offers:** `migrate.generate_coverage@1` (R1); generated coverage tables consumed by `MIG.LADDER` assessments, `OUTR.CONTENT.MIG` guides and `ECO.PARITY`.
- **Consumes:** `ECO.DATA.CAT`, `ECO.PARITY.DRIFT`, `IO.DMIS.CONF`, reports from `MIG.TEMPL`, `MIG.PROJ` and `MIG.PROGEQ`.

## Decomposition hints

CORPUS (corpus admission and construct inventory), TAXO (construct taxonomies per format), GEN (generator and tables), DRIFT (regeneration check).

## Open questions

- REQ-MIG-005 lists supported, partial and unsupported; `UNTESTED` is added here so that zero-occurrence constructs are not misreported. A reviewer should confirm.

## Children

_None yet: status `open`._
