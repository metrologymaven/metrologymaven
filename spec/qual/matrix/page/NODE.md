---
id: QUAL.MATRIX.PAGE
title: Per-release matrix rendering
level: 3
parent: QUAL.MATRIX
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.MATRIX-001, REQ-QUAL.MATRIX-004]
covers: []
depends_on: [QUAL.MATRIX.DATA]
uses_artifacts: [QUAL.EVID.ASSEMBLE, TRUST.PROFILE.MAP]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.MATRIX.PAGE: Per-release matrix rendering

## Purpose

Renders the matrix for each release into a Markdown page and a CSV, citing the release's evidence-statement digest, with the customer-responsibility statement first and each row's review status visible, so customers and auditors read exactly what the project claims for that release.

## Scope

**In:**
- `compliance-matrix-<release>.md` and `.csv`, grouped by framework then clause
- First section: the customer-responsibility statement, release version, evidence-statement digest, count of draft and reviewed rows
- Imported profile controls shown as links to the `TRUST.PROFILE.MAP` page, not restated

**Out** (and which node owns it instead):
- Row content and checks (owned by `QUAL.MATRIX.DATA`, `QUAL.MATRIX.CHECK`)

## Requirements

- **REQ-QUAL.MATRIX.PAGE-001** When the renderer runs for a release, it shall write a Markdown page and a CSV containing every row, grouped by framework designator and edition and sorted by clause ID.
- **REQ-QUAL.MATRIX.PAGE-002** The page shall begin with the customer-responsibility statement, followed by the release version and the evidence-statement digest from that release's bundle.
- **REQ-QUAL.MATRIX.PAGE-003** Every rendered row shall show its review status, and the page shall state the number of `draft` rows.
- **REQ-QUAL.MATRIX.PAGE-004** Given the same rows and evidence statement, the renderer shall produce byte-identical outputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/matrix/page/ok/` (rows and evidence statement for v1.4.0), when `cargo xtask qual matrix render --rows rows.toml --evidence statement.json --release v1.4.0 --out out/` runs, then the page and CSV equal the golden files.
- **AC-2** Given AC-1's page, then its first paragraph is the responsibility statement and its second line contains the statement digest.
- **AC-3** (error path) Given an evidence statement for another release, then the renderer exits 1 with `PAGE-RELEASE-MISMATCH`.
- **AC-4** (determinism) Given AC-1 twice, then outputs are byte-identical.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual matrix render --rows <toml> --evidence <statement.json> --release <tag> --out <dir>` (W1).
- **Data structures:** reuses `qual::matrix::data` types.
- **Algorithm:** group, sort by byte order of clause ID, fixed Markdown template; CSV with RFC 4180 quoting.
- **Errors:** `PAGE-RELEASE-MISMATCH`, `PAGE-PARSE`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::matrix::page`).

## Test fixtures

`fixtures/qual/matrix/page/ok/` synthetic; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::matrix::page`: `qual matrix render --rows <toml> --evidence <statement.json> --release <tag> --out <dir>`. Refuse when the statement's release differs. Write `compliance-matrix-<release>.md` (responsibility statement first, then release and evidence-statement digest, draft/reviewed counts, then rows grouped by designator and edition sorted by clause ID with review status; imported profile controls as links, not restated) and a CSV with the same rows. Deterministic output; no panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.MATRIX.PAGE.

## Open questions

- Publishing the page on the website is `OUTR`'s job; it reads these files.

## Children

_Leaf: none._
