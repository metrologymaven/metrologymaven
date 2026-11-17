---
id: RPT.STATS.CMP.STEP
title: Piece stepping in a multi-piece data set
level: 4
parent: RPT.STATS.CMP
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS.CMP-004, REQ-RPT.STATS.CMP-005]
covers: [PW.MSP.05]
depends_on: [RPT.STATS.DSET.MAT]
side_effect: R0
determinism: D0
cli: [fid stats step-dataset]
mcp: [stats_step_dataset]
journal: [stats.step_dataset@1]
updated: 2026-11-17
---

# RPT.STATS.CMP.STEP: Piece stepping in a multi-piece data set

## Purpose

Lets a user move through the pieces of a multi-piece data set one at a time (first, previous, next, last, or a given piece) and see that piece's stored results. Stepping only reads: it returns the stored ResultRecord values of the selected piece and never recomputes a value of record. The "current piece" is view state held by the caller (the app or VIZ), not a document change, so this leaf offers a read command and a pure cursor type.

## Scope

**In:**
- `PieceCursor` type and its pure navigation function
- `stats.step_dataset@1` (R0): resolve a navigation request to a piece and return its stored results

**Out:**
- Holding the current piece in view state and recording it on the presentation track (VIZ, as consumer of `PieceCursor`)
- Comparison between pieces (`RPT.STATS.CMP.DIFF`)

## Requirements

- **REQ-RPT.STATS.CMP.STEP-001** The piece list of a data set shall be the distinct piece IDs of its used entries, ordered by the position of each piece's first entry in data-set order.
- **REQ-RPT.STATS.CMP.STEP-002** When `stats.step_dataset@1` runs with a data set, a current piece (optional) and a move (`FIRST`, `PREV`, `NEXT`, `LAST` or `PIECE{id}`), the system shall return the resolved piece ID, its position, the piece count and that piece's stored results per characteristic.
- **REQ-RPT.STATS.CMP.STEP-003** `PREV` on the first piece and `NEXT` on the last piece shall return the same piece with flag `AT_BOUND`, not wrap around.
- **REQ-RPT.STATS.CMP.STEP-004** The returned values shall be the stored ResultRecord values and decisions, copied bit for bit, and the command shall not call any statistics or evaluation function.
- **REQ-RPT.STATS.CMP.STEP-005** The command shall be class R0 and shall write nothing to the Record Journal; it shall reach the Activity Log only under the D-031 rules for reads.
- **REQ-RPT.STATS.CMP.STEP-006** If the named piece is not in the data set, then the command shall fail with `stats.piece_not_found`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/three_part_run`, when `fid stats step-dataset --dataset <id> --move first --json` and then `--current <p1> --move next` run, then pieces 1 and 2 are returned with position 1 and 2 of 3 and their stored values bit for bit.
- **AC-2** (bounds) Given the last piece, when `--move next` runs, then the last piece is returned with `AT_BOUND`.
- **AC-3** (no recompute) Given a test build that counts calls into `fid-stats` kernel functions, when stepping through all pieces, then the count is zero and the Record Journal head is unchanged.
- **AC-4** (error path) Given an unknown piece ID, when `--move piece:<id>` runs, then it fails with `stats.piece_not_found`.

## Design

- **Registry command:** `stats.step_dataset@1(dataset: DataSetId, current: Option<PieceId>, mv: Move) -> PieceView`, R0. The journal name is listed for registry completeness; R0 commands are not written to the Record Journal (CONSTITUTION §3, D-031).
- **Data structures:** `PieceCursor { dataset_digest, pieces: Vec<PieceId>, index: u32 }` with `fn step(&self, mv: Move) -> (PieceCursor, AtBound)`; `PieceView { piece, position, count, at_bound, rows: Vec<(CharId, RecordId, StoredValue, Outcome)> }`.
- **Algorithm:** materialise (DSET.MAT), derive the piece list (REQ-001), apply the move, copy the selected piece's entries.
- **Errors:** `stats.piece_not_found`, `stats.dataset_not_found`.
- **Determinism:** D0 (no arithmetic).
- **Performance budget:** a step under 10 ms with a cached materialised data set [unverified until measured].
- **Crate:** `fid-stats-cmd`.

## Test fixtures

- `fixtures/rpt/stats/three_part_run` (shared).

## Build prompt

> In crate `fid-stats-cmd` (layer L2, RPT), implement piece stepping: a pure `PieceCursor { dataset_digest, pieces, index }` with `step(Move) -> (PieceCursor, AtBound)` (moves `FIRST`, `PREV`, `NEXT`, `LAST`, `PIECE{id}`; no wrap-around), and the command `stats.step_dataset@1(dataset, current?, move) -> PieceView` (R0; CLI `fid stats step-dataset`, MCP `stats_step_dataset`, `--json`). The piece list is the distinct piece IDs of used entries in order of first appearance. Return the stored values and decisions of the selected piece bit for bit; never call a statistics or evaluation function; write nothing to the Record Journal. Unknown piece -> `stats.piece_not_found`.
>
> **Tests:** AC-1 to AC-4 above with `fixtures/rpt/stats/three_part_run`.
>
> **Constraints:** CONSTITUTION §8; §3 read rules (D-031). Spec: `spec/rpt/stats/cmp/step/NODE.md`.

## Open questions

- Recording the current piece on the presentation track (CONSTITUTION §6) is VIZ's job; VIZ (L3) consumes `PieceCursor`. The previous dependency of `RPT.STATS.CMP` on `VIZ.VIEW` was removed because RPT (L2) may not depend on L3 (D-034).

## Children

_Leaf: none._
