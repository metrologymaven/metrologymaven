---
id: VIZ.RENDER.ORACLE.CPURAST
title: CPU reference rasterizer for every render path
level: 4
parent: VIZ.RENDER.ORACLE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.ORACLE-001]
covers: []
depends_on: [VIZ.RENDER.SCENE.PROJ, VIZ.VIEW.STATE]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.ORACLE.CPURAST: CPU reference rasterizer for every render path

## Purpose

A deliberately simple, wgpu-independent CPU rasterizer that renders the same scene, view and size as the GPU paths: points (nearest-point per pixel), triangles with flat or Lambert shading, lines, sorted alpha blending as the transparency reference, clip planes with caps, and LUT colouring by kernel bin. Its colour, depth and ID images are the references the golden-image gate compares against.

## Scope

**In:** a scanline/edge-function rasterizer in f64 producing RGBA8, depth and ID images; per-path reference modes (`points`, `mesh`, `edges`, `transp`, `clipcap`, `glyph`, `lut`); deterministic tie rules.
**Out:** tolerance masks and CI gate (`VIZ.RENDER.ORACLE.GATE`); pixel probe (`VIZ.RENDER.ORACLE.PROBE`); GPU paths themselves.

## Requirements

- **REQ-VIZ.RENDER.ORACLE.CPURAST-001** For each registered render path, the rasterizer shall produce colour, depth and ID images from the same scene graph, ViewState and size as the GPU path.
- **REQ-VIZ.RENDER.ORACLE.CPURAST-002** The rasterizer shall not link wgpu or any GPU API, enforced by a dependency check.
- **REQ-VIZ.RENDER.ORACLE.CPURAST-003** Depth ties shall resolve to the lower render ID, then the lower primitive or point index.
- **REQ-VIZ.RENDER.ORACLE.CPURAST-004** The transparency reference shall composite fragments sorted back to front per pixel with exact alpha blending.
- **REQ-VIZ.RENDER.ORACLE.CPURAST-005** Rendering the same inputs twice shall produce byte-identical images.

## Acceptance criteria

- **AC-1** Given `fixtures/viz/golden-cube`, when rendered at 640 × 480, then the ID image at face centres holds the face render IDs and the colour image hash equals the stored reference hash.
- **AC-2** Given `fixtures/viz/points-grid`, then each sampled pixel's ID equals the analytically expected nearest point.
- **AC-3** Given `fixtures/viz/transp-stack`, then the colour at the triple-overlap pixel equals the analytic back-to-front blend within 1/255.
- **AC-4** Given two runs, then all three images are byte-identical; given `cargo tree`, then no wgpu crate appears.

## Design

- **Registry command(s):** none; test crate `fid-render-oracle`.
- **Algorithm:** project with the same f64 camera math as `VIZ.VIEW.STATE` cameras; edge-function triangle raster with top-left fill rule; points as single-pixel or sized squares with nearest-depth; lines by DDA; clip planes in f64; caps by odd-crossing test per pixel.
- **Errors:** `UnsupportedPath`.
- **Determinism:** D1 (f64 on one platform; images compared within masks across platforms).
- **Performance budget:** 640 × 480 golden scene under 2 s [unverified].
- **Crate:** `fid-render-oracle`.

## Test fixtures

`fixtures/viz/golden-cube`, `points-grid`, `transp-stack` (in-repo, CC0).

## Build prompt

> Create test crate `fid-render-oracle` with a CPU rasterizer (no wgpu dependency) that takes the scene graph, a ViewState and a size and returns colour (RGBA8), depth and ID images for path modes points, mesh, edges, transp (per-pixel back-to-front exact blending), clipcap, glyph and lut. Use f64 camera math, top-left fill rule, ties to lower render ID then lower index; deterministic byte-identical output. Tests AC-1..AC-4 with `fixtures/viz/golden-cube`, `points-grid`, `transp-stack`. Rules: CONSTITUTION §8.

## Open questions

- Settles the parent's question: the oracle is fully independent of wgpu (no software-adapter reuse), so a wgpu bug cannot mask itself.

## Children

_Leaf: none._
