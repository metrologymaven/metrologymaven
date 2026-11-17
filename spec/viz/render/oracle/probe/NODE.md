---
id: VIZ.RENDER.ORACLE.PROBE
title: Pixel-probe harness
level: 4
parent: VIZ.RENDER.ORACLE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.ORACLE-005]
covers: []
depends_on: [VIZ.RENDER.HEADLESS, VIZ.RENDER.FRAME.GRAPH]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.ORACLE.PROBE: Pixel-probe harness

## Purpose

Answer "what colour is drawn for this element?" in tests: given a render ID and a point or primitive index, find a pixel whose ID-buffer entry matches and return its colour, or report that the element is not visible. The legend-conformance tests in `VIZ.OVERLAY` and the LUT exactness tests in RENDER use it.

## Scope

**In:** probe API over headless colour and ID images; selection of the most interior matching pixel; not-visible reporting.
**Out:** legend assertions (`VIZ.OVERLAY.LEGEND`); image production (`VIZ.RENDER.HEADLESS`).

## Requirements

- **REQ-VIZ.RENDER.ORACLE.PROBE-001** Given a render ID and an element index, the probe shall return the colour of a pixel whose ID entry matches both, choosing the pixel farthest from any non-matching pixel, ties to the lowest row then column.
- **REQ-VIZ.RENDER.ORACLE.PROBE-002** If no pixel matches, then the probe shall return `NotVisible` rather than a colour.
- **REQ-VIZ.RENDER.ORACLE.PROBE-003** The probe shall accept batches of up to 10^5 queries per image.

## Acceptance criteria

- **AC-1** Given `fixtures/viz/points-bins` rendered headlessly, when probing all 1 000 points, then each returns a colour or `NotVisible`, and visible ones equal their LUT entries (with EDL off).
- **AC-2** Given a point hidden behind a plane, then the probe returns `NotVisible`.
- **AC-3** Given the same image twice, then the chosen pixels are identical.

## Design

- **Registry command(s):** none; test API `probe(image: &OffscreenImage, ids: &IdImage, q: &[(RenderId, u32)]) -> Vec<ProbeResult>`.
- **Algorithm:** one pass groups pixels by (render ID, index); distance transform within each group to pick the most interior pixel.
- **Errors:** `NotVisible` per query; `SizeMismatch`.
- **Determinism:** D1.
- **Performance budget:** 10^5 queries on a 1920 × 1080 image under 1 s.
- **Crate:** `fid-render-oracle` (module `probe`).

## Test fixtures

`fixtures/viz/points-bins` (shared with LOD.ATTR).

## Build prompt

> In crate `fid-render-oracle`, add module `probe`: given headless colour and ID images and queries (render ID, element index), return the colour at the most interior matching pixel (ties lowest row then column) or `NotVisible`; support batches up to 10^5. Tests AC-1..AC-3 with `fixtures/viz/points-bins`. Rules: CONSTITUTION §8.

## Open questions

- None.

## Children

_Leaf: none._
