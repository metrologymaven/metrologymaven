---
id: QUAL.THREAT.REVIEW
title: Per-minor-release threat-model review record
level: 3
parent: QUAL.THREAT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.THREAT-004]
covers: []
depends_on: [QUAL.THREAT.REG]
uses_artifacts: [ECO.REL.PROC.GATES]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.THREAT.REVIEW: Per-minor-release threat-model review record

## Purpose

Produces and checks the review record that must exist for each `MAJOR.MINOR` before it is tagged: who reviewed, which areas changed since the previous review, and which threats were added, changed or closed. The added, changed and closed lists are computed from the register at the two review points, so the reviewer confirms rather than retypes them. `ECO.REL.PROC.GATES` (gate `qual-threat`, `--pre-tag`) blocks the tag when the record is missing.

## Scope

**In:**
- Record file `threat/reviews/<MAJOR.MINOR>.toml` (path proposed by `ECO.REL.PROC.GATES` and adopted here), schema `qual-threat-review/1`
- `qual threat review-draft`: computes the register diff between the previous review's register digest and the current register, and the changed coverage areas
- `qual threat review-check`: validates a record
- Reviewer rule from D-003: the reviewer is a maintainer other than the author of the record's PR

**Out** (and which node owns it instead):
- Register schema and diff library (owned by `QUAL.THREAT.REG`)
- Blocking the tag (owned by `ECO.REL.PROC.GATES`)

## Requirements

- **REQ-QUAL.THREAT.REVIEW-001** The review record shall name the `MAJOR.MINOR`, the reviewer, the PR author, the previous review's version and register SHA-256, the current register SHA-256, the changed coverage areas and the threat IDs added, changed and closed.
- **REQ-QUAL.THREAT.REVIEW-002** When `review-draft` runs, the tool shall compute the added, changed and closed threat IDs and the changed areas from the two register versions.
- **REQ-QUAL.THREAT.REVIEW-003** If the record's lists differ from the lists recomputed from the two registers, or its current register SHA-256 differs from the committed register, then `review-check` shall fail and name the field.
- **REQ-QUAL.THREAT.REVIEW-004** If the reviewer equals the PR author or is not in the maintainer list, then `review-check` shall fail.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/threat/review/ok/` (registers at 1.3 and 1.4 with one added, one changed and one closed threat; maintainers list), when `cargo xtask qual threat review-draft --minor 1.4 --previous reviews/1.3.toml --register register.toml --out out/` runs, then the draft equals `expected-1.4.toml` except for the reviewer field.
- **AC-2** Given the completed record, when `qual threat review-check --record reviews/1.4.toml --register register.toml --previous-register register-1.3.toml --maintainers maintainers.toml --pr-author alice` runs, then it exits 0.
- **AC-3** (error path) Given a record whose `closed` list omits the closed threat, then the check exits 1 naming `closed`.
- **AC-4** (error path) Given reviewer `alice` and PR author `alice`, then the check exits 1.
- **AC-5** (joint) Given the `ECO.REL.PROC.GATES` fixture `threat-no-review/` with AC-2's record added at `threat/reviews/1.5.toml` renamed to 1.5, then gate `qual-threat` passes for `v1.5.0-beta.1`.

## Design

- **Registry command(s):** none; repository tools `cargo xtask qual threat review-draft ...` (W1) and `cargo xtask qual threat review-check ...` (R0).
- **Data structures:** `Review { schema, minor, reviewer, pr_author, previous: { minor, register_sha256 }, register_sha256, changed_areas, added, changed, closed, notes: Untrusted<String> }`.
- **Algorithm:** diff entries by ID; an entry is changed when any field other than `owner` and `issue` differs; areas changed are the union of areas of added, changed and closed entries.
- **Errors:** `REVIEW-PARSE`, `REVIEW-MISMATCH(field)`, `REVIEW-REVIEWER`. Exit 0/1/2.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::threat`, file `review.rs`).

## Test fixtures

`fixtures/qual/threat/review/ok/` with two synthetic register versions and a maintainers list; CC-BY-4.0.

## Build prompt

> In `fid-xtask` module `qual::threat`, add `qual threat review-draft --minor <M.m> --previous <toml> --register <toml> --out <dir>` and `qual threat review-check --record <toml> --register <toml> --previous-register <toml> --maintainers <toml> --pr-author <login>`. Record schema `qual-threat-review/1` stored at `threat/reviews/<MAJOR.MINOR>.toml`: minor, reviewer, pr_author, previous {minor, register_sha256}, register_sha256, changed_areas, added, changed, closed, notes (untrusted). Draft computes the diff by threat ID (changed = any field other than owner or issue differs). Check recomputes and fails on any mismatch, on reviewer equal to PR author, or on a reviewer not in the maintainers list. Use the register parser from `qual::threat`. Canonical TOML ordering; no panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.THREAT.REVIEW.

## Open questions

- External reviewers (non-maintainers) are not provided for; D-003 requires a maintainer other than the author.

## Children

_Leaf: none._
