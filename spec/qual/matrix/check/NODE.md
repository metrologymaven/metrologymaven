---
id: QUAL.MATRIX.CHECK
title: Matrix reference and profile-map import check
level: 3
parent: QUAL.MATRIX
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.MATRIX-003, REQ-QUAL.MATRIX-006]
covers: []
depends_on: [QUAL.MATRIX.DATA]
uses_artifacts: [TRUST.PROFILE.MAP, QUAL.EVID.LAYOUT, ECO.CI.WF.TESTRPT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.MATRIX.CHECK: Matrix reference and profile-map import check

## Purpose

Resolves every reference in the matrix: requirement IDs against the spec tree, test IDs against the CI test report, evidence member kinds against the release's evidence statement, and imported profile controls against `TRUST.PROFILE.MAP`'s CSV. Imported Part 11 and Annex 11 rows are referenced by control ID, never restated, and every control in the map must be imported.

## Scope

**In:**
- Reference resolution with failures naming the row
- Import rule: every `control_id` in `regulated-mapping.csv` appears in some row's `profile_controls`; no row restates a mapped control's description

**Out** (and which node owns it instead):
- Row schema (owned by `QUAL.MATRIX.DATA`); the mapping CSV (owned by `TRUST.PROFILE.MAP`)

## Requirements

- **REQ-QUAL.MATRIX.CHECK-001** If a row cites a requirement ID absent from the spec tree, a test ID absent from `test-report.json`, or an evidence member kind absent from the release's evidence statement, then the check shall fail and name the row and reference.
- **REQ-QUAL.MATRIX.CHECK-002** If a `control_id` of `regulated-mapping.csv` is cited by no row, or a row cites a control ID absent from the CSV, then the check shall fail and name it.
- **REQ-QUAL.MATRIX.CHECK-003** If a row's description equals a mapped control's description, then the check shall fail, so imported rows are referenced rather than restated.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/matrix/check/ok/` (rows, spec tree, test report, evidence statement, mapping CSV), when `cargo xtask qual matrix check-refs --rows rows.toml --spec spec/ --tests test-report.json --evidence statement.json --mapping regulated-mapping.csv` runs, then it exits 0.
- **AC-2** (error path) Given a row citing `REQ-CORE.BUS-999`, then it exits 1 naming the row.
- **AC-3** (error path) Given a mapping CSV with control `C-SEQ` cited by no row, then it exits 1 naming `C-SEQ`.
- **AC-4** (error path) Given a row citing evidence kind `pentest_report`, then it exits 1.

## Design

- **Registry command(s):** none; repository check `cargo xtask qual matrix check-refs ...` (R0).
- **Data structures:** reference sets as BTreeSets.
- **Algorithm:** parse requirement lines from `spec/**/NODE.md`, test IDs from `test-report.json` v1, member kinds from `qual-evidence-statement/1`, control IDs from the CSV.
- **Errors:** `MREF-REQ(row, id)`, `MREF-TEST(row, id)`, `MREF-EVID(row, kind)`, `MREF-IMPORT(control)`, `MREF-RESTATED(row)`.
- **Determinism:** D0.
- **Performance budget:** under 3 s.
- **Crate:** `fid-xtask` (module `qual::matrix::check`).

## Test fixtures

`fixtures/qual/matrix/check/` synthetic inputs; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::matrix::check`: `qual matrix check-refs --rows <toml> --spec <dir> --tests <test-report.json> --evidence <statement.json> --mapping <regulated-mapping.csv>`. Using `qual::matrix::data` types, fail naming the row when a requirement ID is not a `- **REQ-...**` line in the spec tree, a test ID is not in test-report v1, or an evidence member kind is not in the evidence statement; fail when a mapping control is cited by no row or a row cites an unknown control; fail when a row description equals a mapped control's description. No panics, deny unwrap/expect, bounded CSV parsing. Tests: AC-1..AC-4 of spec node QUAL.MATRIX.CHECK.

## Open questions

- Test IDs in `test-report.json` are per leaf and acceptance criterion; rows cite them as `<LEAF.ID>#AC-n`.

## Children

_Leaf: none._
