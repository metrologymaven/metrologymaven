---
id: RPT.DESIGN.PLACE.ARRANGE
title: Align, distribute, group and z-order
level: 4
parent: RPT.DESIGN.PLACE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.PLACE-003, REQ-RPT.DESIGN.PLACE-006]
covers: [PW.RPT.01]
depends_on: [RPT.DESIGN.PLACE.FRAME]
side_effect: [W2, W2, W2]
determinism: D0
cli: [fid report align-block-layout, fid report group-block-layout, fid report order-block-layout]
mcp: [report_align_block_layout, report_group_block_layout, report_order_block_layout]
journal: [report.align_block_layout@1, report.group_block_layout@1, report.order_block_layout@1]
updated: 2026-11-17
---

# RPT.DESIGN.PLACE.ARRANGE: Align, distribute, group and z-order

## Purpose

The arrangement operations of a page designer: align a selection of blocks to an edge or centre, distribute them with equal gaps, group and ungroup them so they move together, and change their stacking order. Each is a W2 command over block frames and page structure, never over content.

## Scope

**In:** `report.align_block_layout@1` (modes left, right, top, bottom, centre-horizontal, centre-vertical, distribute-horizontal, distribute-vertical; reference selection bounds or page body); `report.group_block_layout@1` (group, ungroup); `report.order_block_layout@1` (front, back, forward, backward); group membership and z-order storage per page.

**Out:** single-block move and resize, snapping (`RPT.DESIGN.PLACE.FRAME`); overflow and flow (`RPT.DESIGN.PLACE.FLOW`).

## Requirements

- **REQ-RPT.DESIGN.PLACE.ARRANGE-001** When aligning, the system shall move each selected frame so the chosen edge or centre equals that of the reference, computing centres with integer floor division.
- **REQ-RPT.DESIGN.PLACE.ARRANGE-002** When distributing n ≥ 3 blocks, the system shall keep the outermost two fixed and set equal gaps, giving any remainder units one each to the first gaps in order.
- **REQ-RPT.DESIGN.PLACE.ARRANGE-003** A group shall move as one in `report.move_block_layout@1` delta moves, and grouping blocks on different pages shall fail with error `report.layout_invalid`.
- **REQ-RPT.DESIGN.PLACE.ARRANGE-004** Z-order shall be a total order per page, and each order change shall be one journaled W2 command.
- **REQ-RPT.DESIGN.PLACE.ARRANGE-005** Arrangement commands shall not change any payload, binding, decision or formatter choice.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/four_blocks` (four blocks at x = 1000, 3100, 5000, 9000, width 1000), when `fid report align-block-layout --blocks B1,B2,B3,B4 --mode left --json` runs, then all x are 1000.
- **AC-2** Given the original fixture, when `--mode distribute-horizontal` runs, then B1 and B4 stay put and the three gaps are 1667, 1667 and 1666 units.
- **AC-3** When B1 and B2 are grouped and a delta move of +500 x is applied to B1, then B2 also moves by 500.
- **AC-4** (error path) Grouping blocks on two pages fails with `report.layout_invalid` and journals nothing.
- **AC-5** When B4 is sent to back, then the page's z-order lists B4 first; replaying the journal gives `IDENTICAL`.

## Design

- **Registry commands:** `report.align_block_layout@1(report, blocks: Vec<BlockId>, mode: AlignMode, reference: Selection | Body) -> Vec<BlockFrame>`; `report.group_block_layout@1(report, action: Group{blocks} | Ungroup{group: GroupId}) -> Option<GroupId>`; `report.order_block_layout@1(report, block, op: Front | Back | Forward | Backward) -> Vec<BlockId>`. All W2, `--json`, `--dry-run`.
- **Data structures:** per page `z_order: Vec<BlockId>`, `groups: BTreeMap<GroupId, BTreeSet<BlockId>>` (group IDs UUIDv7, journaled).
- **Algorithm:** distribute sorts by current position then block ID; all arithmetic in `i64`.
- **Errors:** `report.layout_invalid { reason }`, `report.not_found`.
- **Determinism:** D0.
- **Performance budget:** under 2 ms for 200 selected blocks [unverified until measured].
- **Crate:** `fid-report` (module `layout::arrange`).

## Test fixtures

- `fixtures/rpt/design/four_blocks`: built by a test builder with `RPT.MODEL.DOC` commands; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `layout::arrange`, implement `report.align_block_layout@1`, `report.group_block_layout@1` and `report.order_block_layout@1` (all W2, one journal event each, `--json`, `--dry-run`; CLI `fid report align-block-layout`, `group-block-layout`, `order-block-layout`; MCP names derived). Align to an edge or centre of the selection bounds or page body (integer floor division for centres); distribute n ≥ 3 blocks with outermost fixed and equal gaps, remainder units given to the first gaps; groups on one page only, moved together by delta moves; z-order a total order per page. Integer arithmetic, ordered collections, never change payloads or bindings. Tests AC-1 to AC-5 (fixture `fixtures/rpt/design/four_blocks`). CONSTITUTION §8 applies. Spec: `spec/rpt/design/place/arrange/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
