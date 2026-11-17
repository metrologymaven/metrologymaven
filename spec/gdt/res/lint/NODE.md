---
id: GDT.RES.LINT
title: DMT copyright and ORS-link lint
level: 3
parent: GDT.RES
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-005, REQ-GDT.RES-006]
covers: []
depends_on: [GDT.RES.SCHEMA]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.RES.LINT: DMT copyright and ORS-link lint

## Purpose

A CI gate over the DMT data: it fails when a row could carry reproduced standards text, when a `supported` row has no ORS case, when an ORS link points at a case that does not exist, or when a row claimed as released lacks the two-reviewer sign-off. It turns REQ-GDT.RES-005 and -006 into a check that runs on every DMT change.

## Scope

**In:** lint rules L1–L5 below; machine-readable findings; a CI test target.
**Out:** the schema-level field cap (owned by `GDT.RES.SCHEMA`); the sign-off workflow (owned by `GDT.RES.ROWFLOW`); ORS case content (owned by `UNC.ORS`).

## Requirements

- **REQ-GDT.RES.LINT-001** (L1) The lint shall fail any row whose free-text fields together exceed 280 characters [unverified: threshold to be confirmed by DMT reviewers].
- **REQ-GDT.RES.LINT-002** (L2) The lint shall fail any `ClauseRef` whose clause value is not a clause number pattern (digits and dots, optional letter suffix).
- **REQ-GDT.RES.LINT-003** (L3) The lint shall fail any row with status `supported` that links no ORS case.
- **REQ-GDT.RES.LINT-004** (L4) The lint shall fail any ORS link that names a case ID absent from the ORS index supplied to it.
- **REQ-GDT.RES.LINT-005** (L5) In release mode, the lint shall fail any row whose review state is not `signed_off` with two distinct reviewers.
- **REQ-GDT.RES.LINT-006** Each finding shall name rule, row key and field, and the lint shall exit non-zero if any finding exists.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/dmt/synthetic_table_v1.json` and `fixtures/gdt/lint/ors_index.json`, when the lint runs in draft mode, then it reports zero findings.
- **AC-2** Given fixture `lint/long_text.json` (a row with 300 characters of lorem ipsum), then L1 fires naming that row and field.
- **AC-3** Given `lint/supported_no_ors.json`, then L3 fires; given `lint/dangling_ors.json`, then L4 fires.
- **AC-4** Given `lint/bad_clause.json` (clause value `see text`), then L2 fires.
- **AC-5** Given the synthetic table in release mode (rows in `draft`), then L5 fires for every row and the exit code is non-zero.
- **AC-6** Running the lint twice gives byte-identical finding lists (sorted by key, rule, field).

## Design

- **Registry command(s):** none; a library function `lint(table, ors_index, mode) -> Vec<Finding>` plus a CI test target that loads the committed DMT and ORS index.
- **Data structures:** `Finding { rule: L1..L5, key, field, detail }`; `Mode { Draft, Release }`.
- **Algorithm:** one pass over rows in key order applying L1–L5.
- **Errors:** findings are data; load errors propagate from the schema.
- **Determinism:** D0.
- **Performance budget:** 5 000 rows in < 100 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_lint`).

## Test fixtures

Small synthetic tables (lorem-ipsum filler only, never standards text) and a synthetic ORS index; CC0.

## Build prompt

> In crate `fid-gdt`, module `dmt_lint`, implement `lint(table: &DmtTable, ors_index: &OrsIndex, mode: Mode) -> Vec<Finding>` with rules: L1 total free text per row > 280 chars; L2 clause value not matching `^[0-9]+(\.[0-9]+)*[a-z]?$`; L3 `supported` row with no ORS case; L4 ORS link to an unknown case; L5 (release mode) review state not `signed_off` with two distinct reviewers. Findings sorted by (key, rule, field). Add a CI test target that loads the committed DMT and ORS index and fails on any finding. Tests per the acceptance criteria with synthetic fixtures (lorem ipsum only). Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never include or send standards text anywhere.

## Open questions

- A length cap cannot prove absence of copied text; reviewer sign-off (L5) is the real control. Whether to add a similarity check against a private hash set of standards sentences is a steering-group question (it would require holding the texts).

## Children

_Leaf: none._
