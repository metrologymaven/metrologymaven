---
id: RPT.CATALOG.CHECK
title: Catalog integrity check and rebuild
level: 3
parent: RPT.CATALOG
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG-003, REQ-RPT.CATALOG-009]
covers: []
depends_on: [RPT.CATALOG.STORE, CORE.JRN.CHAIN, CORE.JRN.CANON]
side_effect: [R0, W1]
determinism: D0
cli: [fid stats check-catalog, fid stats rebuild-catalog]
mcp: [stats_check_catalog, stats_rebuild_catalog]
journal: [stats.check_catalog@1, stats.rebuild_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.CHECK: Catalog integrity check and rebuild

## Purpose

Proves that the catalog is only an index: the integrity check compares every entry with its source project's journal and marks mismatches `STALE`, and rebuild recreates the catalog from the registered projects with the same content digest. This keeps statistics from silently using results that no longer match their record.

## Scope

**In:**
- Integrity check per entry: source project reachable, record present at the stored journal entry, journal entry hash matches
- Entry status `OK`, `STALE` or `UNAVAILABLE`, written back to the entry status column
- Catalog content digest over a canonical ordering of entries
- Full rebuild from registered projects into a new file, swapped in atomically

**Out** (and which node owns it instead):
- Incremental indexing (`RPT.CATALOG.STORE.INDEX`)
- Journal chain verification itself (`CORE.JRN.CHAIN`, `TRUST.CHAIN`)
- Excluding `STALE` entries from data sets (`RPT.STATS.DSET`, which reads the status)

## Requirements

- **REQ-RPT.CATALOG.CHECK-001** `stats.check_catalog@1` shall be side-effect class R0 and report, per project, the counts of `OK`, `STALE` and `UNAVAILABLE` entries and list every `STALE` entry.
- **REQ-RPT.CATALOG.CHECK-002** If an entry's journal entry hash no longer matches its source project, then the check shall mark it `STALE`, and data sets shall exclude it with that reason.
- **REQ-RPT.CATALOG.CHECK-003** The catalog shall have a content digest computed over its entries in canonical order, independent of insertion order.
- **REQ-RPT.CATALOG.CHECK-004** When the catalog is deleted and rebuilt from the same registered projects at the same journal heads, the rebuilt catalog shall have the same content digest.
- **REQ-RPT.CATALOG.CHECK-005** Rebuild shall be side-effect class W1 and shall never modify a source project.
- **REQ-RPT.CATALOG.CHECK-006** The content digest shall be the SHA-256 of the RFC 8785 canonical JSON array of entries sorted by (project ID, record ID), excluding the entry status column and all catalog-local timestamps.
- **REQ-RPT.CATALOG.CHECK-007** If a rebuild fails part-way, then the existing catalog shall remain in place unchanged.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/catalog/three_projects` indexed, when `fid stats check-catalog --json` runs, then every entry is `OK` and the content digest is reported.
- **AC-2** Given one row's journal entry hash altered in the catalog file by the test, when the check runs, then that entry is `STALE`, it is listed, and the other entries stay `OK`.
- **AC-3** Given p3 removed, then p3's entries are `UNAVAILABLE`.
- **AC-4** Given AC-1, when the catalog file is deleted and `fid stats rebuild-catalog --json` runs, then the new digest equals AC-1's digest, and every project folder digest is unchanged.
- **AC-5** Given the same projects indexed in two different registration orders, then the content digests are equal.
- **AC-6** (error path) Given a rebuild interrupted by fault injection, then the previous catalog file is byte-identical to before.

## Design

- **Registry commands:** `stats.check_catalog@1(project: Option<ProjectId>) -> CheckReport { per_project: Vec<(ProjectId, ok, stale, unavailable)>, stale: Vec<EntryKey>, digest: Sha256 }` (R0; it updates only the derived entry status column, which is excluded from the digest and never reaches the Record Journal); `stats.rebuild_catalog@1() -> { digest: Sha256, entries: u64 }` (W1, journaled to the user settings journal).
- **Algorithm (check):** per project in ID order, open read-only, look up each entry's record by journal position, compare hashes. **Rebuild:** create a temporary catalog next to the current one, register the same projects, run the indexer to each project's current head, compute the digest, then atomically rename over the old file.
- **Errors:** `stats.catalog_locked`, `stats.catalog_read_only`, `stats.rebuild_failed { project }`.
- **Determinism:** D0; canonical ordering and RFC 8785 encoding.
- **Performance budget:** check 1 000 000 entries in under 60 s [unverified until measured].
- **Crate:** `fid-catalog` (module `check`).

## Test fixtures

- `fixtures/rpt/catalog/three_projects` (shared with `RPT.CATALOG.STORE`).

## Build prompt

> In crate `fid-catalog`, module `check`, implement `stats.check_catalog@1` (R0) and `stats.rebuild_catalog@1` (W1, journaled to the user settings journal), both `--json`; CLI `fid stats check-catalog`, `rebuild-catalog`; MCP names derived. Check: for every entry, open its project read-only, find the record at the stored journal position and compare the journal entry hash; set entry status `OK`, `STALE` or `UNAVAILABLE`; report counts per project, the stale list and the content digest. Digest = SHA-256 of the RFC 8785 canonical JSON array of entries sorted by (project ID, record ID), excluding entry status and catalog-local timestamps. Rebuild into a temporary file with the indexer, then rename atomically; on failure leave the old catalog untouched. Never write to a project. Tests AC-1 to AC-6 (fixture `fixtures/rpt/catalog/three_projects`). CONSTITUTION §8 applies. Spec: `spec/rpt/catalog/check/NODE.md`.

## Open questions

- Writing the derived entry status column during an R0 check is treated as cache maintenance, not a state change of record; confirm with CORE that this fits R0.
- **Build status (partial, not marked built).** Crate `fid-catalog` module `check` implements the check (`check_catalog`: `OK`, `STALE`, `UNAVAILABLE`, written back in one `BEGIN IMMEDIATE` transaction; per-project counts, stale list, optional project filter) and the content digest (`content_digest`, REQ-003 and REQ-006), tested at library level against `fixtures/rpt/catalog/check/make_cases.py` with synthetic source journals. Projects are read through the `ProjectSources` and `RecordLookup` traits, because handle resolution (`RPT.CATALOG.STORE.REG`) and finding a ResultRecord in a journal entry (`RPT.CATALOG.STORE.INDEX`) are not built. **Open ACs:** AC-1 (needs the `fixtures/rpt/catalog/three_projects` fixture, the journal-backed `ProjectSources` and the `stats.check_catalog@1` command and CLI); AC-4 and AC-6 (`stats.rebuild_catalog@1` needs the indexer); AC-2 and AC-3 pass at library level and must be re-run on `three_projects`. AC-5 is covered (digest equal across registration and insertion orders). Set `built_in` once every AC passes.
- Digest content (build choice): every `entry` column except `entry_status`, plus the entry's tags as a sorted array; the catalog has no catalog-local timestamp column (`timestamp` is the record's own instant, so it is included). `journal_seq` is a decimal string and the SI value the `0x` IEEE 754 bit form, following `fid-canon`'s number rules.
- Build choice: a project filter naming an unregistered project is refused with `stats.catalog_unknown_project` (a code not in the Design's error list); a read-only catalog open is `stats.catalog_read_only`.

## Children

_Leaf: none._
