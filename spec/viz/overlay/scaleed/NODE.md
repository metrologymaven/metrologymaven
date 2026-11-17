---
id: VIZ.OVERLAY.SCALEED
title: Colour-scale editor
level: 3
parent: VIZ.OVERLAY
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-003, REQ-VIZ.OVERLAY-009]
covers: [GOM.VIZ.04]
depends_on: [VIZ.OVERLAY.CMAP, VIZ.SHELL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.OVERLAY.SCALEED: Colour-scale editor

## Purpose

The interactive editor for colour scales: choose stepped or continuous mode, set range and step count, edit individual edges as exact decimals in the display unit, derive edges from tolerance zones, pick a palette, set out-of-range handling, and preview. Every confirmed edit becomes one `view.set_colormap` command; the editor itself holds no persisted state.

## Scope

**In:**
- Editor panel with range, steps, symmetric and asymmetric scales, per-edge entry
- "From tolerance" presets that derive edges from the characteristic or comparison tolerance
- Palette picker limited to the palette catalogue
- Preview of the pending spec with the "updating" state while the kernel re-bins
- Undo through the journal

**Out** (and which node owns it instead):
- The ColorMapSpec model and command (`VIZ.OVERLAY.CMAP`)
- Panel host and theming (`VIZ.SHELL`)
- Locked status palette enforcement (`VIZ.OVERLAY.STATUS`)

## Requirements

- **REQ-VIZ.OVERLAY.SCALEED-001** When the user confirms an edit, the editor shall issue exactly one `view.set_colormap@1` command and shall persist nothing else.
- **REQ-VIZ.OVERLAY.SCALEED-002** Edge entry shall accept decimals in the display unit and shall pass them to the command as exact decimal strings without binary rounding.
- **REQ-VIZ.OVERLAY.SCALEED-003** When "from tolerance" is chosen, the editor shall derive edges from the kernel-supplied tolerance limits of the selected comparison or characteristic.
- **REQ-VIZ.OVERLAY.SCALEED-004** While the kernel re-bins after an edit, the editor preview shall show the "updating" state rather than a GPU-computed approximation.
- **REQ-VIZ.OVERLAY.SCALEED-005** Every editor control shall be operable by keyboard.

## Interfaces

- **Offers:** the colour-scale editor panel.
- **Consumes:** `VIZ.OVERLAY.CMAP` command and spec; `VIZ.SHELL` panel host and form widgets; tolerance limits from SCAN.CMP or GDT as data.

## Decomposition hints

PANEL, PRESETS (from tolerance, symmetric), EDGES (exact decimal entry), PREVIEW.

## Open questions

- Tagged P1b because P1a ships fixed default maps; confirm with RPT whether P1a reports need user-edited scales [unverified].

## Children

_None yet: status `open`._
