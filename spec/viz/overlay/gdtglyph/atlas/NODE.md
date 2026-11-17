---
id: VIZ.OVERLAY.GDTGLYPH.ATLAS
title: GD&T symbol MSDF atlas from the original drawings
level: 4
parent: VIZ.OVERLAY.GDTGLYPH
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.GDTGLYPH-003, REQ-VIZ.OVERLAY.GDTGLYPH-006]
covers: []
depends_on: []
uses_artifacts: [UX.ICONS.GDT, UX.ICONS.PROV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.GDTGLYPH.ATLAS: GD&T symbol MSDF atlas from the original drawings

## Purpose

Builds, at build time, the multi-channel signed distance field (MSDF) atlas the renderer uses for GD&T symbols, from the project's original SVG drawings published by `UX.ICONS.GDT` (`gdt-symbols@1`). No symbol is drawn or copied here: the atlas is a mechanical conversion, checked back against the drawings' reference outlines, and every entry carries the provenance record of its source drawing so ATTRIBUTION.md stays complete.

## Scope

**In:** a build step that reads `gdt/index.json` and the SVGs; MSDF generation per symbol at a fixed em size (64 px) and distance range (4 px); packing with a deterministic shelf packer in index order; atlas PNG and metrics JSON (`gdt-atlas@1`: symbol ID, dialect flags, UV rect, advance, bearing); an outline round-trip check against `fid-design gdt-compare`; provenance passthrough.
**Out:** drawing the symbols (`UX.ICONS.GDT`); provenance records (`UX.ICONS.PROV`); composing frames (`VIZ.OVERLAY.GDTGLYPH.COMPOSE`); MSDF text rendering in shaders (`VIZ.RENDER`).

## Requirements

- **REQ-VIZ.OVERLAY.GDTGLYPH.ATLAS-001** The atlas shall contain exactly one entry per symbol in `gdt-symbols@1`, carrying its dialect flags, and the build shall fail if any symbol is missing or extra.
- **REQ-VIZ.OVERLAY.GDTGLYPH.ATLAS-002** Every atlas entry shall be generated only from the corresponding `gdt-symbols@1` SVG, and the build shall fail if any entry lacks a provenance record from `UX.ICONS.PROV`.
- **REQ-VIZ.OVERLAY.GDTGLYPH.ATLAS-003** The outline reconstructed from each MSDF entry at its zero level shall pass `fid-design gdt-compare` against the source drawing at h = 64 px.
- **REQ-VIZ.OVERLAY.GDTGLYPH.ATLAS-004** Building the atlas twice from the same inputs shall produce byte-identical PNG and metrics files.

## Acceptance criteria

- **AC-1** Given the design-system package's `gdt/` directory, when the atlas build runs, then the metrics list the same symbol IDs as `gdt/index.json` and each has dialect flags.
- **AC-2** (error path) Given a fixture index with one symbol lacking a provenance record, then the build fails naming it.
- **AC-3** Given every entry, then `gdt-compare` passes on the reconstructed outline.
- **AC-4** Given two builds, then output digests are identical.

## Design

- **Registry command(s):** none; build-time tool `fid-overlay-atlas`.
- **Data structures:** `AtlasMetrics { format: "gdt-atlas@1", em_px: 64, range_px: 4, entries: [{ id, dialects, uv, advance_h, bearing_h, provenance_ref }] }`.
- **Algorithm:** parse SVG paths (a bounded parser of the path subset the UX grid rules allow); compute MSDF with edge colouring per Chlumský's method (thesis, 2015) implemented in-crate; shelf-pack; write PNG with fixed encoder settings.
- **Errors:** `MissingSymbol`, `ExtraSymbol`, `MissingProvenance`, `OutlineMismatch`, `SvgUnsupported`.
- **Determinism:** D0.
- **Performance budget:** full atlas build < 10 s [unverified].
- **Crate:** `fid-overlay-atlas` (build tool) producing assets embedded by `fid-overlay`.

## Test fixtures

The `gdt/` directory of the UX design-system package; a fixture index missing provenance; CC-BY-4.0.

## Build prompt

> Create build tool crate `fid-overlay-atlas`: read `gdt/index.json` (`gdt-symbols@1`) and its SVGs, generate one MSDF per symbol (em 64 px, range 4 px) with an in-crate implementation of multi-channel distance fields (Chlumský 2015), shelf-pack in index order, and write a PNG plus `gdt-atlas@1` metrics (ID, dialect flags, UV, advance, bearing, provenance reference). Fail on missing or extra symbols, missing provenance records (from the icon provenance file) or an outline that fails `fid-design gdt-compare`. Output byte-identical on rebuild. Tests AC-1..AC-4. Rules: clean room — no symbol drawn or copied here; no standards text; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
