---
id: RPT.CATALOG.CHECK
title: Catalog integrity check and rebuild
level: 3
parent: RPT.CATALOG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG-003, REQ-RPT.CATALOG-009]
covers: []
depends_on: [RPT.CATALOG.STORE, CORE.JRN]
cli: [fid stats check-catalog, fid stats rebuild-catalog]
mcp: [stats_check_catalog, stats_rebuild_catalog]
journal: [stats.rebuild_catalog@1]
updated: 2026-11-16
---

# RPT.CATALOG.CHECK: Catalog integrity check and rebuild

## Purpose

Proves that the catalog is only an index: the integrity check compares every entry with its source project's journal and marks mismatches `STALE`, and rebuild recreates the catalog from the registered projects with the same content digest. This keeps statistics from silently using results that no longer match their record.

## Scope

**In:**
- Integrity check per entry: source project reachable, record present, journal entry hash matches
- Entry status `OK`, `STALE` or `UNAVAILABLE`
- Catalog content digest over a canonical ordering of entries
- Full rebuild from registered projects

**Out** (and which node owns it instead):
- Incremental indexing (`RPT.CATALOG.STORE`)
- Journal chain verification itself (`CORE.JRN`, `TRUST.CHAIN`)

## Requirements

- **REQ-RPT.CATALOG.CHECK-001** `stats.check_catalog@1` shall be side-effect class R0 and report, per project, the counts of `OK`, `STALE` and `UNAVAILABLE` entries and list every `STALE` entry.
- **REQ-RPT.CATALOG.CHECK-002** If an entry's journal entry hash no longer matches its source project, then the check shall mark it `STALE`, and data sets shall exclude it with that reason.
- **REQ-RPT.CATALOG.CHECK-003** The catalog shall have a content digest computed over its entries in canonical order, independent of insertion order.
- **REQ-RPT.CATALOG.CHECK-004** When the catalog is deleted and rebuilt from the same registered projects at the same journal heads, the rebuilt catalog shall have the same content digest.
- **REQ-RPT.CATALOG.CHECK-005** Rebuild shall be side-effect class W1 and shall never modify a source project.

## Interfaces

- **Offers:** `stats.check_catalog@1` (R0), `stats.rebuild_catalog@1` (W1); entry status for `RPT.STATS.DSET`.
- **Consumes:** catalog store (`RPT.CATALOG.STORE`), journal reads (`CORE.JRN`).

## Decomposition hints

Likely one or two leaves (check; rebuild with digest).

## Open questions

- None yet.

## Children

_None yet: status `open`._
