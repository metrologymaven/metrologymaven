---
id: RPT.DESIGN.UI.CANVAS
title: Designer page canvas, handles and proposal overlay
level: 4
parent: RPT.DESIGN.UI
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.UI-001, REQ-RPT.DESIGN.UI-004, REQ-RPT.DESIGN.UI-005]
covers: [PW.RPT.01]
depends_on: [RPT.DESIGN.PLACE.FRAME, RPT.DESIGN.PLACE.ARRANGE, RPT.DESIGN.PREVIEW.INCR, RPT.DESIGN.PREVIEW.RASTER, VIZ.SHELL.DOCK, VIZ.INBOX]
uses_artifacts: [VIZ.SHELL.SPKRPT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.DESIGN.UI.CANVAS: Designer page canvas, handles and proposal overlay

## Purpose

The central surface of the report designer: the preview pages drawn at the current zoom, with selection, drag, resize and snap handles over each block frame. Every gesture ends in exactly one existing registry command (`report.move_block_layout@1`, the arrange commands) issued through the bus; the canvas keeps only transient view state (zoom, scroll, current selection, drag ghost). When an agent has proposed a layout on an agent branch, the canvas shows it as a proposal overlay for a human to adopt through the Proposal Inbox. It is built only after the D-005 report-designer spike passes.

## Scope

**In:** page view of preview rasters with zoom 25–800 %; selection (click, shift-click, rubber band); move and resize handles with snapping feedback; drag ghost drawn locally, one command on release; overflow flags; proposal overlay of an agent-branch layout diff with "open in Inbox"; the spike gate.

**Out:** palette, property panel, template browser and undo buttons (`RPT.DESIGN.UI.PANEL`); keyboard operation and screen-reader labels (`RPT.DESIGN.UI.KEYS`); placement rules (`RPT.DESIGN.PLACE.*`); adoption itself (`VIZ.INBOX`).

## Requirements

- **REQ-RPT.DESIGN.UI.CANVAS-001** Each completed drag or resize gesture shall issue exactly one `report.move_block_layout@1` command (or one arrange command), and no command while the gesture is in progress.
- **REQ-RPT.DESIGN.UI.CANVAS-002** The canvas shall hold no report state other than view state (zoom, scroll, selection, gesture), so that closing and reopening the designer at the same head shows the same layout.
- **REQ-RPT.DESIGN.UI.CANVAS-003** When the current report has a layout proposal on an agent branch, the canvas shall draw the proposed frames as a distinct overlay labelled as an agent proposal and shall offer adoption only through `VIZ.INBOX`.
- **REQ-RPT.DESIGN.UI.CANVAS-004** Blocks flagged by the overflow check shall be marked on the canvas with a visible marker and the warning text.
- **REQ-RPT.DESIGN.UI.CANVAS-005** The canvas shall not be built beyond the spike until the `VIZ.SHELL.SPKRPT` result record shows the D-005 report-designer criteria as passed.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/four_blocks` in a UI test harness, when B1 is dragged by 20 mm in 40 mouse-move events and released, then exactly one `report.move_block_layout@1` is issued (observed on the bus).
- **AC-2** When the designer is closed and reopened at the same head, then the frames shown equal `show-report` frames.
- **AC-3** Given an agent-branch proposal moving B2, then the overlay shows B2's proposed frame labelled as an agent proposal, and no canvas action adopts it directly.
- **AC-4** Given `overflow_report`, the overflowing block is marked.
- **AC-5** The build reads the spike result record and fails the designer feature flag if it is missing or failed.

## Design

- **Registry commands:** none (issues commands owned by `RPT.DESIGN.PLACE.*`).
- **Structure:** an egui widget in the report-designer surface crate; view state struct `CanvasView { zoom, scroll, selection: BTreeSet<BlockId>, gesture: Option<Gesture> }`; hit-testing in integer page units.
- **Determinism:** N (interactive surface); the commands it issues are deterministic.
- **Performance budget:** 60 fps pan and drag on a 20-page report on the recommended machine [unverified until measured].
- **Crate:** `fid-report-designer`, a layer L3 surface crate (it may depend on VIZ; the `fid-report` domain crate never depends on it).

## Test fixtures

- `fixtures/rpt/design/four_blocks`, `fixtures/rpt/design/overflow_report`, and an agent-branch proposal built by a test builder.

## Build prompt

> In the L3 surface crate `fid-report-designer`, implement the designer page canvas as an egui widget hosted in the workspace shell: show preview rasters with zoom 25–800 %, selection, move and resize handles with snapping feedback, a local drag ghost, and exactly one placement or arrange registry command per completed gesture (none during the gesture). Keep only view state. Mark overflow-flagged blocks. When the report has an agent-branch layout proposal, draw it as a labelled overlay and link to the Proposal Inbox for adoption; never adopt from the canvas. Gate the feature on the passed D-005 report-designer spike record. Tests AC-1 to AC-5 in a UI test harness. CONSTITUTION §8 and C3 (no side doors), C5 (humans adopt agent work). Spec: `spec/rpt/design/ui/canvas/NODE.md`.

## Open questions

- Layering: this child and its siblings are built in an L3 surface crate so that depending on `VIZ.SHELL.*` and `VIZ.INBOX` is layer-legal (CONSTITUTION §2.1); confirm the crate split with maintainers.

## Children

_Leaf: none._
