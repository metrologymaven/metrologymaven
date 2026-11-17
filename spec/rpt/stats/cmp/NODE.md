---
id: RPT.STATS.CMP
title: Part-to-part comparison and piece stepping
level: 3
parent: RPT.STATS
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-010, REQ-RPT.STATS-011]
covers: [GOM.TRD.04, PW.MSP.05]
depends_on: [RPT.STATS.DSET, KRN.NUM]
cli: [fid stats compare-parts, fid stats compare-reference, fid stats step-dataset]
mcp: [stats_compare_parts, stats_compare_reference, stats_step_dataset]
journal: [stats.compare_parts@1, stats.compare_reference@1, stats.step_dataset@1]
updated: 2026-11-17
---

# RPT.STATS.CMP: Part-to-part comparison and piece stepping

## Purpose

Compares results characteristic by characteristic between two parts, or between a part and a reference part, and lets the user step through the pieces of a multi-piece set one at a time. Comparison is a kernel difference with units; stepping only changes which stored results are shown and never recomputes a value of record.

## Scope

**In:**
- Difference per characteristic between two parts (or part and reference part), with unit and the source record IDs
- Handling of characteristics present in only one part
- Piece stepping: a view-state selection of the current piece in a multi-piece data set, exposed to reports and VIZ

**Out** (and which node owns it instead):
- Mesh surface deviation between parts (`SCAN`)
- Showing results on screen (VIZ)

## Requirements

- **REQ-RPT.STATS.CMP-001** The system shall compute, per characteristic, the difference of the two parts' values in the characteristic's unit, and record both source record IDs.
- **REQ-RPT.STATS.CMP-002** If a characteristic has a result in only one of the two parts, then its comparison shall be `MISSING_IN_A` or `MISSING_IN_B` and no number shall be produced.
- **REQ-RPT.STATS.CMP-003** Comparison output shall be ordered by characteristic ID and be bit-identical for the same inputs and algorithm version.
- **REQ-RPT.STATS.CMP-004** When the user steps to another piece of a multi-piece set, the system shall show that piece's stored results and shall not recompute any value of record.
- **REQ-RPT.STATS.CMP-005** Piece stepping shall be a view-state change and not a journaled document change.

## Interfaces

- **Offers:** `stats.compare_parts@1`, `stats.compare_reference@1` (R1); type `PartComparison`; a current-piece selector for reports and VIZ.
- **Consumes:** data sets (`RPT.STATS.DSET`), numerics (`KRN.NUM`). VIZ (L3) consumes the `PieceCursor` this node offers; there is no dependency on VIZ (D-034).

## Decomposition hints

DIFF (comparison), STEP (piece stepping).

## Open questions

- Should piece stepping be recorded on the presentation track (CONSTITUTION §6) for session replay? Assumed yes, as view state held and recorded by VIZ. Stepping is the R0 command `stats.step_dataset@1` plus a pure `PieceCursor` (`RPT.STATS.CMP.STEP`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.STATS.CMP.DIFF | Part-to-part and part-to-reference comparison (leaf) | 001 002 003 | GOM.TRD.04 |
| RPT.STATS.CMP.STEP | Piece stepping in a multi-piece data set (leaf) | 004 005 | PW.MSP.05 |
