---
id: RPT.STATS.DSET.DEF
title: Data-set definition and save
level: 4
parent: RPT.STATS.DSET
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.DSET-001, REQ-RPT.STATS.DSET-003]
covers: []
depends_on: [CORE.PRJ.CONT, CORE.PRJ.STAGE, CORE.JRN.CANON, RPT.CATALOG.QUERY]
side_effect: [W2, R0]
determinism: D0
cli: [fid stats save-dataset, fid stats list-dataset]
mcp: [stats_save_dataset, stats_list_dataset]
journal: [stats.save_dataset@1]
updated: 2026-11-17
---

# RPT.STATS.DSET.DEF: Data-set definition and save

## Purpose

Defines the `DataSetDef` object (which results go into a statistic, in which order, and which the user has excluded) and the commands to save and list definitions in the project. A definition holds no values; it is turned into an ordered record list by `RPT.STATS.DSET.MAT`. Its canonical digest is the first input of every data-set digest.

## Scope

**In:**
- `DataSetDef` type and its validation
- Source: project runs and stages, or a saved catalog query
- Characteristic selection and ordering key
- The user-exclusion list field (edited by `RPT.STATS.DSET.EXCL`)
- Canonical definition digest
- `stats.save_dataset@1` (W2) and `stats.list_dataset@1` (R0)

**Out:**
- Materialisation, ordering and the data-set digest (`RPT.STATS.DSET.MAT`)
- Exclusion reason codes and the exclude/reinclude commands (`RPT.STATS.DSET.EXCL`)
- Executing catalog queries (`RPT.CATALOG.QUERY`)

## Requirements

- **REQ-RPT.STATS.DSET.DEF-001** A `DataSetDef` shall hold an ID, a name, a source (a list of project runs with an optional stage filter, or one saved catalog query ID), a characteristic selection (an explicit list of characteristic IDs or `ALL`), an ordering key (`STAGE`, `RUN_TIME` or `PART_SEQUENCE`) and a user-exclusion list.
- **REQ-RPT.STATS.DSET.DEF-002** When `stats.save_dataset@1` runs with a valid definition, the system shall store it in the project as a journaled W2 command, assigning a UUIDv7 ID to a new definition and keeping the ID of an existing one.
- **REQ-RPT.STATS.DSET.DEF-003** If a definition names a run, stage, characteristic or saved query that does not exist in the project or catalog, then the save shall fail with error `stats.dataset_invalid` naming the field and the missing ID, and nothing shall be journaled.
- **REQ-RPT.STATS.DSET.DEF-004** The definition digest shall be SHA-256 over the RFC 8785 canonical JSON of the definition, with list fields in their stored order and the user-exclusion list sorted by record ID.
- **REQ-RPT.STATS.DSET.DEF-005** The definition shall store no result values and no computed statistics.
- **REQ-RPT.STATS.DSET.DEF-006** When `stats.list_dataset@1` runs, the system shall return every saved definition's ID, name, source kind and definition digest, ordered by name and then ID.
- **REQ-RPT.STATS.DSET.DEF-007** The definition name shall be typed as untrusted text (C6), limited to 200 characters, and a longer name shall fail with `stats.dataset_invalid`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/three_part_run`, when `fid stats save-dataset --name "bore trend" --runs all --order part-sequence --json` runs, then a definition with a UUIDv7 ID is returned, one `stats.save_dataset@1` event is journaled, and `fid stats list-dataset --json` lists it with the same definition digest.
- **AC-2** Given the definition from AC-1, when it is saved again unchanged, then the ID and definition digest are unchanged.
- **AC-3** (digest order) Given two definitions equal except that their user-exclusion lists were entered in different orders, when both digests are computed, then they are identical.
- **AC-4** (error path) Given a definition naming run ID `00000000-0000-7000-8000-000000000000`, which is not in the project, when the save runs, then it fails with `stats.dataset_invalid` naming `source.runs` and nothing is journaled.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `stats.save_dataset@1(def: DataSetDef) -> DataSetRef`, W2. Agents write to an agent branch (CONSTITUTION §3).
  - `stats.list_dataset@1() -> Vec<DataSetSummary>`, R0.
- **Data structures:**
  - `DataSetDef { id: DataSetId, schema: 1, name: UntrustedText, source: DataSetSource, chars: CharSelection, order: OrderKey, user_exclusions: Vec<UserExclusion> }`
  - `enum DataSetSource { Project { runs: RunSelection, stages: Option<Vec<StageId>> }, Catalog { query: SavedQueryId } }`; `enum RunSelection { All, List(Vec<RunId>) }`
  - `enum CharSelection { All, List(Vec<CharId>) }`
  - `enum OrderKey { Stage, RunTime, PartSequence }`
  - `UserExclusion { record: RecordId, reason: UntrustedText, by: PrincipalId }` (the list is written only by `RPT.STATS.DSET.EXCL`; `save_dataset` keeps the stored list and ignores a supplied one when the ID already exists)
  - `DataSetSummary { id, name, source_kind, def_digest }`
- **Storage:** an element type `stats.dataset_def` registered with the project's element-type registry (CORE.PRJ), journaled through the command bus.
- **Algorithm:** validate references against the project (runs, stages, characteristics) and the catalog (saved query exists); canonicalise with `fid-canon` (CORE.JRN.CANON); digest with SHA-256.
- **Errors:** `stats.dataset_invalid{field, id}`, `stats.dataset_not_found`.
- **Determinism:** D0; no floating-point arithmetic.
- **Performance budget:** save and list under 50 ms for 1 000 definitions [unverified until measured].
- **Crate:** `fid-stats-cmd` (L2 RPT command crate).

## Test fixtures

- `fixtures/rpt/stats/three_part_run`: synthetic project, one plan, three runs (parts 1 to 3), five characteristics; built by the test builder, CC-BY-4.0. Shared with `RPT.STATS.DESC`.

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT command crate; may depend on CORE and lower, on `fid-canon`, and on the RPT catalog query API), implement data-set definitions.
>
> **Types:** `DataSetDef { id, schema: 1, name, source, chars, order, user_exclusions }` with `DataSetSource::{Project{runs: All|List, stages: Option<Vec<StageId>>}, Catalog{query: SavedQueryId}}`, `CharSelection::{All, List}`, `OrderKey::{Stage, RunTime, PartSequence}`, `UserExclusion { record, reason, by }`. Names and reasons are untrusted text (CONSTITUTION C6), name at most 200 characters.
>
> **Commands:** `stats.save_dataset@1(def) -> DataSetRef` (W2, journaled; new ID is UUIDv7; existing ID kept; the user-exclusion list of an existing definition is preserved, because only the exclude/reinclude commands edit it), and `stats.list_dataset@1() -> Vec<DataSetSummary>` (R0, ordered by name then ID). CLI `fid stats save-dataset`, `fid stats list-dataset`; MCP `stats_save_dataset`, `stats_list_dataset`; `--json` on both. Store definitions as project element type `stats.dataset_def`.
>
> **Digest:** SHA-256 of RFC 8785 canonical JSON (use `fid-canon`), user exclusions sorted by record ID.
>
> **Errors:** `stats.dataset_invalid{field, id}` when a referenced run, stage, characteristic or saved query does not exist, or the name is too long; nothing is journaled on error.
>
> **Tests:** AC-1 to AC-5 above with fixture `fixtures/rpt/stats/three_part_run`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`, bounded inputs); no values are stored in a definition. Spec: `spec/rpt/stats/dset/def/NODE.md`.

## Open questions

- Where a definition lives when the user works only against the catalog, with no project open, is undecided; this leaf assumes a project is always open (the Report app could keep a statistics project).
- Matching characteristics across projects in a catalog source relies on the characteristic key that `RPT.CATALOG.QUERY` defines.

## Children

_Leaf: none._
