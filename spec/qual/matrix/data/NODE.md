---
id: QUAL.MATRIX.DATA
title: Compliance matrix schema, rows and review rules
level: 3
parent: QUAL.MATRIX
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.MATRIX-001, REQ-QUAL.MATRIX-002, REQ-QUAL.MATRIX-004, REQ-QUAL.MATRIX-005]
covers: []
depends_on: []
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.MATRIX.DATA: Compliance matrix schema, rows and review rules

## Purpose

The compliance matrix as data: one row per framework clause the project has evidence for, naming the framework designator and edition, clause ID, the platform control, spec requirement IDs, test or check IDs and evidence members, with a review status. The file header carries the customer-responsibility statement. A schema check enforces the row shape, the no-quotation rule and the reviewer rule; reference resolution is in `QUAL.MATRIX.CHECK`.

## Scope

**In:**
- `compliance/matrix/rows.toml` schema `qual-matrix/1`: header (statement, schema version), rows: row ID (`M-NNN`), framework designator, edition, clause ID, project-written description, platform control, requirement IDs, test IDs, evidence member kinds, imported profile control IDs, review status `draft|reviewed`, reviewer, review PR
- `compliance/reviewers.toml`: people allowed to mark rows `reviewed`, with a one-line note of their relevant regulatory experience
- Initial rows for the P1a frameworks chosen by maintainers, all `draft`
- Edition policy: a row names exactly one edition; a revised framework gets new rows, old rows kept until the matrix drops that edition

**Out** (and which node owns it instead):
- Reference and import checks (owned by `QUAL.MATRIX.CHECK`)
- Rendering (owned by `QUAL.MATRIX.PAGE`)
- Part 11 and Annex 11 control rows (owned by `TRUST.PROFILE.MAP`; imported by ID)

## Requirements

- **REQ-QUAL.MATRIX.DATA-001** Every row shall have a framework designator, edition, clause ID, platform control, at least one requirement ID and at least one evidence member kind, and the schema check shall fail and name any row that lacks one.
- **REQ-QUAL.MATRIX.DATA-002** If a row description contains a quotation-marked passage or block quote longer than 12 words, then the check shall fail and name the row.
- **REQ-QUAL.MATRIX.DATA-003** The file header shall contain the statement that the matrix describes evidence the project supplies and that compliance decisions belong to the customer, and the check shall fail if it is absent or altered from the fixed text.
- **REQ-QUAL.MATRIX.DATA-004** If a row is `reviewed` without a reviewer listed in `reviewers.toml` and a review PR number, then the check shall fail and name the row.
- **REQ-QUAL.MATRIX.DATA-005** When the check runs on a PR with an exported list of approving reviews, a row changed to `reviewed` in that PR shall require an approving review by its named reviewer, and the check shall fail otherwise.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/matrix/data/ok/` (10 rows across 3 frameworks, header, reviewers), when `cargo xtask qual matrix check-data --rows rows.toml --reviewers reviewers.toml` runs, then it exits 0.
- **AC-2** (error path) Given `no-evidence/` (a row without evidence members), then it exits 1 naming the row.
- **AC-3** (error path) Given `quote/` (a 15-word quoted passage), then it exits 1.
- **AC-4** (error path) Given `header/` (statement edited), then it exits 1.
- **AC-5** (error path) Given `reviewed-no-approval/` (row changed to reviewed, PR reviews export lacking the reviewer's approval), when run with `--pr-diff base.toml --reviews reviews.json`, then it exits 1 naming the row.

## Design

- **Registry command(s):** none; repository check `cargo xtask qual matrix check-data --rows <toml> --reviewers <toml> [--pr-diff <base.toml> --reviews <json>]` (R0).
- **Data structures:** `Row { id, designator, edition, clause, description: Untrusted<String>, control, req_ids, test_ids, evidence: Vec<MemberKind>, profile_controls, review: { status, reviewer, pr } }`; fixed header text constant.
- **Algorithm:** schema checks; quote detection shared with `QUAL.CRA.TECHDOC`; PR mode diffs status by row ID.
- **Errors:** `MATRIX-FIELD(row, field)`, `MATRIX-QUOTE(row)`, `MATRIX-HEADER`, `MATRIX-REVIEW(row)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s for 2,000 rows.
- **Crate:** `fid-xtask` (module `qual::matrix::data`).

## Test fixtures

`fixtures/qual/matrix/data/{ok,no-evidence,quote,header,reviewed-no-approval}/` with synthetic designators (`TEST-STD 1:2020`); CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::matrix::data`: schema `qual-matrix/1` for `compliance/matrix/rows.toml` (header with fixed customer-responsibility statement; rows with id M-NNN, designator, edition, clause, project-written description, control, req_ids ≥1, test_ids, evidence member kinds ≥1, profile_controls, review {status draft|reviewed, reviewer, pr}) and `compliance/reviewers.toml`. `qual matrix check-data --rows --reviewers [--pr-diff <base> --reviews <json>]` fails on missing fields, quoted passages over 12 words, altered header, reviewed rows without listed reviewer and PR, and (PR mode) rows newly reviewed without an approving review from the named reviewer. Never paste standards text; cite designator, edition and clause ID only, mark unreviewed clause IDs [unverified]. No panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.MATRIX.DATA.

## Open questions

- Which frameworks are in the P1a matrix is a maintainer choice; clause IDs need expert review [unverified clause IDs].
- Revised frameworks (for example AS9100 to IA9100) follow the one-edition-per-row policy; whether to keep superseded editions is a maintainer choice.

## Children

_Leaf: none._
