---
id: MIG.PROJ
title: Incumbent project and point-export import for re-evaluation
level: 2
parent: MIG
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-MIG-001, REQ-MIG-007]
covers: []
depends_on: [IO.QIF.IDS, IO.CLOUD.ASCII, IO.DMIS.OUT, IO.RES.CSV, ALN.XFM.MATRIX, GDT.CHAR.MODEL, MIG.SRC]
cli: [fid migrate import-project-export, fid migrate import-point-export]
mcp: [migrate_import_project_export, migrate_import_point_export]
journal: [migrate.import_project_export@1, migrate.import_point_export@1]
updated: 2026-11-16
---

# MIG.PROJ: Incumbent project and point-export import for re-evaluation

## Purpose

Ladder stage 3. Brings the measurement content of incumbent work into a project so our kernel can re-evaluate it: measured points per feature, nominal features, alignments (as known transforms) and characteristics with tolerances and datum references. Sources are only admitted exports (QIF, DMIS output, documented CSV point and feature exports). The point of this node is the **mapping**: incumbent feature, alignment and characteristic identities onto our project model, keeping the incumbent identifiers so that parallel-run can pair results one-to-one. Results re-evaluated from imported points carry basis `POINTS` (or `FEATURES` when only fitted features were exported).

## Scope

**In:**
- Mapping profiles per admitted incumbent export (field and identifier mapping as data), versioned
- Import of measured point sets per feature with their frame and unit, and of nominal features
- Import of incumbent alignments as known transforms, labelled with their incumbent name; never re-fitted silently
- Import of characteristics, tolerances and datum references onto the QIF-based characteristic model, with the dialect left unset until a human chooses it if the export does not state it
- Preservation of incumbent identifiers as external IDs for pairing
- A per-import mapping report: every source item as `MAPPED`, `PARTIAL` or `UNSUPPORTED` with reason
- `migrate.import_project_export@1` (W2), `migrate.import_point_export@1` (W2)

**Out** (and which node owns it instead):
- Parsing QIF, DMIS output, CSV and point tables (owned by `IO.QIF`, `IO.DMIS.OUT`, `IO.RES.CSV`, `IO.CLOUD.ASCII`)
- Results-only transcription (owned by `IO.RES.TRANS`)
- Evaluation of imported data (owned by `GDT` and `ALN`)
- Pairing and differencing against incumbent results (owned by `MIG.PARALLEL`)
- Admission of formats (owned by `MIG.SRC`)
- Corpus coverage tables (owned by `MIG.COVER`)

## Requirements

- **REQ-MIG.PROJ-001** When an admitted incumbent export is imported, the system shall produce a mapping report that classifies every source feature, alignment and characteristic as `MAPPED`, `PARTIAL` or `UNSUPPORTED`, with a reason for the last two.
- **REQ-MIG.PROJ-002** Every imported feature, alignment and characteristic shall keep the incumbent identifier and source file digest as an external ID, and re-importing the same file shall map to the same entity IDs (UUIDv5 from source digest and incumbent ID).
- **REQ-MIG.PROJ-003** An imported incumbent alignment shall be stored as a known transform labelled with its source, and shall not be replaced by a re-fitted alignment unless a user runs an alignment command explicitly.
- **REQ-MIG.PROJ-004** If an export does not state the GD&T dialect of a characteristic, then the imported characteristic shall have no dialect and its evaluation shall be NOT_DECIDABLE until a human sets one.
- **REQ-MIG.PROJ-005** Imported point coordinates shall be converted to SI metres with a single journaled rounding per value, and the source unit and frame shall be recorded on the point set.
- **REQ-MIG.PROJ-006** If an import references a source whose `MIG.SRC` entry is not admitted, then the import shall refuse with `source_not_admitted` and add nothing.

## Interfaces

- **Offers:** `migrate.import_project_export@1` (W2), `migrate.import_point_export@1` (W2); `MigrationMappingProfile` data; external-ID index used by `MIG.PARALLEL`; mapping reports for `MIG.COVER`.
- **Consumes:** `IO.QIF.IDS`, `IO.CLOUD.ASCII`, `IO.DMIS.OUT`, `IO.RES.CSV`, `ALN.XFM.MATRIX`, `GDT.CHAR.MODEL`, `MIG.SRC` gate.

## Decomposition hints

PROFILE (mapping profile schema and loader), POINTS (point and nominal feature import), ALIGN (alignment import as known transforms), CHAR (characteristic and datum import), XID (external-ID index and stable mapping), REPORT (mapping report).

## Open questions

- Which documented incumbent CSV and project exports exist and are admissible is not verified [unverified]; profiles are added only as `MIG.SRC` entries are admitted.

## Children

_None yet: status `open`._
