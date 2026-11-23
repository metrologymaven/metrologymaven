---
id: RPT.BALLOON.BLOCK
title: Ballooned-drawing report block
level: 3
parent: RPT.BALLOON
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-009]
covers: []
depends_on: [RPT.BALLOON.REG, RPT.BALLOON.SHEET, RPT.MODEL.DOC, RPT.MODEL.FMT]
uses_artifacts: [UX.REPORT]
side_effect: W2
determinism: D0
cli: [fid report set-drawing-balloon]
mcp: [report_set_drawing_balloon]
journal: [report.set_drawing_balloon@1]
built_in: [fid-report]
updated: 2026-11-22
---

# RPT.BALLOON.BLOCK: Ballooned-drawing report block

## Purpose

A report block kind `drawing` that shows one drawing sheet, or a cropped region of it, with its balloons drawn at their stored sheet coordinates and optionally marked with each characteristic's decision outcome. FAI packages and general reports include it so readers can find every form 3 row on the drawing. The block stores only references (sheet ID, crop, styling options); balloon numbers and positions are resolved from the register at generation time.

## Scope

**In:**
- Block kind `drawing` (version 1) registered with the `RPT.MODEL.DOC` block-kind registry, with its payload schema and validation
- Payload editing command `report.set_drawing_balloon@1`
- A pure resolver that turns the payload plus register and sheet state into a `DrawingBlockContent`: sheet image reference, sheet-to-block transform, and one vector balloon glyph per balloon in the crop region
- Outcome marking (colour role and symbol) through `RPT.MODEL.FMT`
- Balloon glyph geometry taken from the `UX.REPORT` balloon style (shape, number size, leader line)

**Out** (and which node owns it instead):
- Painting the content into PDF bytes or preview pixels (`RPT.OUTPUT.PDF`, `RPT.DESIGN.PREVIEW`)
- Block frame position and size on the page (`RPT.DESIGN.PLACE`)
- Balloon register and sheets (`RPT.BALLOON.REG`, `RPT.BALLOON.SHEET`)
- Decision outcomes (`UNC.DEC`, read through `RPT.MODEL.FMT`)

## Requirements

- **REQ-RPT.BALLOON.BLOCK-001** The resolver shall place each balloon's anchor and label at its stored sheet coordinates mapped by the block's sheet-to-block transform, using integer arithmetic in 0.01 mm units with one half-even rounding per coordinate.
- **REQ-RPT.BALLOON.BLOCK-002** The resolver shall read balloon numbers from the register at the generation journal head, so that a renumbering is shown without editing the block.
- **REQ-RPT.BALLOON.BLOCK-003** When outcome marking is on, each balloon glyph shall carry its characteristic's outcome as both a colour role and a symbol from `RPT.MODEL.FMT`, and a characteristic with no ResultRecord shall be marked `not evaluated`.
- **REQ-RPT.BALLOON.BLOCK-004** Balloon glyphs and leader lines shall be emitted as vector primitives separate from the sheet image, so that a PDF renderer can draw them as vectors.
- **REQ-RPT.BALLOON.BLOCK-005** When a crop region is set, the resolver shall include only balloons whose anchor lies inside the region and shall list the excluded balloon numbers in the content.
- **REQ-RPT.BALLOON.BLOCK-006** If the payload names a sheet that does not exist, or a crop region that is empty or outside the sheet bounds, then `report.set_drawing_balloon@1` shall fail with error `report.invalid_block_payload` and journal nothing.
- **REQ-RPT.BALLOON.BLOCK-007** The scale-to-fit transform shall preserve the sheet's aspect ratio and centre the sheet or crop region in the block frame.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/ballooned_a3` (one A3 landscape sheet of 42000 × 29700 units, ten balloons, a 21000 × 14850 block frame), when the block is resolved, then every glyph anchor equals the stored anchor divided by 2 exactly, and ten glyphs are emitted in ascending number order.
- **AC-2** Given AC-1, when balloon 9 is renumbered to 11 and the block is resolved again at the new head, then the glyph that was 9 shows 11 and the block payload is unchanged.
- **AC-3** Given AC-1 with results C1 CONFORMS, C2 NONCONFORMS and no result for C3, when outcome marking is on, then glyphs 1–3 carry colour roles and symbols for CONFORMS, NONCONFORMS and `not evaluated`.
- **AC-4** Given a crop region covering the left half of the sheet, when resolved, then only balloons with anchors in that half appear and the rest are listed as excluded.
- **AC-5** (error path) When `fid report set-drawing-balloon --block <id> --sheet <unknown> --json` runs, then it fails with `report.invalid_block_payload` and the journal length is unchanged.
- **AC-6** (replay) Given the journal after AC-2, when replayed in exact mode, then the outcome is `IDENTICAL` and the resolved content digest is byte-identical.

## Design

- **Registry command:** `report.set_drawing_balloon@1(report: ReportId, block: BlockId, sheet: SheetId, crop: Option<SheetRect>, fit: ScaleToFit | FixedScale{num: u32, den: u32}, outcome_marking: bool, show_leaders: bool) -> Block` (W2, one journal event; `--dry-run` supported).
- **Data structures:** payload `DrawingPayload { sheet, crop, fit, outcome_marking, show_leaders }` (canonical JSON, kind version 1); `DrawingBlockContent { sheet_image: BlobRef, transform: Affine2Int { scale_num, scale_den, offset_x, offset_y }, glyphs: Vec<BalloonGlyph>, excluded: Vec<u32> }`; `BalloonGlyph { number: u32, anchor: BlockPoint, label: BlockPoint, outcome: Option<OutcomeMark> }`.
- **Algorithm:** fit scale = min(frame_w / src_w, frame_h / src_h) kept as a rational; each coordinate is `(c − crop_origin) × num / den + offset` with half-even rounding; balloons sorted by number.
- **Consumed interfaces:** `SheetInfo` (bounds, image blob) from `RPT.BALLOON.SHEET`; register read API from `RPT.BALLOON.REG`; outcome text and symbol from `RPT.MODEL.FMT`; block-kind trait from `RPT.MODEL.DOC`. Glyph sizes come from the `UX.REPORT` balloon style artifact (read-only; built-in defaults until it is published).
- **Errors:** `report.invalid_block_payload { field }`, `report.not_found`.
- **Determinism:** D0; integers and rationals only, ordered output.
- **Performance budget:** resolve a sheet with 2000 balloons in under 20 ms on reference hardware [unverified until measured].
- **Crate:** `fid-report` (module `balloon::block`).

## Test fixtures

- `fixtures/rpt/balloon/ballooned_a3`: synthetic sheet (blank image, no real drawing content), ten balloons, three stub results; built by a test builder, CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `balloon::block`, implement report block kind `drawing` (version 1) and the command `report.set_drawing_balloon@1` (W2, one journal event, `--json`, `--dry-run`; CLI `fid report set-drawing-balloon`, MCP name derived by the registry).
>
> Register the kind with the `BlockKind` trait of the report document model. Validate the payload: the sheet exists, the crop is non-empty and inside the sheet bounds, else fail with `report.invalid_block_payload { field }` and journal nothing. Write a pure resolver `resolve(payload, register_at_head, sheet_info, outcomes) -> DrawingBlockContent`: rational scale-to-fit preserving aspect ratio and centring; coordinates in 0.01 mm integers, one half-even rounding per coordinate; balloon numbers read from the register at the generation head; glyphs sorted by number; balloons outside the crop listed as excluded; when outcome marking is on, each glyph gets a colour role and a symbol from the formatter layer, and characteristics without a result get `not evaluated`. Glyphs and leaders are vector primitives, never baked into the sheet image. Glyph sizes come from the UX report balloon style file when present, else built-in defaults.
>
> **Acceptance tests:** AC-1 to AC-6 (fixture `fixtures/rpt/balloon/ballooned_a3`), including exact replay giving `IDENTICAL`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); no floating point; `BTreeMap` only, never std `HashMap`; the block never stores numbers or positions copied from the register. Do not implement PDF or raster painting. Spec: `spec/rpt/balloon/block/NODE.md`.

## Open questions

- **Cross-domain:** rendering vector PDF sheet content to pixels (for preview and on-screen display) has no owner (raised in `IO.REG.PLUGIN.DRAW`). The resolver emits a sheet reference; for vector PDF sheets `RPT.OUTPUT.PDF` can embed the source page directly, but preview needs a page renderer.

## Children

_Leaf: none._
