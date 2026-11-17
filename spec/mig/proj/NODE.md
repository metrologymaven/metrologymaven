---
id: MIG.PROJ
title: Incumbent project and point-export import for re-evaluation
level: 2
parent: MIG
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-MIG-001, REQ-MIG-007]
covers: []
depends_on: [IO.QIF.IDS, IO.CLOUD.ASCII, IO.DMIS.OUT, IO.RES.CSV, IO.REG.ENV, IO.REG.PROV, ALN.XFM.MATRIX, GDT.CHAR.MODEL, GDT.CHAR.SPEC, GDT.CHAR.IDENT, MIG.SRC.GATE, MIG.SRC.REG]
uses_artifacts: [MIG.COVER.TAXO]
cli: [fid migrate import-project-export, fid migrate import-point-export]
mcp: [migrate_import_project_export, migrate_import_point_export]
journal: [migrate.import_project_export@1, migrate.import_point_export@1]
updated: 2026-11-17
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

- **Offers:** `migrate.import_project_export@1` (W2), `migrate.import_point_export@1` (W2); `MigrationMappingProfile` data (`mig-profile@1`); the external-ID index used by `MIG.PARALLEL.PAIR`; mapping reports (`mig-mapreport@1`) read by `MIG.COVER.GEN`; imported alignments used as `ALIGNMENT` probe candidates by `MIG.EXPLAIN.VARIANT`.
- **Consumes:** `IO.QIF.IDS`, `IO.CLOUD.ASCII`, `IO.DMIS.OUT`, `IO.RES.CSV`, `IO.REG.ENV`, `IO.REG.PROV`, `ALN.XFM.MATRIX`, `GDT.CHAR.MODEL`, `GDT.CHAR.SPEC`, `GDT.CHAR.IDENT`, `MIG.SRC.GATE` and `MIG.SRC.REG`; construct IDs from `MIG.COVER.TAXO` taxonomies (artifact).

## Decomposition hints

PROFILE (mapping profile schema and loader), POINTS (point and nominal feature import), ALIGN (alignment import as known transforms), CHAR (characteristic and datum import), XID (external-ID index and stable mapping), REPORT (mapping report).

## Open questions

- Which documented incumbent CSV and project exports exist and are admissible is not verified [unverified]; profiles are added only as `MIG.SRC` entries are admitted (D-033: open exports only until P4).
- `IO.CLOUD.ASCII` is phased P1b while this node is P1a. Until it lands, `migrate.import_point_export@1` can read point data only from QIF measured points; the IO maintainers should consider moving the ASCII point-table reader to P1a.

## Split rationale

Split by **entity kind being mapped** (points and nominal features, alignments, characteristics), with the three shared rules pulled into their own leaves so they hold identically for both commands: mapping profiles (PROFILE), identity (XID) and the mapping report (MAPREP). The two commands are separate leaves because the point-export import is usable alone (many shops can export points but not whole projects) and is smaller.

Alternatives considered:
- **One child per incumbent product.** Rejected: profiles are data, so per-product code would duplicate the mapping engine and split the clean-room gate.
- **One child per source format (QIF, DMIS output, CSV).** Rejected: format parsing is IO's; the MIG-owned work is entity mapping, which is shared across formats.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [MIG.PROJ.PROFILE](profile/NODE.md) | Mapping profile schema and loader (leaf) | 001 006 | none |
| [MIG.PROJ.XID](xid/NODE.md) | External-ID index and stable entity IDs (leaf) | 002 | none |
| [MIG.PROJ.MAPREP](maprep/NODE.md) | Mapping report model and classification (leaf) | 001 | none |
| [MIG.PROJ.POINTS](points/NODE.md) | Point-export import of measured points and nominal features (leaf) | 001 005 006 | none |
| [MIG.PROJ.ALIGN](align/NODE.md) | Incumbent alignments as labelled known transforms (leaf) | 003 | none |
| [MIG.PROJ.CHAR](char/NODE.md) | Characteristic, tolerance and datum mapping (leaf) | 004 | none |
| [MIG.PROJ.PROJECT](project/NODE.md) | Project-export import command (leaf) | 001 002 006 | none |
