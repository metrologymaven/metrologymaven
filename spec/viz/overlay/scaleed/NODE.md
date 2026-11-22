---
id: VIZ.OVERLAY.SCALEED
title: Colour-scale editor
level: 3
parent: VIZ.OVERLAY
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-003, REQ-VIZ.OVERLAY-009]
covers: [GOM.VIZ.04]
depends_on: [VIZ.OVERLAY.CMAP.SPEC, VIZ.OVERLAY.CMAP.REBIN, VIZ.SHELL.DOCK, VIZ.SHELL.PALETTE, GDT.CHAR.RECORD, KRN.NUM.KDEC]
uses_artifacts: [UX.TOKENS.DATAPAL]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# VIZ.OVERLAY.SCALEED: Colour-scale editor

## Purpose

The interactive editor for colour scales: choose stepped or continuous mode, set range and step count (symmetric or asymmetric), edit individual edges as exact decimals in the display unit, derive edges from the tolerance limits of the selected comparison or characteristic, pick a palette from the catalogue, set out-of-range handling, and preview. Every confirmed edit becomes exactly one `view.set_colormap@1` command (owned by `VIZ.OVERLAY.CMAP.SPEC`); the editor persists nothing itself, and undo goes through the journal. The editor owns no registry command, so its CLI, MCP and journal surface is that of `view.set_colormap@1`.

## Scope

**In:**
- Editor panel (range, steps, symmetric toggle, per-edge entry, mode, palette picker limited to `palette-catalogue@1`, out-of-range style)
- "From tolerance" preset: edges derived from the lower and upper limits stored with the selected characteristic's ResultRecord (`gdt.get_result@1`) or the comparison's band-set tolerance markers
- Exact decimal arithmetic for derived edges (`KRN.NUM.KDEC`) and unit conversion from the display unit to metres as exact decimal scaling
- Preview showing the pending spec with the REBIN "updating" state
- Keyboard operation of every control

**Out** (and which node owns it instead):
- The ColorMapSpec model and command (`VIZ.OVERLAY.CMAP.SPEC`); re-binning (`VIZ.OVERLAY.CMAP.REBIN`)
- Panel host and generated form widgets (`VIZ.SHELL.DOCK`, `VIZ.SHELL.PALETTE`)
- Locked status palette rules (`VIZ.OVERLAY.STATUS`)

## Requirements

- **REQ-VIZ.OVERLAY.SCALEED-001** When the user confirms an edit, the editor shall issue exactly one `view.set_colormap@1` command and shall persist nothing else.
- **REQ-VIZ.OVERLAY.SCALEED-002** Edge entry shall accept decimals in the display unit and shall pass them to the command as exact decimal strings in metres, converted by exact decimal scaling without binary rounding.
- **REQ-VIZ.OVERLAY.SCALEED-003** When "from tolerance" is chosen, the editor shall derive edges only from the stored lower and upper limits of the selected characteristic or comparison, splitting the zone into the chosen number of equal steps with exact decimal arithmetic, and shall fail with `NoToleranceLimits` when the selection has none.
- **REQ-VIZ.OVERLAY.SCALEED-004** While the kernel re-bins after an edit, the preview shall show the "updating" state and the previous bins, never a locally computed approximation.
- **REQ-VIZ.OVERLAY.SCALEED-005** Every editor control shall be operable by keyboard, and the panel shall be reachable from the command palette.
- **REQ-VIZ.OVERLAY.SCALEED-006** Symmetric mode shall produce edges that are exact decimal negatives of each other about zero.
- **REQ-VIZ.OVERLAY.SCALEED-007** Users shall be able to drag bin and tolerance handles directly on the viewport colorbar (with keyboard arrow equivalents), showing a clearly labelled `PREVIEW` state while dragging and issuing exactly one `view.set_colormap@1` command on release (or cancelling cleanly on `Esc`).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/devfield-10k`, when the UI harness sets range ±0.5 mm, 5 steps, symmetric, palette `pal.step.cvd@1` and confirms, then exactly one `view.set_colormap@1` entry is journaled with edges `-0.0005, -0.0003, -0.0001, 0.0001, 0.0003, 0.0005` m (as strings) and nothing else is written.
- **AC-2** Given an edge typed as `0.1` mm, then the command receives `0.0001` exactly (string compare).
- **AC-3** Given a comparison point with limits −0.2 mm and +0.3 mm selected and "from tolerance" with 5 steps, then edges are `-0.0002, -0.0001, 0, 0.0001, 0.0002, 0.0003` m; given a selection without limits, then `NoToleranceLimits` is shown and nothing is issued.
- **AC-4** Given a slow binning job, then the preview shows "updating" with the previous colours.
- **AC-5** Given the keyboard-only UI test, then every control (including handle scrubbing) is reachable and operable, and the palette entry opens the editor.
- **AC-6** Given a colorbar handle dragged 20 steps and released, then zero commands are journaled during the drag, exactly one `view.set_colormap@1` is journaled on release, and pressing `Esc` mid-drag journals nothing.

## Design

- **Registry command(s):** none owned; issues `view.set_colormap@1` (W2).
- **Data structures:** `EditorDraft { mode, edges_display: Vec<DecimalString>, unit, palette, out_of_range, symmetric }` (in memory only); `HandleDragState { handle_idx, original_edges, preview_edges }`.
- **Algorithm:** exact decimal scaling by powers of ten for mm and µm, and by the exact factor 0.0254 for inches; equal steps by exact decimal division where the result terminates, else rounding to 9 significant decimals stated in the preview [unverified that 9 is sufficient]; direct colorbar handle dragging snaps to display-unit increments in `EditorDraft` with a `PREVIEW` indicator (`research/viz-ux-sota/UX-INTERACTION-SPEC.md`) and submits one `view.set_colormap@1` on release so the kernel re-bins authoritatively.
- **Errors:** `NoToleranceLimits`, plus validation errors from `view.set_colormap@1` shown inline.
- **Determinism:** D0 (draft to spec is pure).
- **Performance budget:** editor interactions < 16 ms [unverified].
- **Crate:** `fid-overlay` (module `scaleed`).

## Test fixtures

`fixtures/viz/devfield-10k` (shared) with a comparison point carrying limits; CC-BY-4.0.

## Build prompt

> In crate `fid-overlay`, add module `scaleed`: a colour-scale editor panel and direct draggable viewport colorbar handles (mode, range, steps, symmetric, per-edge decimal entry in the display unit, palette picker from `palette-catalogue@1`, out-of-range style, per `research/viz-ux-sota/UX-INTERACTION-SPEC.md`) hosted through the shell panel host and reachable from the command palette. Convert display-unit decimals to metres by exact decimal scaling (`fid-num` kdec); "from tolerance" derives equal steps from the stored limits of the selected characteristic (`gdt.get_result@1`) or comparison band set, else `NoToleranceLimits`. Dragging a handle shows a `PREVIEW` state and commits nothing until release (`Esc` cancels); each confirm or release issues exactly one `view.set_colormap@1`; persist nothing else; while the kernel re-bins, show the `cmap::rebin` updating state, never treating a local preview as values of record. Keyboard-operable. Tests AC-1..AC-6. Rules: no binary rounding of authored decimals; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Tagged P1b because P1a ships fixed default maps; confirm with RPT whether P1a reports need user-edited scales [unverified].
- Finished as a leaf of kind `task` because it owns no registry command; its user-facing command is `view.set_colormap@1` in `VIZ.OVERLAY.CMAP.SPEC`.

## Children

_Leaf: none._
