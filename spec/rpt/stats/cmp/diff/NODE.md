---
id: RPT.STATS.CMP.DIFF
title: Part-to-part and part-to-reference comparison
level: 4
parent: RPT.STATS.CMP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.CMP-001, REQ-RPT.STATS.CMP-002, REQ-RPT.STATS.CMP-003]
covers: [GOM.TRD.04]
depends_on: [RPT.STATS.DSET.MAT, RPT.STATS.DSET.EXCL, CORE.PRJ.CONT, GDT.CHAR.RECORD, KRN.NUM]
side_effect: [R1, R1]
determinism: D0
cli: [fid stats compare-parts, fid stats compare-reference]
mcp: [stats_compare_parts, stats_compare_reference]
journal: [stats.compare_parts@1, stats.compare_reference@1]
updated: 2026-11-17
---

# RPT.STATS.CMP.DIFF: Part-to-part and part-to-reference comparison

## Purpose

Compares two measured parts characteristic by characteristic: either two pieces of one data set, or one piece against a reference part (a run the user names as the reference, such as a golden part). Each difference is a single kernel subtraction in the characteristic's unit with both source record IDs; characteristics that cannot be compared get a typed status instead of a number.

## Scope

**In:**
- `stats.compare_parts@1` and `stats.compare_reference@1` (R1)
- The `PartComparison` type, statuses and ordering

**Out:**
- Piece stepping (`RPT.STATS.CMP.STEP`)
- Mesh surface deviation between parts (SCAN)
- Showing comparisons on screen (VIZ) or in reports (`RPT.MODEL`)

## Requirements

- **REQ-RPT.STATS.CMP.DIFF-001** When `stats.compare_parts@1` runs with a data set and two piece IDs A and B, the system shall return, per characteristic of the data set, the difference value(A) − value(B) in the characteristic's unit and both source record IDs.
- **REQ-RPT.STATS.CMP.DIFF-002** When `stats.compare_reference@1` runs with a data set, a piece ID and a reference run ID, the system shall return, per characteristic, value(piece) − value(reference) with both record IDs, reading the reference run's ResultRecords with the same automatic exclusions as the data set.
- **REQ-RPT.STATS.CMP.DIFF-003** If a characteristic has a used entry only in A (or the piece), then its status shall be `MISSING_IN_B`; only in B (or the reference), `MISSING_IN_A`; and no number shall be produced.
- **REQ-RPT.STATS.CMP.DIFF-004** If a side has more than one used entry for a characteristic, then the status shall be `MULTIPLE_RESULTS`; if the two units differ, then `UNIT_MISMATCH`; and no number shall be produced.
- **REQ-RPT.STATS.CMP.DIFF-005** The output shall be ordered by characteristic ID and shall record the data-set digest, the algorithm ID `stats.cmp.diff@1.0.0` and the kernel version.
- **REQ-RPT.STATS.CMP.DIFF-006** If a named piece is not in the data set or the reference run does not exist, then the command shall fail with `stats.piece_not_found` or `stats.run_not_found`.
- **REQ-RPT.STATS.CMP.DIFF-007** Results shall be bit-identical on every supported target for the same inputs (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/three_part_run`, when `fid stats compare-parts --dataset <id> --a <piece1> --b <piece2> --json` runs, then five comparisons are returned in characteristic-ID order, each equal to the stored values' difference bit for bit, with both record IDs.
- **AC-2** (missing) Given the characteristic excluded as `NOT_DECIDABLE` on piece 2, when compared, then its status is `MISSING_IN_B` and no number is emitted.
- **AC-3** (reference) Given fixture `fixtures/rpt/stats/golden_part` (a reference run and three production runs), when `fid stats compare-reference --dataset <id> --piece <p> --reference <run>` runs, then each difference is value(piece) − value(reference).
- **AC-4** (error path) Given an unknown piece ID, when the command runs, then it fails with `stats.piece_not_found`.
- **AC-5** (replay) Given a journal that printed AC-1's comparison in a report, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `stats.compare_parts@1(dataset: DataSetId, a: PieceId, b: PieceId) -> PartComparison`, R1.
  - `stats.compare_reference@1(dataset: DataSetId, piece: PieceId, reference: RunId) -> PartComparison`, R1.
- **Data structures:** `PartComparison { dataset_digest, a: PieceRef, b: PieceRef, rows: Vec<CmpRow>, algorithm, kernel_version }`; `CmpRow { char, unit, value: CmpValue::{Diff(f64), MissingInA, MissingInB, MultipleResults, UnitMismatch}, record_a: Option<RecordId>, record_b: Option<RecordId> }`.
- **Algorithm:** materialise (DSET.MAT); select each side's entries by piece ID; for the reference, read the run's ResultRecords (CORE.PRJ, GDT.CHAR.RECORD) and apply `auto_exclusion` (DSET.EXCL via MAT); one IEEE subtraction per row.
- **Errors:** `stats.piece_not_found`, `stats.run_not_found`, `stats.dataset_not_found`.
- **Determinism:** D0.
- **Performance budget:** 10 000 characteristics in under 20 ms after materialisation [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/three_part_run` (shared) and `fixtures/rpt/stats/golden_part` (synthetic project with a reference run), CC-BY-4.0.

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement part comparison: `stats.compare_parts@1(dataset, a: PieceId, b: PieceId)` and `stats.compare_reference@1(dataset, piece: PieceId, reference: RunId)`, both R1, returning `PartComparison`; CLI `fid stats compare-parts`, `fid stats compare-reference`; MCP `stats_compare_parts`, `stats_compare_reference`; `--json`.
>
> Per characteristic of the data set, in characteristic-ID order: difference = value(A) − value(B) (one f64 subtraction) with both record IDs; statuses instead of numbers for `MISSING_IN_A`, `MISSING_IN_B`, `MULTIPLE_RESULTS` and `UNIT_MISMATCH`. The reference run's records get the same automatic exclusions as the data set. Record data-set digest, algorithm ID `stats.cmp.diff@1.0.0` and kernel version. Errors `stats.piece_not_found`, `stats.run_not_found`.
>
> **Tests:** AC-1 to AC-5 above with `fixtures/rpt/stats/three_part_run` and `golden_part`.
>
> **Constraints:** CONSTITUTION §8; C1. Spec: `spec/rpt/stats/cmp/diff/NODE.md`.

## Open questions

- A "reference part" is taken to be any run the user names. A persistent reference designation (stored per part number) would be a W2 command and is not specified yet.
- The difference has no uncertainty attached (it is not a ResultRecord); whether UNC should propagate one is open.

## Children

_Leaf: none._
