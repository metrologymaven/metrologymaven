---
id: VIZ.RENDER.GLYPH.PATH
title: Measurement-path polylines by move type
level: 4
parent: VIZ.RENDER.GLYPH
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.GLYPH-005]
covers: []
depends_on: [VIZ.RENDER.SCENE.PROJ, VIZ.RENDER.FRAME.GRAPH]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.GLYPH.PATH: Measurement-path polylines by move type

## Purpose

Draw a measurement program's path as polylines whose rapid, approach, touch, scan and retract segments differ by both colour and line pattern, so the path stays readable for colour-blind users and in greyscale prints, with each segment pickable back to its program step.

## Scope

**In:** `PathPolyline` input (data only: f64 vertices, move kind, step ID per segment); screen-space line pass with dash patterns; per-segment derived render IDs; step highlighting.
**Out:** generating or importing paths (`CMM.PATH`, `IO.DMIS`); ghost probes (`VIZ.RENDER.GLYPH.STYLUS`).

## Requirements

- **REQ-VIZ.RENDER.GLYPH.PATH-001** Each move kind (rapid, approach, touch, scan, retract) shall have a distinct colour token and a distinct dash pattern, so that any two kinds differ in pattern even when rendered in greyscale.
- **REQ-VIZ.RENDER.GLYPH.PATH-002** Every path segment shall write a derived render ID that maps to its program step ID.
- **REQ-VIZ.RENDER.GLYPH.PATH-003** Path line width shall be constant in screen pixels (default 2 px) regardless of zoom.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/path-5kinds` (one segment of each kind), when rendered headlessly and converted to greyscale, then the five segments' dash run-length signatures are pairwise distinct.
- **AC-2** Given a pixel on the touch segment, then its derived render ID maps to the fixture's step 3.
- **AC-3** Given zoom factors 0.1 and 10, then sampled line widths are 2 px ± 0.5 px.

## Design

- **Registry command(s):** none.
- **Data structures:** `PathPolyline { vertices_m: Vec<[f64;3]>, segments: Vec<{ kind: MoveKind, step: StepId, range }> }`; `MoveKind` → (colour token, dash pattern) table.
- **Algorithm:** screen-space expanded quads; dash from accumulated screen length; colour tokens resolved from the theme.
- **Errors:** `MalformedPath`.
- **Determinism:** N.
- **Performance budget:** 10^5 segments under 2 ms per frame [unverified].
- **Crate:** `fid-render` (module `glyph::path`).

## Test fixtures

`fixtures/viz/path-5kinds`: project-authored, CC0.

## Build prompt

> In crate `fid-render`, add module `glyph::path`: draw data-only `PathPolyline`s (f64 vertices; segments with move kind and step ID) as constant-width (default 2 px) screen-space lines; each move kind has a distinct colour token and a distinct dash pattern (distinguishable in greyscale); each segment writes a derived render ID mapping to its step. Tests AC-1..AC-3 with `fixtures/viz/path-5kinds`. Rules: CONSTITUTION §8; C2.

## Open questions

- None.

## Children

_Leaf: none._
