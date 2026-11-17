---
id: RPT.CATALOG.QUERY.SAVE
title: Saved catalog queries
level: 4
parent: RPT.CATALOG.QUERY
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.QUERY-006]
covers: []
depends_on: [RPT.CATALOG.QUERY.EXEC, CORE.STO.SET]
side_effect: W2
determinism: D0
cli: [fid stats save-query-catalog]
mcp: [stats_save_query_catalog]
journal: [stats.save_query_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.QUERY.SAVE: Saved catalog queries

## Purpose

Data sets and dashboards refer to a query by ID and version rather than repeating its filters. This leaf saves a `CatalogQuery` (without cursor) under a stable ID with an incrementing version in the user's catalog settings, as a journaled W2 command. Old versions stay resolvable so that a statistic computed with version 3 can still say exactly which query it used.

## Scope

**In:** `stats.save_query_catalog@1`; saved-query storage in the user settings scope; versioning; resolution by (ID, version) for `RPT.STATS.DSET`.

**Out:** executing queries (`RPT.CATALOG.QUERY.EXEC`); data sets (`RPT.STATS.DSET`); queries published by a shared catalog's owner (not in scope; see Open questions).

## Requirements

- **REQ-RPT.CATALOG.QUERY.SAVE-001** When a query is saved without an ID, the system shall assign a UUIDv7 ID and version 1; when saved with an existing ID, it shall store the next version and keep every earlier version.
- **REQ-RPT.CATALOG.QUERY.SAVE-002** Each save shall be one journaled W2 command in the user settings journal, storing the query in canonical JSON with its SHA-256 digest.
- **REQ-RPT.CATALOG.QUERY.SAVE-003** If the query fails validation or contains a cursor, then the command shall fail with error `stats.query_invalid` and journal nothing.
- **REQ-RPT.CATALOG.QUERY.SAVE-004** The query name shall be untrusted text of at most 128 bytes (CONSTITUTION C6).
- **REQ-RPT.CATALOG.QUERY.SAVE-005** A resolve function shall return the exact stored query and digest for an (ID, version) pair, or error `stats.query_not_found`.

## Acceptance criteria

- **AC-1** When `fid stats save-query-catalog --name "Bore C3" --part P1 --char C3 --json` runs, then a new ID with version 1 and the query digest are returned and one event is journaled.
- **AC-2** When saved again with that ID and a different time window, then version 2 is stored and version 1 still resolves to the original digest.
- **AC-3** (error path) Given a query with a cursor, then `stats.query_invalid` and the journal length is unchanged.
- **AC-4** (replay) Given the settings journal after AC-2, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `stats.save_query_catalog@1(id: Option<SavedQueryId>, name: Untrusted<Text>, query: CatalogQuery) -> { id, version: u32, digest: Sha256 }` (W2, `--json`).
- **Data structures:** `SavedQuery { id, version, name, query, digest }`, stored as a versioned list under setting key `stats.saved_queries` (user scope).
- **Errors:** `stats.query_invalid`, `stats.query_not_found`, `report.text_invalid`.
- **Determinism:** D0.
- **Performance budget:** trivial [unverified until measured].
- **Crate:** `fid-catalog` (module `saved`).

## Test fixtures

- None beyond in-test queries.

## Build prompt

> In crate `fid-catalog`, module `saved`, implement `stats.save_query_catalog@1` (W2, `--json`; CLI `fid stats save-query-catalog`; MCP name derived): validate the `CatalogQuery` (no cursor allowed), assign a UUIDv7 ID on first save and increment the version on each later save, keep every version, store canonical JSON plus SHA-256 under the user-scope setting `stats.saved_queries`, journal one event in the user settings journal. Provide `resolve(id, version)`. Name is untrusted text ≤ 128 bytes. Tests AC-1 to AC-4. CONSTITUTION §8 applies. Spec: `spec/rpt/catalog/query/save/NODE.md`.

## Open questions

- Saved queries live in the user's settings, also when a shared catalog is attached; queries published by a shared catalog's owner are not offered yet.

## Children

_Leaf: none._
