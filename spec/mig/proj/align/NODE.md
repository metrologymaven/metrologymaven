---
id: MIG.PROJ.ALIGN
title: Incumbent alignments as labelled known transforms
level: 3
parent: MIG.PROJ
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.PROJ-003]
covers: []
depends_on: [ALN.XFM.MATRIX, MIG.PROJ.XID, MIG.PROJ.MAPREP, MIG.PROJ.PROFILE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.PROJ.ALIGN: Incumbent alignments as labelled known transforms

## Purpose

Brings an incumbent's alignment across exactly as the incumbent computed it: as a known rigid transform (rotation and translation, from a matrix or origin-plus-axes record in the export), labelled with the incumbent alignment name and source, and validated through `ALN.XFM.MATRIX`. It is never re-fitted: results evaluated in it reproduce the incumbent's frame, and any later re-fit is a separate, explicit alignment command by the user. Imported alignments are also the candidates for the `ALIGNMENT` factor in `MIG.EXPLAIN`.

## Scope

**In:**
- Mapping alignment records (4×4 matrix, 3×3 rotation plus translation, or origin plus primary and secondary axis) to an `ALN.XFM.MATRIX` known-matrix entry
- Label `incumbent:<namespace>:<alignment name>` and an `ExternalId`; parent alignment chain if the export states one
- Validation outcomes mapped to mapping-report items: orthonormality failure → `UNSUPPORTED` with `value_out_of_range`; missing translation unit → `PARTIAL` with `unit_unknown`
- A lock flag `imported_known_transform` that alignment-solver commands must not overwrite implicitly

**Out:**
- Matrix validation and transform storage (owned by `ALN.XFM.MATRIX`)
- Fitting alignments (owned by `ALN`)
- Comparison with our alignments (owned by `ALN.QA.CMP`, used by `MIG.EXPLAIN.VARIANT`)

## Requirements

- **REQ-MIG.PROJ.ALIGN-001** For each alignment record the profile maps, the system shall create a known-transform alignment through `ALN.XFM.MATRIX` with the incumbent label, an `ExternalId` and the source digest.
- **REQ-MIG.PROJ.ALIGN-002** An origin-plus-axes record shall be converted to a rotation matrix by right-handed completion of the stated axes, and the conversion shall be listed in the mapping-report item detail.
- **REQ-MIG.PROJ.ALIGN-003** If `ALN.XFM.MATRIX` rejects the transform, then no alignment shall be created and the item shall be `UNSUPPORTED` with reason `value_out_of_range`.
- **REQ-MIG.PROJ.ALIGN-004** An imported alignment shall carry `imported_known_transform = true`, and no command shall replace its transform unless the command is an explicit alignment command naming that alignment.
- **REQ-MIG.PROJ.ALIGN-005** If an alignment record references a parent alignment that is not in the same import, then the item shall be `PARTIAL` with reason `missing_field` and the alignment shall be created relative to the project's base frame.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/proj/align/matrix.json` (a QIF-style 4×4 record, mm translation), when mapped, then a known-transform alignment is created whose matrix equals `expected-si.json` bit for bit and whose label is `incumbent:demo:ALN_MAIN`.
- **AC-2** Given `axes.json` (origin, primary Z, secondary X), then the rotation equals the right-handed completion in `expected-axes.json` and the report detail names the conversion.
- **AC-3** (error path) Given `skewed.json` (non-orthonormal rotation beyond the matrix tolerance), then no alignment is created and the item is `UNSUPPORTED value_out_of_range`.
- **AC-4** Given an imported alignment, when a best-fit solver command targets another alignment, then the imported transform is unchanged.

## Design

- **Registry command(s):** none; library called by `migrate.import_project_export@1` (`MIG.PROJ.PROJECT`).
- **Data structures:** `ImportedAlignment { external_id, label, transform: KnownMatrixRef, parent?, imported_known_transform: bool }`.
- **Algorithm:** read fields by profile selectors → unit conversion of translation (single rounding) → axes completion (normalise primary, Gram-Schmidt secondary, cross product for third, using `fid-num`) → `ALN.XFM.MATRIX` entry with its validation → XID → MAPREP item.
- **Errors:** propagated from `ALN.XFM.MATRIX` and recorded as report items, not raised.
- **Determinism:** D0.
- **Performance budget:** negligible (< 1 ms per alignment) [unverified until measured].
- **Crate:** `fid-mig` (module `proj::align`).

## Test fixtures

`fixtures/mig/proj/align/`: hand-written alignment records, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `proj::align` implementing spec node MIG.PROJ.ALIGN: map profile-selected alignment records (4×4 matrix, rotation+translation, or origin+primary+secondary axes) to a known-matrix alignment through the ALN known-matrix entry API, labelled `incumbent:<namespace>:<name>`, with ExternalId (`proj::xid`) and source digest; complete axes right-handedly with `fid-num` operations (no FMA) and record the conversion in the report detail; when the matrix validation rejects, create nothing and add an `UNSUPPORTED value_out_of_range` item; set `imported_known_transform = true` so that only an explicit alignment command naming it may replace it; missing parent → `PARTIAL missing_field`, relative to base frame. Tests for AC-1..AC-4. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Enforcing REQ-004 needs `ALN` solver commands to honour the lock flag; the `ALN` maintainers should confirm the flag is checked in `ALN.MODEL`.

## Children

_Leaf: none._
