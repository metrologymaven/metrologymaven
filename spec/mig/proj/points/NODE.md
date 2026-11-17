---
id: MIG.PROJ.POINTS
title: Point-export import of measured points and nominal features
level: 3
parent: MIG.PROJ
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.PROJ-001, REQ-MIG.PROJ-005, REQ-MIG.PROJ-006]
covers: []
depends_on: [MIG.PROJ.PROFILE, MIG.PROJ.XID, MIG.PROJ.MAPREP, MIG.SRC.GATE, IO.REG.ENV, IO.REG.PROV, IO.CLOUD.ASCII, IO.QIF.IDS]
side_effect: W2
determinism: D0
cli: [fid migrate import-point-export]
mcp: [migrate_import_point_export]
journal: [migrate.import_point_export@1]
updated: 2026-11-17
---

# MIG.PROJ.POINTS: Point-export import of measured points and nominal features

## Purpose

The smallest useful stage-3 path: many shops can export measured points per feature from their incumbent (as QIF measured points or as a documented CSV/ASCII table) even when they cannot export a whole project. `migrate.import_point_export@1` reads such an export under a mapping profile, creates one point set per incumbent feature (with source unit and frame recorded, coordinates converted to SI metres with one rounding each) and, where the export carries them, nominal features. It also provides the point-set and nominal-feature mapping used by `MIG.PROJ.PROJECT`.

## Scope

**In:**
- `migrate.import_point_export@1` (W2) through the `IO.REG.ENV` import envelope, with the `MIG.SRC` gate run before any bytes beyond format detection are read
- Grouping rows or elements into per-feature point sets by the profile's feature-identifier field
- Optional per-point normals and probe-compensation flags when the profile maps them
- Nominal features (type, nominal geometry parameters) when present in the export
- Unit and frame resolution through `IO.REG.PROV` (from file, fixed by profile, or caller-supplied; otherwise `unit_required`)
- Mapping-report items for every source feature and point set

**Out:**
- Parsing the files (owned by `IO.CLOUD.ASCII` and `IO.QIF`)
- Alignments and characteristics (owned by `MIG.PROJ.ALIGN`, `MIG.PROJ.CHAR`)
- Fitting or evaluating the points (owned by `GDT`, `ALN`, `KRN`)

## Requirements

- **REQ-MIG.PROJ.POINTS-001** When `migrate.import_point_export@1` runs on an admitted export, the system shall create one point set per distinct incumbent feature identifier, each with an `ExternalId` and an entity ID from `MIG.PROJ.XID`.
- **REQ-MIG.PROJ.POINTS-002** Each coordinate shall be converted to SI metres with a single rounding through `IO.REG.PROV`, and each point set shall record the source unit, the source frame label and the source file digest.
- **REQ-MIG.PROJ.POINTS-003** If the source unit cannot be resolved from the file, the profile or the caller, then the import shall fail with `unit_required` and add nothing.
- **REQ-MIG.PROJ.POINTS-004** If the source's register entry is not `ADMITTED` or `OPEN_EXPORT_ONLY`, then the import shall fail with `source_not_admitted` before reading beyond format detection and add nothing.
- **REQ-MIG.PROJ.POINTS-005** The command result shall include the mapping report, with every source feature and point set classified, and the journal event shall carry the report digest and the profile digest.
- **REQ-MIG.PROJ.POINTS-006** Re-importing the same file with the same profile shall produce the same entity IDs and a byte-identical mapping report and shall not duplicate entities.
- **REQ-MIG.PROJ.POINTS-007** Rows with non-finite coordinates shall be excluded from their point set and reported as `PARTIAL` items with reason `value_out_of_range`, never silently dropped.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/proj/points/csv-mm/` (CSV export, 6 features, 1,200 points, unit mm in header, profile `csv-points-generic`), when `fid migrate import-point-export --file <handle> --profile csv-points-generic --json` runs, then 6 point sets are created, coordinates equal `expected/points-si.json` bit for bit, and the mapping report equals `expected/maprep.json`.
- **AC-2** Given the same import run twice, then entity IDs are identical and the project holds 6 point sets, not 12.
- **AC-3** (error path) Given `csv-nounit/` with no unit in file, profile or caller, then the command fails with `unit_required` and journals nothing.
- **AC-4** (error path) Given a source whose register entry is `NOT_ADMITTED`, then the command fails with `source_not_admitted` and the reader was never invoked.
- **AC-5** Given `csv-nan/` with 3 rows containing `NaN`, then those rows are reported `PARTIAL` with `value_out_of_range` and excluded.
- **AC-6** Given `qif-points/` (QIF measured points for 4 features), then 4 point sets are created with external IDs taken from the QIF feature identifiers.
- **AC-7** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `migrate.import_point_export@1`, W2; params `file: ImportHandle`, `profile: ProfileId`, `unit?: Unit`, `frame?: FrameRef`, `stage?`; result `{ point_sets: Vec<EntityId>, nominals: Vec<EntityId>, mapping_report: MappingReport }`.
- **Data structures:** imported `PointSet { id, external_id, points_si: Vec<[f64; 3]>, normals?, source_unit, source_frame, source_sha256 }` in the project document; nominal features as GEO feature entities with external IDs.
- **Algorithm:** envelope resolves handle → detect format → `source_gate` → reader under limits → group by feature identifier (BTreeMap, source order kept within group) → unit/frame resolution and SI conversion (`IO.REG.PROV`) → derive IDs (XID) → build mapping report (MAPREP) → commit one transaction.
- **Errors:** `source_not_admitted`, `unit_required`, `profile_source_not_admitted`, `profile_format_mismatch`, reader errors from `IO.REG.LIMITS`.
- **Determinism:** D0.
- **Performance budget:** 1 M points in under 2 s including parsing [unverified until measured].
- **Crate:** `fid-mig` (module `proj::points`).

## Test fixtures

`fixtures/mig/proj/points/`: CSV and QIF exports synthesised by a seeded generator written for the project (not exported from an incumbent), CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `proj::points` and register `migrate.import_point_export@1` (W2; CLI `fid migrate import-point-export`, MCP `migrate_import_point_export`) using the IO import envelope (`FormatCommand` skeleton) with approved import-root handles only. Implement spec node MIG.PROJ.POINTS: run the MIG.SRC `source_gate` right after format detection (refuse `source_not_admitted`, reading nothing more); read via the ASCII point-table or QIF reader; group rows into one point set per incumbent feature identifier per the `mig-profile@1` profile; resolve unit and frame (file, profile, caller, else `unit_required`) and convert to SI metres with one rounding through the provenance service; derive entity IDs with `proj::xid`; exclude non-finite rows as `PARTIAL value_out_of_range`; build the mapping report with `proj::maprep`; commit in one transaction and journal profile and report digests. Re-import must be idempotent. Tests for AC-1..AC-7. Bounded, fail-closed parsing; no `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- `IO.CLOUD.ASCII` is P1b; until it is built only the QIF path (AC-6) can pass in P1a.

## Children

_Leaf: none._
