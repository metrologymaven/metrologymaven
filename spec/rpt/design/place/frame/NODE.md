---
id: RPT.DESIGN.PLACE.FRAME
title: Block frames, move, resize and grid snapping
level: 4
parent: RPT.DESIGN.PLACE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-report]
satisfies: [REQ-RPT.DESIGN.PLACE-001, REQ-RPT.DESIGN.PLACE-002, REQ-RPT.DESIGN.PLACE-003, REQ-RPT.DESIGN.PLACE-006]
covers: [PW.RPT.01]
depends_on: [RPT.DESIGN.PAGE, RPT.MODEL.DOC]
side_effect: W2
determinism: D0
cli: [fid report move-block-layout]
mcp: [report_move_block_layout]
journal: [report.move_block_layout@1]
updated: 2026-11-17
---

# RPT.DESIGN.PLACE.FRAME: Block frames, move, resize and grid snapping

## Purpose

Every block on a page has a frame: x, y, width and height as integers in 0.01 mm relative to the page body origin. This leaf defines `BlockFrame`, stores frames alongside the report model, and implements the one command that sets a block's frame (move, resize, or move to another page), with optional snapping to a grid whose pitch is a user setting. It is the only writer of frames apart from align and distribute (`RPT.DESIGN.PLACE.ARRANGE`).

## Scope

**In:** `BlockFrame` type and its storage keyed by block ID; `report.move_block_layout@1` (absolute frame or relative delta, optional target page); grid pitch setting (minimum 0.1 mm, default 1 mm) and snap on or off per command; minimum frame size 1 mm.

**Out:** align, distribute, group and z-order (`RPT.DESIGN.PLACE.ARRANGE`); overflow detection and page flow (`RPT.DESIGN.PLACE.FLOW`); handles and mouse interaction (`RPT.DESIGN.UI.CANVAS`).

## Requirements

- **REQ-RPT.DESIGN.PLACE.FRAME-001** The system shall store each block frame as four `i64` values in 0.01 mm relative to the page body origin from `RPT.DESIGN.PAGE`.
- **REQ-RPT.DESIGN.PLACE.FRAME-002** When snapping is on, the command shall round each frame edge to the nearest multiple of the grid pitch (ties away from zero), and the pitch setting shall reject values below 10 units (0.1 mm) with error `report.layout_invalid`.
- **REQ-RPT.DESIGN.PLACE.FRAME-003** Each `report.move_block_layout@1` call shall be one journaled W2 command, and replaying the journal shall reproduce the same frames.
- **REQ-RPT.DESIGN.PLACE.FRAME-004** If the resulting width or height is below 100 units (1 mm), then the command shall fail with error `report.layout_invalid` and journal nothing.
- **REQ-RPT.DESIGN.PLACE.FRAME-005** The command shall change only the frame (and page membership when moving to another page) and never a block payload, binding, decision or formatter choice.
- **REQ-RPT.DESIGN.PLACE.FRAME-006** A frame may extend outside the body area; the command shall accept it and leave flagging to the overflow check.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/one_page_report` with block B1, when `fid report move-block-layout --block B1 --x 10.04mm --y 20mm --w 80mm --h 40mm --snap --json` runs with pitch 1 mm, then the stored frame is (1000, 2000, 8000, 4000).
- **AC-2** When `--dx 0.35mm` runs with snapping off, then x becomes 1035.
- **AC-3** (error path) When `--w 0.5mm` runs, then it fails with `report.layout_invalid` and the journal length is unchanged; setting the pitch to 0.05 mm also fails.
- **AC-4** Given the report digest of payloads before AC-1, then after AC-1 and AC-2 the payload digest is unchanged.
- **AC-5** (replay) Given the journal after AC-2, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `report.move_block_layout@1(report: ReportId, block: BlockId, to: Absolute{x, y, w, h: Length} | Delta{dx, dy, dw, dh: Length}, page: Option<PageId>, snap: bool) -> BlockFrame` (W2, `--json`, `--dry-run`).
- **Data structures:** `BlockFrame { x: i64, y: i64, w: i64, h: i64 }`; report-level `frames: BTreeMap<BlockId, BlockFrame>`; setting `report.grid_pitch` (user scope, `Length`).
- **Algorithm:** parse lengths with the integer length parser of `RPT.DESIGN.PAGE`; apply; snap each edge (left, top, right, bottom) then recompute w and h; validate minimum size.
- **Errors:** `report.layout_invalid { field, reason }`, `report.not_found`.
- **Determinism:** D0; integer arithmetic only.
- **Performance budget:** under 1 ms per command [unverified until measured].
- **Crate:** `fid-report` (module `layout::frame`).

## Test fixtures

- `fixtures/rpt/design/one_page_report` (shared with `RPT.DESIGN.PAGE`).

## Build prompt

> In crate `fid-report`, module `layout::frame`, implement `BlockFrame { x, y, w, h: i64 }` (0.01 mm, relative to the page body origin) stored per block in a `BTreeMap`, and `report.move_block_layout@1` (W2, one journal event, `--json`, `--dry-run`; CLI `fid report move-block-layout`; MCP name derived) taking an absolute frame or a delta, an optional target page and a snap flag. Snap each edge to the nearest multiple of the user-scope grid pitch (minimum 10 units, default 100; ties away from zero). Reject frames under 100 units wide or high (`report.layout_invalid`). Frames may lie outside the body. Never change payloads, bindings, decisions or formatter choices. Integer arithmetic only, reusing the page module's length parser. Tests AC-1 to AC-5. CONSTITUTION §8 applies. Spec: `spec/rpt/design/place/frame/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
