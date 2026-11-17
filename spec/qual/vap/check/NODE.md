---
id: QUAL.VAP.CHECK
title: Framework checklists (17025, Part 11 and Annex 11)
level: 3
parent: QUAL.VAP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.VAP-001, REQ-QUAL.VAP-004]
covers: []
depends_on: [QUAL.VAP.TPL]
uses_artifacts: [TRUST.PROFILE.MAP, QUAL.MATRIX.DATA]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.VAP.CHECK: Framework checklists (17025, Part 11 and Annex 11)

## Purpose

Checklist templates a customer works through for ISO/IEC 17025 records and for 21 CFR Part 11 and EU GMP Annex 11, each row citing its framework by designator, edition and clause ID only, with a project-written description and links to compliance-matrix rows and `TRUST.PROFILE.MAP` control IDs. A CI check enforces the citation form and that every link resolves.

## Scope

**In:**
- Templates `checklist-17025` and `checklist-part11-annex11` in `vap-template/1` with rows: designator, edition, clause ID, project-written description, matrix row IDs, profile control IDs, customer answer fields
- Check: citation form, no quotation-marked passage over 12 words, link resolution against the matrix data file and `regulated-mapping.csv`

**Out** (and which node owns it instead):
- The matrix rows (owned by `QUAL.MATRIX.DATA`) and the Part 11 control mapping (owned by `TRUST.PROFILE.MAP`)

## Requirements

- **REQ-QUAL.VAP.CHECK-001** Every checklist row shall hold a designator, an edition, a clause ID and a project-written description.
- **REQ-QUAL.VAP.CHECK-002** If a row description contains a quotation-marked passage longer than 12 words, then the check shall fail and name the row.
- **REQ-QUAL.VAP.CHECK-003** If a row cites a matrix row ID absent from the matrix data file or a control ID absent from `regulated-mapping.csv`, then the check shall fail and name the row.
- **REQ-QUAL.VAP.CHECK-004** Every Part 11 and Annex 11 row shall link at least one `TRUST.PROFILE.MAP` control ID or be marked `not_supported` with a reason.

## Acceptance criteria

- **AC-1** Given the shipped checklists, fixture matrix `fixtures/qual/vap/check/rows.toml` and mapping CSV, when `cargo xtask qual vap check-checklists --matrix rows.toml --mapping regulated-mapping.csv` runs, then it exits 0.
- **AC-2** (error path) Given a row citing matrix row `M-999`, then it exits 1 naming the row.
- **AC-3** (error path) Given a row with a 20-word quoted passage, then it exits 1.
- **AC-4** (error path) Given a Part 11 row with no control and no `not_supported`, then it exits 1.

## Design

- **Registry command(s):** none; repository check `cargo xtask qual vap check-checklists --matrix <toml> --mapping <csv>` (R0).
- **Data structures:** `ChecklistRow { id, designator, edition, clause, description: Untrusted<String>, matrix_rows, controls, not_supported: Option<String> }`.
- **Algorithm:** set lookups; quote detection as in `QUAL.CRA.TECHDOC`.
- **Errors:** `CHK-FIELD(row)`, `CHK-QUOTE(row)`, `CHK-LINK(row, id)`, `CHK-PART11(row)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::vap::checklists`), templates under `vap/templates/`.

## Test fixtures

`fixtures/qual/vap/check/` with synthetic matrix rows and mapping CSV; CC-BY-4.0.

## Build prompt

> Write `vap/templates/checklist-17025.json` and `checklist-part11-annex11.json` (`vap-template/1`), rows citing designator, edition and clause ID only, with descriptions in the project's own words (never quote a standard or regulation), matrix row IDs and `TRUST.PROFILE.MAP` control IDs; mark clause IDs [unverified] until a reviewer confirms. In `fid-xtask`, add `qual vap check-checklists --matrix <rows.toml> --mapping <regulated-mapping.csv>` failing on missing fields, quoted passages over 12 words, unresolved links, or Part 11/Annex 11 rows with neither a control nor `not_supported`. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.VAP.CHECK.

## Open questions

- Clause IDs need review by someone holding the standards (D-016 pattern) [unverified clause IDs].

## Children

_Leaf: none._
