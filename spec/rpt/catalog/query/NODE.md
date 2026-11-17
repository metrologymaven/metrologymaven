---
id: RPT.CATALOG.QUERY
title: Catalog query model
level: 3
parent: RPT.CATALOG
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG-004, REQ-RPT.CATALOG-008]
covers: []
depends_on: [RPT.CATALOG.STORE, CORE.STO.SET]
cli: [fid stats query-results, fid stats save-query-catalog]
mcp: [stats_query_results, stats_save_query_catalog]
journal: [stats.save_query_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.QUERY: Catalog query model

## Purpose

The typed query model over the catalog: filters by part, revision, characteristic, time window, tag, stage, decision and record status, with a total, deterministic result order and pagination. Queries are data structures, not query strings, so no user text ever becomes SQL. Saved queries are what data sets and dashboards refer to.

## Scope

**In:**
- `CatalogQuery` type: filters, ordering, page size and cursor
- Translation to parameterised SQL with no string interpolation of user values
- Deterministic ordering (default: part, characteristic, time, record ID)
- Saved queries with IDs and versions
- Performance targets and indexes

**Out** (and which node owns it instead):
- Schema and indexing (`RPT.CATALOG.STORE`)
- Exclusion of `STALE` entries from statistics (`RPT.STATS.DSET` uses `RPT.CATALOG.CHECK` status)

## Requirements

- **REQ-RPT.CATALOG.QUERY-001** `stats.query_results@1` shall be side-effect class R0 and accept only a typed `CatalogQuery`, never a raw query string.
- **REQ-RPT.CATALOG.QUERY-002** User-supplied filter values shall be passed only as bound SQL parameters (CONSTITUTION C6).
- **REQ-RPT.CATALOG.QUERY-003** Query results shall be returned in a total order: the requested sort keys, then record ID; default part, characteristic, time, record ID.
- **REQ-RPT.CATALOG.QUERY-004** A query over 1 000 000 indexed results filtered by part and characteristic shall return its first page in under 1 s on reference hardware [unverified].
- **REQ-RPT.CATALOG.QUERY-005** Pagination shall use a stable cursor so that paging through an unchanged catalog returns every match exactly once.
- **REQ-RPT.CATALOG.QUERY-006** When the user saves a query, the system shall store it with an ID and version as a journaled W2 command in the user's catalog settings.

## Interfaces

- **Offers:** `stats.query_results@1` (R0), `stats.save_query_catalog@1` (W2); types `CatalogQuery`, `SavedQueryId`.
- **Consumes:** catalog schema (`RPT.CATALOG.STORE`).

## Decomposition hints

MODEL (query type and SQL translation), ORDER (ordering and cursor), PERF (indexes and benchmark).

## Open questions

- Resolved: saved queries live in the user's catalog settings (user scope), also when a shared catalog is attached (`RPT.CATALOG.QUERY.SAVE`). Queries across local and attached catalogs are `RPT.CATALOG.SHARE.FED`, built on the per-catalog executor of `RPT.CATALOG.QUERY.EXEC`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.CATALOG.QUERY.EXEC | Typed catalog query, ordering and pagination (leaf) | 001 002 003 004 005 | none |
| RPT.CATALOG.QUERY.SAVE | Saved catalog queries (leaf) | 006 | none |
