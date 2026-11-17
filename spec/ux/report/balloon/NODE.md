---
id: UX.REPORT.BALLOON
title: Balloon style
level: 3
parent: UX.REPORT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.REPORT-006]
covers: []
depends_on: [UX.REPORT.STYLE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.REPORT.BALLOON: Balloon style

## Purpose

How characteristic balloons look on drawing sheets and in reports: shape, size relative to the number text, numbering format, leader line and arrowhead, placement clearances, and states (normal, nonconforming, not measured) with non-colour cues. It is an original style, not a copy of any incumbent's balloons.

## Scope

**In:** `report/balloon.json` (`balloon-style@1`) and `report/balloon.md`; the placement clearance rules that RPT.BALLOON's placement should honour.
**Out:** balloon placement and numbering logic (RPT.BALLOON); drawing-sheet import (RPT.BALLOON.SHEET).

## Requirements

- **REQ-UX.REPORT.BALLOON-001** The balloon number text shall be at least 8 pt at print scale, and the balloon outline shall leave at least 0.25 × text height of clearance around a three-digit number [unverified].
- **REQ-UX.REPORT.BALLOON-002** The style shall define leader-line weight, arrowhead or dot terminator, and minimum clearance between any two balloons and between a balloon and another balloon's leader line.
- **REQ-UX.REPORT.BALLOON-003** Balloon states shall differ by outline pattern or glyph and text, not only colour.
- **REQ-UX.REPORT.BALLOON-004** The reference mockups of `UX.REPORT.MOCK` shall show no balloon overlapping another balloon's leader line.

## Acceptance criteria

- **AC-1** Given `report/balloon.json`, when `fid-design report-style-check --root design --json` runs (it also validates `balloon-style@1`), then it exits 0.
- **AC-2** (error path) Given fixture `fixtures/ux/report/balloon/small.json` (7 pt number), when the check runs, then it exits 1 with `BALLOON-SIZE`.
- **AC-3** Given the reference mockups, then the design review records no overlap of balloons with leader lines (review item).

## Design

- **Deliverable:** style file and guidance in the package; validation added to `report-style-check` (errors `BALLOON-SCHEMA`, `BALLOON-SIZE`, `BALLOON-CLEARANCE`, `BALLOON-STATE-CUE`).
- **Determinism:** D0.
- **Crate:** `fid-design`, module `report_style` (balloon section).

## Test fixtures

`fixtures/ux/report/balloon/small.json`; synthetic, CC-BY-4.0.

## Build prompt

> Author `report/balloon.json` (`balloon-style@1`: shape, number text role ≥ 8 pt, outline clearance ≥ 0.25 h for three digits, leader weight, terminator, balloon-to-balloon and balloon-to-leader clearances, states normal, nonconforming, not measured with outline pattern or glyph and text) and `report/balloon.md` in your own words with original diagrams. Extend `fid-design report-style-check` to validate it with errors `BALLOON-SCHEMA|BALLOON-SIZE|BALLOON-CLEARANCE|BALLOON-STATE-CUE`. Tests AC-1..AC-2 of spec node UX.REPORT.BALLOON.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- RPT.BALLOON should follow this style (`uses_artifacts: [UX.REPORT.BALLOON]` on the RPT side).

## Children

_Leaf: none._
