---
id: RPT.CATALOG.STORE.INDEX
title: Incremental indexing from project journals
level: 4
parent: RPT.CATALOG.STORE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.STORE-001, REQ-RPT.CATALOG.STORE-002, REQ-RPT.CATALOG.STORE-006]
covers: [PW.MSP.04, PCD.RPT.07]
depends_on: [RPT.CATALOG.STORE.SCHEMA, CORE.JRN.CHAIN, GDT.CHAR.RECORD]
side_effect: W1
determinism: D0
cli: [fid stats reindex-catalog]
mcp: [stats_reindex_catalog]
journal: [stats.reindex_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.STORE.INDEX: Incremental indexing from project journals

## Purpose

Keeps the catalog current. For each registered project, the indexer reads journal entries after the last indexed position, extracts every committed ResultRecord (and every void or supersession that changes a record's status), and upserts catalog rows. It runs explicitly with `stats.reindex_catalog@1` and implicitly at the start of every other catalog command, so users never see a catalog behind its projects.

## Scope

**In:** journal scan from the last position; ResultRecord extraction (GDT results, direct readings, transcription records); record status changes (voided, superseded); `UNAVAILABLE` handling; the explicit command and the implicit catch-up hook.

**Out:** schema (`RPT.CATALOG.STORE.SCHEMA`); registration (`RPT.CATALOG.STORE.REG`); hash comparison and rebuild (`RPT.CATALOG.CHECK`).

## Requirements

- **REQ-RPT.CATALOG.STORE.INDEX-001** When indexing a registered project, the system shall read only journal entries after the stored position and insert one row per newly committed ResultRecord with its source journal entry hash.
- **REQ-RPT.CATALOG.STORE.INDEX-002** When an indexed record is voided or superseded in its project's journal, the system shall update that row's record status and keep the row.
- **REQ-RPT.CATALOG.STORE.INDEX-003** The system shall store the new position and head hash in the same transaction as the rows, so that an interrupted pass leaves the catalog at the previous consistent position.
- **REQ-RPT.CATALOG.STORE.INDEX-004** If a registered project cannot be opened, then the pass shall skip it, mark the project `UNAVAILABLE`, mark its rows `unverified`, and continue with the other projects.
- **REQ-RPT.CATALOG.STORE.INDEX-005** Before any other catalog command runs, the system shall run an indexing pass over every registered project whose journal head differs from the stored head hash.
- **REQ-RPT.CATALOG.STORE.INDEX-006** If a project's stored head hash is no longer in its journal (history rewritten or a different project at the handle), then the system shall mark the project `UNAVAILABLE` with reason `head_mismatch` and leave its rows for `RPT.CATALOG.CHECK`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/catalog/three_projects` registered, when `fid stats reindex-catalog --json` runs, then 160 rows exist, each with its record's journal entry hash.
- **AC-2** Given 10 new results appended to p2, when any catalog command runs, then exactly 10 journal entries are read (counted by the test reader) and 170 rows exist.
- **AC-3** Given a reading voided in p1, after the next pass its row has status `voided`.
- **AC-4** Given p3's folder renamed away, then the pass marks p3 `UNAVAILABLE`, its rows `unverified`, and indexes p1 and p2.
- **AC-5** Given a pass killed after 50 of 100 records (fault injection), then the catalog is at the previous position and a new pass completes with the same digest as an uninterrupted pass.

## Design

- **Registry command:** `stats.reindex_catalog@1(project: Option<ProjectId>) -> { per_project: Vec<(ProjectId, added, updated, status)> }` (W1, `--json`).
- **Algorithm:** per project in project-ID order: open read-only → verify the stored head hash is in the chain → iterate entries after the position → map ResultRecord fields to rows → one transaction per project per 10 000 entries, each storing position and hash.
- **Errors:** `stats.catalog_locked`, `stats.catalog_read_only`; per-project failures are statuses, not errors.
- **Determinism:** D0; rows are a function of the journal content.
- **Performance budget:** index 100 000 new records in under 30 s [unverified until measured].
- **Crate:** `fid-catalog` (module `index`).

## Test fixtures

- `fixtures/rpt/catalog/three_projects` (shared with `RPT.CATALOG.STORE.REG`).

## Build prompt

> In crate `fid-catalog`, module `index`, implement incremental indexing and `stats.reindex_catalog@1` (W1, `--json`; CLI `fid stats reindex-catalog`; MCP name derived). For each registered project (project-ID order), open it read-only, check the stored head hash is in its journal (else `UNAVAILABLE`, `head_mismatch`), read only entries after the stored position, insert one row per committed ResultRecord with its journal entry hash, and update record status on voids and supersessions. Store position and head hash in the same transaction as the rows. Skip projects that cannot be opened (`UNAVAILABLE`, rows `unverified`). Provide a catch-up hook that every other catalog command calls first. Tests AC-1 to AC-5 with fixture `fixtures/rpt/catalog/three_projects`, including fault injection. CONSTITUTION §8 rules apply; projects are never written. Spec: `spec/rpt/catalog/store/index/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
