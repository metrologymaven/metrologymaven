---
id: IND.SHEET.SPRING
title: Springback assessment
level: 3
parent: IND.SHEET
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.SHEET-006]
covers: [GOM.SPE.02]
depends_on: [IND.SHEET.CTX, SCAN, ALN]
cli: [fid pack evaluate-sheet-springback]
mcp: [pack_evaluate_sheet_springback]
journal: [pack.evaluate_sheet_springback@1]
updated: 2026-11-16
---

# IND.SHEET.SPRING: Springback assessment

## Purpose

Assesses how a formed panel deviates from nominal after forming: surface deviation under a named clamping alignment, summarised per declared region (for example flange, wall, base), so tool makers see where and how much the part springs back.

## Scope

**In:**
- Region definitions on the nominal (face sets or surface patches)
- Deviation statistics per region (minimum, maximum, mean, standard deviation, percentage within tolerance) under a named, journaled clamping or RPS alignment
- Recording the alignment in every ResultRecord
- `pack.evaluate_sheet_springback@1` (R1)

**Out** (and which node owns it instead):
- Surface deviation computation (owned by `SCAN`)
- Clamping and RPS alignments (owned by `ALN`)
- Die compensation or CAD morphing from springback results (not covered pending decision D-022)

## Requirements

- **REQ-IND.SHEET.SPRING-001** When springback is assessed, the pack shall report minimum, maximum, mean and standard deviation of normal deviation and the share of points within tolerance for each declared region.
- **REQ-IND.SHEET.SPRING-002** The assessment shall run under a named, journaled alignment, and every ResultRecord shall record the alignment's ID and version.
- **REQ-IND.SHEET.SPRING-003** If a declared region contains fewer deviation points than the declared minimum, then the pack shall report that region as `NOT_DECIDABLE`.
- **REQ-IND.SHEET.SPRING-004** Region statistics shall be computed with fixed-order reductions so results are bit-identical across runs on one platform.

## Interfaces

- **Offers:** `pack.evaluate_sheet_springback@1` (R1); `SpringbackRegionResult`.
- **Consumes:** SCAN surface deviations; ALN named alignments; `IND.SHEET.CTX`.

## Decomposition hints

REGION (region definitions), STATS (per-region statistics). Possibly one leaf.

## Open questions

- Whether springback should also be expressed as angular deviation of flanks (common in practice [unverified]); if so, it needs flank plane fits per region.

## Children

_None yet: status `open`._
