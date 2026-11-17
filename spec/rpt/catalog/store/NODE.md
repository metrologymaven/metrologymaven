---
id: RPT.CATALOG.STORE
title: Catalog store and incremental indexing
level: 3
parent: RPT.CATALOG
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG-001, REQ-RPT.CATALOG-002, REQ-RPT.CATALOG-007]
covers: [PW.MSP.04, PCD.RPT.07]
depends_on: [CORE.STO.LAYOUT, CORE.STO.SET, CORE.JRN.CHAIN, CORE.PRJ.CONT, IO.REG.ROOTS, GDT.CHAR.RECORD]
cli: [fid stats register-catalog, fid stats unregister-catalog, fid stats reindex-catalog]
mcp: [stats_register_catalog, stats_unregister_catalog, stats_reindex_catalog]
journal: [stats.register_catalog@1, stats.unregister_catalog@1, stats.reindex_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.STORE: Catalog store and incremental indexing

## Purpose

The local SQLite index of ResultRecord summaries across registered projects: its schema, project registration, and incremental indexing that follows each project's journal head. The index is derived data: every entry points back to its source project and journal entry hash, and the whole catalog can be rebuilt.

## Scope

**In:**
- Catalog schema: entry per ResultRecord with project, part, revision, plan, run, stage, characteristic, time, value and unit, decision, evaluation basis, record status, tags and source journal entry hash
- Register and unregister projects (by project handle, never raw path for agents)
- Incremental indexing from the last indexed journal position, on the next catalog command
- Catalog location in the user's machine scope settings (CORE.STO)

**Out** (and which node owns it instead):
- Query language (`RPT.CATALOG.QUERY`)
- Integrity check and rebuild (`RPT.CATALOG.CHECK`)
- Shared-folder catalogs (`RPT.CATALOG.SHARE`)
- Per-project SQLite and journal storage (`CORE.STO`, `CORE.JRN`)

## Requirements

- **REQ-RPT.CATALOG.STORE-001** The catalog shall hold one entry per journaled ResultRecord of each registered project, carrying the project ID, record ID and source journal entry hash.
- **REQ-RPT.CATALOG.STORE-002** When a catalog command runs and a registered project's journal head has advanced, the system shall index only the journal entries after the last indexed position.
- **REQ-RPT.CATALOG.STORE-003** Registering, unregistering and re-indexing shall be side-effect class W1 (derived, rebuildable data), and agents shall reference projects only by handle.
- **REQ-RPT.CATALOG.STORE-004** When a project is unregistered, the system shall remove all its entries from the catalog and leave the project untouched.
- **REQ-RPT.CATALOG.STORE-005** The catalog shall need no network service and no account (CONSTITUTION C7).
- **REQ-RPT.CATALOG.STORE-006** If a registered project cannot be opened, then indexing shall skip it, mark it `UNAVAILABLE`, and keep its existing entries marked as unverified.

## Interfaces

- **Offers:** `stats.register_catalog@1`, `stats.unregister_catalog@1`, `stats.reindex_catalog@1` (W1); type `CatalogEntry`; the schema used by QUERY, CHECK and SHARE.
- **Consumes:** storage and settings (`CORE.STO`), journal reads (`CORE.JRN`), project model (`CORE.PRJ`).

## Decomposition hints

SCHEMA (tables and migrations), REG (registration commands), INDEX (incremental indexer).

## Open questions

- Resolved: the catalog stores values with digests, not only references (decided in `RPT.CATALOG.STORE.SCHEMA`) for query speed; every row still points to its source record and journal entry hash.
- Catalog commands (W1) journal to the user settings journal, not to any project.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.CATALOG.STORE.SCHEMA | Catalog database schema and migrations (leaf) | 001 005 | PW.MSP.04, PCD.RPT.07 |
| RPT.CATALOG.STORE.REG | Project registration commands (leaf) | 003 004 | none |
| RPT.CATALOG.STORE.INDEX | Incremental indexing from project journals (leaf) | 001 002 006 | PW.MSP.04, PCD.RPT.07 |
