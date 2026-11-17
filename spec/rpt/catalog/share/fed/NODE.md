---
id: RPT.CATALOG.SHARE.FED
title: Queries across local and attached catalogs
level: 4
parent: RPT.CATALOG.SHARE
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.SHARE-005]
covers: []
depends_on: [RPT.CATALOG.QUERY.EXEC, RPT.CATALOG.SHARE.ATTACH]
side_effect: R0
determinism: D0
cli: [fid stats query-shared-catalog]
mcp: [stats_query_shared_catalog]
journal: [stats.query_shared_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.SHARE.FED: Queries across local and attached catalogs

## Purpose

Runs one `CatalogQuery` over the local catalog and every attached shared catalog and returns a single, deterministic, paginated result. Each catalog is queried with the per-catalog executor of `RPT.CATALOG.QUERY.EXEC`; this leaf merges the pages by the full sort tuple, adds the source catalog ID to each entry, and removes duplicate entries (the same project and record indexed in two catalogs).

## Scope

**In:** `stats.query_shared_catalog@1`; k-way merge by sort tuple then catalog ID; de-duplication by (project ID, record ID), preferring the entry whose journal entry hash is `OK`, then the local catalog; a composite cursor.

**Out:** single-catalog query translation (`RPT.CATALOG.QUERY.EXEC`); attaching catalogs (`RPT.CATALOG.SHARE.ATTACH`).

## Requirements

- **REQ-RPT.CATALOG.SHARE.FED-001** `stats.query_shared_catalog@1` shall be R0 and return entries from the local and all attached catalogs in the query's total order, with ties broken by catalog ID.
- **REQ-RPT.CATALOG.SHARE.FED-002** When the same (project ID, record ID) appears in several catalogs, the result shall contain it once, taken from an `OK` entry if any, else from the local catalog, and shall name the catalogs that hold it.
- **REQ-RPT.CATALOG.SHARE.FED-003** Paging with the composite cursor through unchanged catalogs shall return every distinct match exactly once.
- **REQ-RPT.CATALOG.SHARE.FED-004** If an attached catalog is unavailable, then the result shall list it as `unavailable` and still return the other catalogs' entries.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/catalog/shared_folder` attached and a local catalog sharing 20 records with it, when `fid stats query-shared-catalog --part P1 --json` runs, then each record appears once, in sort order, with its source catalogs listed.
- **AC-2** Paging with size 9 gives the same concatenated result as AC-1.
- **AC-3** Given the share folder removed, then the result lists the shared catalog as `unavailable` and returns local entries.

## Design

- **Registry command:** `stats.query_shared_catalog@1(query: CatalogQuery) -> { entries: Vec<(CatalogEntry, Vec<CatalogId>)>, unavailable: Vec<CatalogId>, next: Option<CompositeCursor> }` (R0, `--json`).
- **Algorithm:** run the executor per catalog (catalog-ID order) with page size + 1; merge with a binary heap keyed by (sort tuple, catalog ID); de-duplicate adjacent equal keys.
- **Errors:** `stats.query_invalid`, `stats.cursor_mismatch`.
- **Determinism:** D0.
- **Performance budget:** first page over two 1 000 000-entry catalogs under 2 s [unverified until measured].
- **Crate:** `fid-catalog` (module `share::fed`).

## Test fixtures

- `fixtures/rpt/catalog/shared_folder` plus a local catalog built by the same builder.

## Build prompt

> In crate `fid-catalog`, module `share::fed`, implement `stats.query_shared_catalog@1` (R0, `--json`; CLI `fid stats query-shared-catalog`; MCP name derived): run the typed query with the per-catalog executor on the local and every attached catalog, merge by (sort tuple, catalog ID), de-duplicate (project ID, record ID) preferring an `OK` entry then the local catalog, list holding catalogs, report unavailable catalogs without failing, and page with a composite cursor that returns every distinct match exactly once. Tests AC-1 to AC-3. CONSTITUTION §8 applies; ordered collections only. Spec: `spec/rpt/catalog/share/fed/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
