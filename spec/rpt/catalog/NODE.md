---
id: RPT.CATALOG
title: Results catalog and historical import
level: 2
parent: RPT
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT-005]
covers: [PW.MSP.03, PW.MSP.04, PCD.RPT.07]
depends_on: [CORE, IO]
cli: [fid stats *-catalog, fid stats import-*, fid stats query-*]
mcp: [stats_*_catalog, stats_import_*, stats_query_*]
journal: [stats.*_catalog, stats.import_*]
updated: 2026-11-16
---

# RPT.CATALOG: Results catalog and historical import

## Purpose

A local, offline index of ResultRecords across every project on the machine (or on a shared folder the user points to), so statistics and dashboards can span parts, revisions and years. It also brings in historical results from CSV, Q-DAS and QIF Statistics files as transcription records. The catalog is an index, not the source of truth: every entry points back to a journaled record in its project.

## Scope

**In:**
- Catalog store: a local SQLite index (CONSTITUTION §2 local storage in CORE) of ResultRecord summaries with project, part, revision, plan, run, stage, characteristic, time and digests
- Registering and unregistering projects; incremental re-indexing when a project's journal advances
- Query language over the catalog (filters by part, characteristic, time window, tag, decision)
- Historical import from CSV, Q-DAS AQDEF and QIF Statistics into a historical-results project as transcription records
- Index integrity check against project journals
- Multi-user shared catalogs on a file share, read-only for non-owners

**Out** (and which node owns it instead):
- Parsing CSV, Q-DAS and QIF files (`IO`)
- Statistics, charts and dashboards over query results (`RPT.STATS`)
- Project storage and journal (`CORE`)
- Hosted or cloud data management servers: not in this node (see Open questions)

## Requirements

- **REQ-RPT.CATALOG-001** The catalog shall index every journaled ResultRecord of each registered project, and each entry shall carry the project ID and journal entry hash of its source.
- **REQ-RPT.CATALOG-002** When a registered project's journal advances, the system shall bring the catalog up to date on the next catalog command without re-indexing unchanged entries.
- **REQ-RPT.CATALOG-003** If a catalog entry's journal entry hash no longer matches its source project, then the integrity check shall report it as `STALE` and statistics shall exclude it.
- **REQ-RPT.CATALOG-004** The catalog shall answer a query over 1,000,000 indexed results filtered by part and characteristic in under 1 s on reference hardware [unverified].
- **REQ-RPT.CATALOG-005** When the user imports historical results, the system shall create transcription records with basis `RESULTS_ONLY`, preserving the source file's characteristic IDs, timestamps and source-software name.
- **REQ-RPT.CATALOG-006** If an import file is malformed or exceeds the configured size limit, then the import shall fail closed, report the first offending line or element, and journal nothing.
- **REQ-RPT.CATALOG-007** The catalog shall work fully offline, with no account and no network service (CONSTITUTION C7).
- **REQ-RPT.CATALOG-008** Query results shall be returned in a deterministic order (default: part, characteristic, time, record ID).
- **REQ-RPT.CATALOG-009** Deleting the catalog shall lose no data: rebuilding it from the registered projects shall give the same index content digest.

## Interfaces

- **Offers:** `stats.register_catalog@1` (add project), `stats.reindex_catalog@1`, `stats.check_catalog@1`, `stats.query_results@1` (R0), `stats.import_history@1` (W2); type `CatalogQuery`, `CatalogEntry`.
- **Consumes:** project and journal access (CORE), results readers for CSV, Q-DAS and QIF Statistics (IO).
- **Side effects:** queries R0; index maintenance W1 (derived, rebuildable data); import W2.

## Decomposition hints

STORE (schema and incremental indexing), QUERY (query model and ordering), CHECK (integrity and rebuild), IMPORT (historical import mapping), SHARE (shared-folder catalogs).

## Open questions

- PW.MSP.03 is a central, server-based data management and collaboration product [unverified]. This node covers the local and shared-folder part only. Whether the project should build a self-hosted server, or mark the server part out of scope, belongs to the maintainers; raise a decision if a later expansion needs it.

## Children

_None yet: status `open`._
