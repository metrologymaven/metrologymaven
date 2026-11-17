---
id: VIZ.OVERLAY.GDTGLYPH.PMIQA
title: Graphic PMI quality-check overlay
level: 4
parent: VIZ.OVERLAY.GDTGLYPH
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.GDTGLYPH-005]
covers: []
depends_on: [VIZ.OVERLAY.GDTGLYPH.PLACE, GDT.PMI.LINK, IO.CAD.PMI.READ]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.GDTGLYPH.PMIQA: Graphic PMI quality-check overlay

## Purpose

An optional overlay for checking CAD models: it draws the graphic (presentation) PMI polylines from the imported model next to the frames composed from semantics, and marks every frame whose semantic content disagrees with the PMI entity it was created from. The comparison uses the characteristic's `PmiLink` (source PMI entity) and the semantic and graphic classification of `IO.CAD.PMI.READ`; it is a display aid and changes nothing.

## Scope

**In:** toggle state (view-local); drawing graphic PMI polylines in a muted style; per-frame agreement check: tolerance text, symbol set and datum labels of the composed frame against the linked PMI entity's semantic fields; mismatch marker with the differing fields.
**Out:** PMI import and classification (`IO.CAD.PMI.READ`); the link (`GDT.PMI.LINK`); re-import diffs (`GDT.PMI.DIFF`).

## Requirements

- **REQ-VIZ.OVERLAY.GDTGLYPH.PMIQA-001** When the overlay is on, every frame whose characteristic has a `PmiLink` and whose symbol, tolerance text or datum labels differ from the linked PMI entity shall carry a mismatch marker listing the differing fields.
- **REQ-VIZ.OVERLAY.GDTGLYPH.PMIQA-002** Frames without a `PmiLink` shall be marked `no_pmi_source`, not as matching.
- **REQ-VIZ.OVERLAY.GDTGLYPH.PMIQA-003** Turning the overlay on or off shall change no document state and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/gdt/pmiqa` (a STEP AP242 test model with 12 PMI entities, one characteristic edited to a different tolerance), when the overlay is on, then exactly that frame is marked with field `tolerance`.
- **AC-2** Given a characteristic created by hand, then it shows `no_pmi_source`.
- **AC-3** Given toggling, then the journal and project database are unchanged.

## Design

- **Registry command(s):** none.
- **Algorithm:** for each placed frame with a link, load the PMI entity's semantic fields from the import result, compare normalised strings and symbol IDs, draw polylines from the graphic PMI.
- **Errors:** `PmiSourceUnavailable` (model not loaded; overlay shows a notice).
- **Determinism:** D0 for the comparison.
- **Performance budget:** 1,000 frames compared < 50 ms [unverified].
- **Crate:** `fid-overlay` (module `gdt::pmiqa`).

## Test fixtures

`fixtures/viz/gdt/pmiqa`: derived from a public NIST MBE PMI test model [unverified licence; check before use] with one edited characteristic.

## Build prompt

> In crate `fid-overlay`, add module `gdt::pmiqa`: an optional, view-local overlay that draws graphic PMI polylines from the CAD import and marks each placed frame whose symbol, tolerance text or datum labels differ from the PMI entity named by its `PmiLink` (listing the fields), marking frames without a link `no_pmi_source`. No document change, nothing journaled. Tests AC-1..AC-3. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- The exact graphic PMI output of `IO.CAD.PMI.READ` (polyline form) is assumed; confirm at build time.

## Children

_Leaf: none._
