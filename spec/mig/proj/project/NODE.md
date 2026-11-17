---
id: MIG.PROJ.PROJECT
title: Project-export import command
level: 3
parent: MIG.PROJ
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.PROJ-001, REQ-MIG.PROJ-002, REQ-MIG.PROJ-006]
covers: []
depends_on: [MIG.PROJ.PROFILE, MIG.PROJ.XID, MIG.PROJ.MAPREP, MIG.PROJ.POINTS, MIG.PROJ.ALIGN, MIG.PROJ.CHAR, MIG.SRC.GATE, IO.REG.ENV, IO.QIF.IDS, IO.DMIS.OUT, IO.RES.CSV]
side_effect: W2
determinism: D0
cli: [fid migrate import-project-export]
mcp: [migrate_import_project_export]
journal: [migrate.import_project_export@1]
updated: 2026-11-17
---

# MIG.PROJ.PROJECT: Project-export import command

## Purpose

`migrate.import_project_export@1` brings a whole admitted incumbent project export (a QIF plan-and-results file, a DMIS output file with features and tolerances, or a documented CSV feature/characteristic export with an optional point table) into a project in one transaction: nominal features and point sets (`MIG.PROJ.POINTS` mapping), alignments as known transforms (`MIG.PROJ.ALIGN`), and characteristics with datums (`MIG.PROJ.CHAR`), all with stable IDs and one mapping report. Either everything the report lists as created is committed, or nothing is.

## Scope

**In:**
- The command, run through the `IO.REG.ENV` envelope, with the `MIG.SRC` gate after format detection
- Ordering: features and point sets, then alignments, then characteristics (so references resolve), all built off-document
- One mapping report for the whole import, embedded in the result and its digest journaled
- Idempotent re-import (same file and profile → same entity IDs, no duplicates)
- Optional companion point file (second handle) for CSV exports that split features and points

**Out:**
- Each entity kind's mapping rules (owned by `MIG.PROJ.POINTS`, `MIG.PROJ.ALIGN`, `MIG.PROJ.CHAR`)
- Results-only import of the incumbent's values (owned by `IO.RES.TRANS`; a parallel run uses both)

## Requirements

- **REQ-MIG.PROJ.PROJECT-001** When `migrate.import_project_export@1` runs on an admitted export, the system shall create features, point sets, alignments and characteristics in one bus transaction, or create nothing if any step fails with an error.
- **REQ-MIG.PROJ.PROJECT-002** The command result shall include one mapping report that classifies every source feature, point set, alignment, characteristic and datum reference, and the journal event shall carry the report digest, the profile ID, version and digest, and the source digests.
- **REQ-MIG.PROJ.PROJECT-003** Re-importing the same source files with the same profile shall map to the same entity IDs and shall not create duplicate entities.
- **REQ-MIG.PROJ.PROJECT-004** If any source file's register entry is not `ADMITTED` or `OPEN_EXPORT_ONLY`, then the command shall fail with `source_not_admitted`, read nothing beyond format detection and add nothing.
- **REQ-MIG.PROJ.PROJECT-005** If the profile's format ID does not match the detected format of the source, then the command shall fail with `profile_format_mismatch` and add nothing.
- **REQ-MIG.PROJ.PROJECT-006** The command shall not create any ResultRecord; results come only from later evaluation commands.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/proj/project/qif-demo/` (QIF file with 10 features with measured points, 2 alignments, 12 characteristics, ASME stated), when `fid migrate import-project-export --file <handle> --profile qif-generic --json` runs, then the project holds those entities with the expected IDs in `expected/ids.json`, the mapping report equals `expected/maprep.json`, and no ResultRecord exists.
- **AC-2** Given `csv-split/` (feature/characteristic CSV plus companion point CSV), then features, point sets and characteristics are created and cross-referenced by external ID.
- **AC-3** Given AC-1 run twice, then the entity count is unchanged and IDs are identical.
- **AC-4** (error path) Given a DMIS output file with `--profile qif-generic`, then the command fails with `profile_format_mismatch` and journals nothing.
- **AC-5** (error path) Given a source register entry `PENDING_REVIEW`, then the command fails with `source_not_admitted` and the reader is never invoked.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `migrate.import_project_export@1`, W2; params `file: ImportHandle`, `points?: ImportHandle`, `profile: ProfileId`, `unit?: Unit`, `frame?: FrameRef`, `stage?`; result `{ created: BTreeMap<EntityKind, Vec<EntityId>>, mapping_report: MappingReport }`.
- **Data structures:** reuses entity types of the sibling leaves; `ImportPlan` (off-document entity set) committed by the envelope.
- **Algorithm:** resolve handles → detect formats → `source_gate` per file → profile load and format match → parse (QIF via `IO.QIF`, DMIS output via `IO.DMIS.OUT`, CSV via `IO.RES.CSV` column maps and the ASCII point reader) → POINTS mapping → ALIGN mapping → CHAR mapping → MAPREP finish → commit.
- **Errors:** `source_not_admitted`, `profile_format_mismatch`, `profile_source_not_admitted`, `unit_required`, reader errors.
- **Determinism:** D0.
- **Performance budget:** a 500-feature, 200,000-point QIF export in under 3 s [unverified until measured].
- **Crate:** `fid-mig` (module `proj::project`).

## Test fixtures

`fixtures/mig/proj/project/`: QIF, DMIS output and CSV exports authored for the project with a seeded generator, CC0-1.0. No file exported from an incumbent product is committed.

## Build prompt

> In crate `fid-mig`, add module `proj::project` and register `migrate.import_project_export@1` (W2; CLI `fid migrate import-project-export`, MCP `migrate_import_project_export`) using the IO import envelope with approved import-root handles only. Implement spec node MIG.PROJ.PROJECT: detect format, run the MIG.SRC `source_gate` per file before reading further (`source_not_admitted`), load the `mig-profile@1` profile and require format match (`profile_format_mismatch`), parse with the QIF, DMIS-output or CSV readers, then map features and point sets (`proj::points`), alignments (`proj::align`), characteristics (`proj::char`) in that order, off-document; finish one mapping report (`proj::maprep`); commit everything in one bus transaction or nothing; journal report digest, profile ID/version/digest and source digests. Never create ResultRecords. Re-import must be idempotent. Tests for AC-1..AC-6. Bounded parsing; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
