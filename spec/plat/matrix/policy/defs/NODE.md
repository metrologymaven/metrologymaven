---
id: PLAT.MATRIX.POLICY.DEFS
title: Tier definitions document and its lint
level: 4
parent: PLAT.MATRIX.POLICY
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
satisfies: [REQ-PLAT.MATRIX.POLICY-001]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.MATRIX.POLICY.DEFS: Tier definitions

## Purpose

Tells users and contributors what each tier means in practice: what CI runs, whether a failure blocks a release, and what support response to expect. A small lint keeps the document's table complete.

## Scope

**In:**
- `docs/platform/tiers.md` with a table: tier, CI obligation, release-blocking, support response, how to promote or demote
- `xtask plat-matrix lint-tiers`

**Out** (and which node owns it instead):
- The rows (`PLAT.MATRIX.POLICY.ROWS`), EOL rules (`PLAT.MATRIX.POLICY.EOL`)

## Requirements

- **REQ-PLAT.MATRIX.POLICY.DEFS-001** The document shall contain one table row each for Tier 1, Tier 2 and Tier 3 with columns CI obligation, Release-blocking, Support response and Promotion/demotion.
- **REQ-PLAT.MATRIX.POLICY.DEFS-002** The content shall state: Tier 1 builds and runs the full test suite on every release and a failure blocks; Tier 2 builds on every release and a failure is noted but does not block; Tier 3 has no CI obligation and is community-maintained.
- **REQ-PLAT.MATRIX.POLICY.DEFS-003** The lint shall fail if a tier row or column is missing or a cell is empty.

## Acceptance criteria

- **AC-1** Given the shipped document, when `cargo xtask plat-matrix lint-tiers` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/plat/matrix/tiers-no-tier2.md`, then `MissingTier { tier: tier2 }`.
- **AC-3** (error path) Given `tiers-empty-cell.md`, then `EmptyCell { tier, column }`.

## Design

- **Registry command(s):** none; docs plus `xtask plat-matrix lint-tiers`.
- **Algorithm:** find the first Markdown table after `## Tiers`; check header and rows.
- **Errors:** `NoTable`, `MissingTier`, `MissingColumn`, `EmptyCell`.
- **Determinism:** D0.
- **Crate:** `xtask` (module `plat_matrix_tiers`).

## Test fixtures

Markdown fixtures under `fixtures/plat/matrix/`. Licensed with the code.

## Build prompt

> Write `docs/platform/tiers.md` with a `## Tiers` table (Tier, CI obligation, Release-blocking, Support response, Promotion/demotion) for Tier 1 (full suite every release, blocks, maintainers respond to regressions before release), Tier 2 (build every release, failure noted in release notes, best-effort fixes), Tier 3 (no CI, community-maintained). Reference the promotion rule (two consecutive green release candidates) and the end-of-support demotion rule. Add `xtask plat-matrix lint-tiers`. Tests: AC-1..AC-3. Constraints: CONSTITUTION §8. Commit: `docs(platform): tier definitions`.

## Open questions

- Support response times are a maintainer commitment; the document states them qualitatively until maintainers set numbers.

## Children

_Leaf: none._
