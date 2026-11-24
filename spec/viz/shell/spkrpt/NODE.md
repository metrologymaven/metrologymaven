---
id: VIZ.SHELL.SPKRPT
title: Toolkit spike - report designer canvas
level: 3
parent: VIZ.SHELL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-003, REQ-VIZ.SHELL-004]
covers: []
depends_on: []
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.SHELL.SPKRPT: Toolkit spike - report designer canvas

## Purpose

The third D-005 spike: a WYSIWYG page canvas in egui of the kind the report designer needs (bands, text blocks, tables, an embedded 3D view snapshot, drag, resize, snap, zoom, multi-page, rich text with fallback fonts, undo), measured against fixed criteria. It tests toolkit feasibility only; the report designer itself belongs to RPT.

## Scope

**In:** a prototype canvas with A4 and Letter pages, 200 blocks across 5 pages, drag, resize, snap to grid and guides, zoom 25–400 %, text editing with IME input, an embedded wgpu-rendered 3D frame, undo and redo, keyboard move and resize, AccessKit exposure; benchmark harness; results report.
**Out:** the production report designer (`RPT.DESIGN`); the other spikes; the decision (maintainers, D-005).

## Requirements

- **REQ-VIZ.SHELL.SPKRPT-001** The spike shall render 5 pages with 200 blocks, including tables and one live 3D frame drawn by wgpu, at zoom levels from 25 % to 400 %.
- **REQ-VIZ.SHELL.SPKRPT-002** The spike shall pass only if p95 frame time stays under 33 ms on the H1 profile while dragging a block, zooming and typing into a text block [unverified].
- **REQ-VIZ.SHELL.SPKRPT-003** Moving, resizing, selecting and editing blocks shall be possible by keyboard alone.
- **REQ-VIZ.SHELL.SPKRPT-004** Text blocks shall accept IME composition (one CJK input method) and render with fallback fonts.
- **REQ-VIZ.SHELL.SPKRPT-005** The spike shall verify screen-reader announcement of block type, name and position, and shall publish a results report with pass or fail per criterion.

## Acceptance criteria

- **AC-1** Given generated fixture `fixtures/viz/report-canvas-200` (seed 13), when the harness drags a block across a page for 10 s on H1, then p95 frame time is recorded and judged against 33 ms.
- **AC-2** Given the same fixture, when the harness zooms from 25 % to 400 % and types 200 characters, then p95 frame time is recorded and judged.
- **AC-3** Given the scripted keyboard sequence, then a block is moved by 10 grid steps and resized without pointer input.
- **AC-4** Given a Japanese IME on Windows, when "そくてい" is composed and committed, then the block contains the committed text (manual step recorded in the report).
- **AC-5** Given any failure, then the report states "fail" with the measured value and points to D-005 option B (or a web panel for the designer only, per the concept doc's migration triggers) for this screen.

## Design

- **Registry command(s):** none (prototype).
- **Data structures:** synthetic `Page { blocks: [Block { kind, rect_mm, content }] }`.
- **Algorithm:** canvas in millimetre space with a zoom transform; hit testing on the CPU; the 3D frame drawn through the same wgpu paint callback as the viewport.
- **Errors:** not applicable.
- **Determinism:** N (timings observed).
- **Performance budget:** p95 < 33 ms on H1 [unverified].
- **Crate:** `fid-spike-ui` (example `rptcanvas`; throwaway).

## Test fixtures

`fixtures/viz/report-canvas-200`: generated (seed 13), in-repo.

## Build prompt

> In throwaway crate `fid-spike-ui`, add example `rptcanvas`: an egui + winit + wgpu page canvas with 5 A4 pages and 200 generated blocks (seed 13) including tables and one live wgpu 3D frame; drag, resize, snap to grid and guides, zoom 25–400 %, multi-page scroll, text editing with IME composition and fallback fonts, undo/redo, keyboard move and resize, AccessKit exposure of blocks. Benchmark p95 frame time while dragging, zooming and typing on the H1 profile and write `REPORT.md` with pass/fail per criterion (p95 < 33 ms; keyboard completeness; IME; screen-reader announcements), versions and limitations. The report is input to the maintainers' confirmation of D-005; do not adopt it. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; never a dependency of product crates.

## Open questions

- RPT.DESIGN owns the real designer; its maintainers should review the spike criteria.
- Build status (not marked `built_in`): the prototype, harness and report renderer are in `fid-spike-ui` (module `rpt`, example `rptcanvas`, fixture `fixtures/viz/report-canvas-200` from `make_canvas.py`). AC-3 (keyboard move and resize, on the model) and AC-5 (fail wording in the report) are tested in CI. Open: AC-1 and AC-2 need a timed run on the PLAT H1 machine with a GPU; AC-4 needs the manual Japanese IME check on Windows (during that run, also record whether Escape pressed during IME composition cancels the composition without ending the edit; the report's IME log has a line for it); the screen-reader check of REQ-VIZ.SHELL.SPKRPT-005 is manual too. No timings exist yet.

## Children

_Leaf: none._
