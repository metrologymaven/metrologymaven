---
id: RPT.STATS.DSET.MAT
title: Data-set materialisation, ordering and digest
level: 4
parent: RPT.STATS.DSET
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.DSET-001, REQ-RPT.STATS.DSET-002, REQ-RPT.STATS.DSET-003, REQ-RPT.STATS.DSET-006]
covers: [PW.MSP.01]
depends_on: [RPT.STATS.DSET.DEF, RPT.STATS.DSET.EXCL, RPT.CATALOG.QUERY, CORE.PRJ.CONT, CORE.PRJ.STAGE, CORE.JRN.CANON, GDT.CHAR.RECORD]
side_effect: R1
determinism: D0
cli: [fid stats compute-dataset]
mcp: [stats_compute_dataset]
journal: [stats.compute_dataset@1]
updated: 2026-11-17
---

# RPT.STATS.DSET.MAT: Data-set materialisation, ordering and digest

## Purpose

Turns a `DataSetDef` into a `DataSet`: per characteristic, the ordered list of used records with their values and pieces, and the list of excluded records with their reasons, sealed by a digest. Every statistic (DESC, TREND, SPC, CAP, CMP) reads this object through one trait, so "which results were used" has one answer and the same inputs always give the same digest.

## Scope

**In:**
- Resolving the source (project runs and stages, or a saved catalog query) into candidate records
- Applying automatic and user exclusions
- Total ordering by the chosen key, then record ID
- Piece identity for each entry (used by piece stepping and comparison)
- The data-set digest
- `stats.compute_dataset@1` (R1) and the `DataSetReader` trait

**Out:**
- The definition and its storage (`RPT.STATS.DSET.DEF`)
- Exclusion reasons and the predicate (`RPT.STATS.DSET.EXCL`)
- Query execution and catalog status (`RPT.CATALOG.QUERY`, `RPT.CATALOG.CHECK`)
- Any statistic (DESC, TREND, SPC, CAP, CMP)

## Requirements

- **REQ-RPT.STATS.DSET.MAT-001** When `stats.compute_dataset@1` runs on a saved definition ID or an inline definition, the system shall return, per selected characteristic, the ordered list of used entries (record ID, piece ID, ordering-key value, value, unit, decision outcome) and the list of excluded record IDs each with one exclusion reason.
- **REQ-RPT.STATS.DSET.MAT-002** Every candidate record shall appear exactly once per characteristic, either as a used entry or as an exclusion.
- **REQ-RPT.STATS.DSET.MAT-003** A candidate with a `USER` exclusion in the definition shall be excluded with reason `USER`, unless an automatic reason applies, in which case the automatic reason shall be recorded.
- **REQ-RPT.STATS.DSET.MAT-004** Used entries shall be ordered by the ordering key (`STAGE`: stage position in the project, then run start time; `RUN_TIME`: run start time in UTC; `PART_SEQUENCE`: part sequence number), then by record ID compared as 16 bytes, and characteristics shall be ordered by characteristic ID.
- **REQ-RPT.STATS.DSET.MAT-005** The piece ID of an entry shall be the pair (source project ID, run ID), so that all records of one measured part instance share one piece ID.
- **REQ-RPT.STATS.DSET.MAT-006** The data-set digest shall be SHA-256 over the RFC 8785 canonical JSON of the definition digest, the ordered entries with values written as their IEEE 754 bit patterns, and the exclusions sorted by record ID.
- **REQ-RPT.STATS.DSET.MAT-007** Materialising the same definition against the same project journal heads and catalog state shall give the same digest on every supported target.
- **REQ-RPT.STATS.DSET.MAT-008** If the candidate count for one data set exceeds the configured limit (default 10 000 000 [unverified]), then the command shall fail with `stats.dataset_too_large` and return no partial data set.
- **REQ-RPT.STATS.DSET.MAT-009** The `DataSet` shall record the journal head of each source project and the catalog snapshot ID it was read from.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/three_part_run` with one record marked `NOT_DECIDABLE` and one user exclusion, when `fid stats compute-dataset --dataset <id> --json` runs, then each of the five characteristics lists three candidates in total, the `NOT_DECIDABLE` record is excluded with that reason, the user-excluded record with `USER`, and entries are in part-sequence order.
- **AC-2** (ordering ties) Given fixture `fixtures/rpt/stats/order_ties` (four runs with identical start times), when materialised with key `RUN_TIME`, then entries are ordered by record ID bytes.
- **AC-3** (precedence) Given a record that is both user-excluded and voided, when materialised, then it is excluded once, with reason `VOIDED`.
- **AC-4** (determinism) Given AC-1's definition, when materialised on x86-64, ARM64 and wasm32, then the data-set digests are identical, and a golden digest file `fixtures/rpt/stats/three_part_run.dsdigest` matches.
- **AC-5** (catalog source) Given fixture `fixtures/rpt/stats/catalog_two_projects` (two registered projects, one `STALE` entry), when a catalog-sourced definition is materialised, then the `STALE` entry is excluded with reason `STALE` and the data set records both project journal heads.
- **AC-6** (error path) Given the limit set to 10 and a source with 11 candidates, when the command runs, then it fails with `stats.dataset_too_large` and nothing beyond the failed call is written to the Activity Log.

## Design

- **Registry command:** `stats.compute_dataset@1(dataset: DataSetId | DataSetDef) -> DataSet`, R1 (scratch result; Activity Log only while recording or inside a Command Program, CONSTITUTION §3).
- **Data structures:**
  - `DataSet { def_digest, digest, sources: Vec<SourceHead{project, journal_head}>, catalog_snapshot: Option<SnapshotId>, order: OrderKey, chars: Vec<CharData> }`
  - `CharData { char: CharId, unit: Unit, entries: Vec<Entry>, excluded: Vec<(RecordId, ExclusionReason)> }`
  - `Entry { record: RecordId, piece: PieceId, key: OrderValue, value: f64, unit: Unit, outcome: Outcome }`; `PieceId { project: ProjectId, run: RunId }`
  - `trait DataSetReader { fn materialise(&self, id: &DataSetId) -> Result<DataSet, StatsError>; }` used by every statistic.
- **Algorithm:**
  1. Resolve candidates: project source reads ResultRecords of the selected runs and stages through the project API (CORE.PRJ); catalog source runs the saved query (`RPT.CATALOG.QUERY`) and attaches entry status.
  2. For each candidate, call `auto_exclusion` (DSET.EXCL); if none, apply a `USER` exclusion from the definition; otherwise keep it as an entry.
  3. Sort entries with a stable sort on (key, record-ID bytes); sort characteristics by ID; use `BTreeMap` only (no std HashMap, for deterministic iteration).
  4. Canonicalise (`fid-canon`, values as `F64Bits`) and hash with SHA-256.
- **Characteristic unit:** the unit declared on the characteristic; when a catalog source spans projects, the unit of the first project in project-ID order, with the others checked against it by DSET.EXCL.
- **Errors:** `stats.dataset_not_found`, `stats.dataset_too_large`, `stats.query_failed{cause}`, `stats.project_unavailable{project}`.
- **Determinism:** D0; sorting and hashing only, no arithmetic on values.
- **Performance budget:** 1 000 characteristics × 1 000 records materialised in under 1 s from a warm catalog [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/three_part_run`: shared synthetic project, plus a golden digest file `three_part_run.dsdigest`.
- `fixtures/rpt/stats/order_ties`: synthetic project with four runs sharing one start time.
- `fixtures/rpt/stats/catalog_two_projects`: two synthetic projects registered in a test catalog, one entry forced `STALE`.
- All built by the test builder with fixed values, CC-BY-4.0.

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT; may depend on CORE, `fid-canon`, the GDT ResultRecord types and the RPT catalog query API), implement data-set materialisation.
>
> **Command:** `stats.compute_dataset@1(dataset: DataSetId | DataSetDef) -> DataSet`, class R1, CLI `fid stats compute-dataset`, MCP `stats_compute_dataset`, `--json`.
>
> **Behaviour:** resolve candidates from project runs and stages or from a saved catalog query; for each candidate call `auto_exclusion` (module DSET.EXCL) and record the first automatic reason; else apply a `USER` exclusion from the definition; else keep it as an `Entry { record, piece: (project, run), key, value, unit, outcome }`. Every candidate appears exactly once. Order entries by key (`STAGE`: stage position then run start time; `RUN_TIME`: UTC start time; `PART_SEQUENCE`: part sequence number), then record-ID bytes; order characteristics by ID. Record the source journal heads and catalog snapshot. Digest = SHA-256 of RFC 8785 canonical JSON (`fid-canon`) of {def digest, ordered entries with values as IEEE 754 bit patterns, exclusions sorted by record ID}. Fail with `stats.dataset_too_large` above a configurable limit (default 10 000 000). Expose `trait DataSetReader` for the statistics modules.
>
> **Tests:** AC-1 to AC-6 above with fixtures `three_part_run` (and golden `three_part_run.dsdigest`), `order_ties` and `catalog_two_projects` under `fixtures/rpt/stats/`; run the digest test on x86-64, ARM64 and wasm32.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`, bounded inputs, fail closed); no std HashMap or HashSet; never modify records. Spec: `spec/rpt/stats/dset/mat/NODE.md`.

## Open questions

- Piece identity as (project, run) assumes one part instance per run. If a run can measure several part instances (fixtures with many parts), the piece ID needs a part-instance field from `CORE.PRJ`.
- Run start time and part sequence number are assumed to be run metadata in `CORE.PRJ.CONT`; confirm field names when that node is a leaf.

## Children

_Leaf: none._
