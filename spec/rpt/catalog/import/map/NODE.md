---
id: RPT.CATALOG.IMPORT.MAP
title: Mapping historical results into a historical-results project
level: 4
parent: RPT.CATALOG.IMPORT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.IMPORT-001]
covers: []
depends_on: [IO.RES.TRANS.MODEL, CORE.PRJ.CONT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.CATALOG.IMPORT.MAP: Mapping historical results into a historical-results project

## Purpose

Historical files describe results by the source system's part numbers, characteristic IDs and timestamps. This task leaf maps the transcription records that IO's results readers produce onto a historical-results project: one Part per source part number, one Revision per source revision label (or `unknown`), one Run per source measurement (inspection timestamp plus serial or sample ID), and one characteristic per source characteristic ID. It computes a pure, deterministic `ImportPlan` (entities to create or reuse and records to commit); the command leaf `RPT.CATALOG.IMPORT.BATCH` commits it.

## Scope

**In:** the historical-results project layout (one per catalog by default); entity matching rules (reuse an existing Part, Revision, Run or characteristic with the same source keys); preservation of source characteristic IDs, timestamps and source-software name; basis `RESULTS_ONLY`; derived entity IDs as UUIDv5 from source keys.

**Out:** parsing files and building transcription records (`IO.RES.TRANS.MODEL` with `IO.RES.CSV`, `IO.RES.AQDEF`, `IO.RES.QIFR`); committing, duplicates and limits (`RPT.CATALOG.IMPORT.BATCH`).

## Requirements

- **REQ-RPT.CATALOG.IMPORT.MAP-001** The mapper shall produce an `ImportPlan` that, for every transcription record, names its target Part, Revision, Run and characteristic, reusing existing entities whose source keys match and otherwise planning new ones.
- **REQ-RPT.CATALOG.IMPORT.MAP-002** New entity IDs shall be UUIDv5 derived from the historical project ID and the source keys, so that importing the same data twice plans the same IDs.
- **REQ-RPT.CATALOG.IMPORT.MAP-003** Every planned record shall keep the source characteristic ID, source timestamp and source-software name unchanged, and carry evaluation basis `RESULTS_ONLY`.
- **REQ-RPT.CATALOG.IMPORT.MAP-004** If a record lacks a part number or characteristic ID, then the mapper shall fail with error `stats.import_unmapped` naming the source line or element.
- **REQ-RPT.CATALOG.IMPORT.MAP-005** The mapper shall be a pure function of the transcription records and the current historical project state.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/catalog/history_csv_two_parts` read by the CSV reader (2 part numbers, 3 runs, 12 characteristics, 36 values), when mapped into an empty historical project, then the plan creates 2 Parts, 2 Revisions, 3 Runs, 12 characteristics and 36 `RESULTS_ONLY` records with the source IDs and timestamps preserved.
- **AC-2** When the same records are mapped again after the plan is committed, then the plan reuses every entity and its new-entity list is empty.
- **AC-3** Given the same input mapped twice into empty projects with the same project ID, then the two plans are byte-identical in canonical JSON.
- **AC-4** (error path) Given a record without a characteristic ID, then `stats.import_unmapped` names its source line.

## Design

- **Registry commands:** none (library used by `RPT.CATALOG.IMPORT.BATCH`; class R1 as a pure computation).
- **API:** `fn plan(records: &[TranscriptionRecord], project: &HistoricalProjectView) -> Result<ImportPlan, MapError>`.
- **Data structures:** `ImportPlan { new_parts, new_revisions, new_runs, new_chars, records: Vec<PlannedRecord> }`, each in source order; source keys `(part_number, revision_label, run_key, char_id)` as untrusted text.
- **Errors:** `stats.import_unmapped { at }`.
- **Determinism:** D0; UUIDv5 IDs; ordered collections only.
- **Performance budget:** plan 100 000 records in under 2 s [unverified until measured].
- **Crate:** `fid-catalog` (module `import::map`).

## Test fixtures

- `fixtures/rpt/catalog/history_csv_two_parts`: synthetic CSV with a column map, authored in-project, CC-BY-4.0.

## Build prompt

> In crate `fid-catalog`, module `import::map`, implement the pure function `plan(records, historical_project_view) -> ImportPlan`: map IO transcription records onto Part (source part number), Revision (source revision label or `unknown`), Run (timestamp plus serial or sample key) and characteristic (source characteristic ID), reusing entities with equal source keys and planning new ones with UUIDv5 IDs derived from the project ID and source keys. Keep source characteristic IDs, timestamps and source-software name; basis `RESULTS_ONLY`. Missing part number or characteristic ID → `stats.import_unmapped { at }`. No I/O, no commits. Tests AC-1 to AC-4. CONSTITUTION §8 and C6 apply. Spec: `spec/rpt/catalog/import/map/NODE.md`.

## Open questions

- Resolved from the parent: one historical-results project per catalog by default; a per-part-number layout can be added later as an option.

## Children

_Leaf: none._
