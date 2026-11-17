---
id: UX.REPORT.PAGE
title: Report page grids and margins
level: 3
parent: UX.REPORT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.REPORT-005]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.REPORT.PAGE: Report page grids and margins

## Purpose

Page grids for A4 and US Letter in portrait and landscape: margins, column grid, gutters, header and footer bands, and the safe area for binding and printer limits. RPT.DESIGN.PAGE uses them as defaults.

## Scope

**In:** `report/pages.json` (`page-grids@1`); `fid-design page-check`.
**Out:** page geometry model and block placement (RPT.DESIGN.PAGE, RPT.DESIGN.PLACE); fixed form layouts (RPT.FORMS.LAYOUT).

## Requirements

- **REQ-UX.REPORT.PAGE-001** The file shall define grids `a4-portrait`, `a4-landscape`, `letter-portrait` and `letter-landscape` with page size in mm, margins, a 12-column grid, gutter, header band and footer band.
- **REQ-UX.REPORT.PAGE-002** Every margin shall be at least 10 mm [unverified printer limit] and the content width shall equal page width minus margins exactly, within 0.01 mm.
- **REQ-UX.REPORT.PAGE-003** If any grid breaks REQ-001 or REQ-002, then `fid-design page-check` shall fail and name the grid.

## Acceptance criteria

- **AC-1** Given the shipped `pages.json`, when `fid-design page-check --root design --json` runs, then it exits 0 and lists four grids (A4 210 × 297 mm, Letter 215.9 × 279.4 mm).
- **AC-2** (error path) Given fixture `fixtures/ux/report/page/narrow-margin.json` (8 mm), when the check runs, then it exits 1 with `PAGE-MARGIN a4-portrait`.
- **AC-3** (error path) Given `bad-width.json` (columns and gutters do not sum to content width), when the check runs, then it exits 1 with `PAGE-GRID-SUM`.

## Design

- **Command:** `fid-design page-check --root <dir> [--json]`. Tooling, nearest class R0. Lengths as exact decimal strings in mm; arithmetic in integer micrometres.
- **Errors:** `PAGE-SCHEMA`, `PAGE-MISSING`, `PAGE-MARGIN`, `PAGE-GRID-SUM`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `page`.

## Test fixtures

`fixtures/ux/report/page/`: `narrow-margin.json`, `bad-width.json`; CC-BY-4.0.

## Build prompt

> Author `report/pages.json` (`page-grids@1`) with a4-portrait, a4-landscape, letter-portrait, letter-landscape: size, margins (≥ 10 mm), 12-column grid, gutter, header and footer bands, lengths as decimal strings in mm. In the Rust tooling crate `fid-design`, add `fid-design page-check --root <dir> [--json]` computing in integer micrometres: all four grids present, margins ≥ 10 mm, columns × width + gutters = content width = page − margins. Errors `PAGE-SCHEMA|PAGE-MISSING|PAGE-MARGIN|PAGE-GRID-SUM`. Exit 0, 1, 2. Tests AC-1..AC-3 of spec node UX.REPORT.PAGE.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- None.

## Children

_Leaf: none._
