---
id: VIZ.RENDER.ORACLE
title: CPU reference rasterizer and golden-image CI
level: 3
parent: VIZ.RENDER
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-011]
covers: []
depends_on: [VIZ.RENDER.FRAME, VIZ.RENDER.HEADLESS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.ORACLE: CPU reference rasterizer and golden-image CI

## Purpose

The test oracle for every GPU render path: a CPU rasterizer that renders the same scene to the same image specification, per-path per-pixel tolerance masks, golden-image management, and the CI harness that compares GPU output against the oracle. It also provides the pixel-probe harness used by the legend-conformance test (pixels at known point indices equal the LUT entry for the kernel's bin).

## Scope

**In:**
- CPU reference rasterizer for points, meshes, CAD edges, transparency, clip caps and glyphs
- Declared per-path tolerance masks
- Golden-image storage, regeneration by explicit maintainer command only
- CI comparison harness on the software adapter
- Pixel-probe harness (sample pixels at known render and point IDs)

**Out** (and which node owns it instead):
- The GPU paths themselves (`VIZ.RENDER.LOD`, `VIZ.RENDER.SHADE`, `VIZ.RENDER.GLYPH`)
- The legend-conformance assertions (`VIZ.OVERLAY`)
- Benchmark runs on reference hardware (`PLAT`)

## Requirements

- **REQ-VIZ.RENDER.ORACLE-001** For every GPU render path the CPU rasterizer shall produce a reference image from the same scene, view and size.
- **REQ-VIZ.RENDER.ORACLE-002** Each render path shall declare a per-pixel tolerance mask, and CI shall fail when GPU output differs from the reference outside that mask.
- **REQ-VIZ.RENDER.ORACLE-003** CI shall fail if a GPU render path exists without a registered oracle comparison.
- **REQ-VIZ.RENDER.ORACLE-004** Golden images shall change only through an explicit maintainer command that records the reason in the commit.
- **REQ-VIZ.RENDER.ORACLE-005** The pixel-probe harness shall return the colour at the pixel holding a given render ID and point or primitive index, and shall report when that element is not visible.

## Interfaces

- **Offers:** `fid-render-oracle` test crate; pixel-probe API; golden-image tooling.
- **Consumes:** headless images from `VIZ.RENDER.HEADLESS`; shader manifest and path registry from `VIZ.RENDER.FRAME`.

## Decomposition hints

CPURAST (per-path reference), MASK (tolerance masks), GOLDEN (storage and regeneration), PROBE (pixel probe), CIGATE (path registry check).

## Open questions

- Whether the oracle reuses the software adapter for some paths or must be fully independent of wgpu is to be fixed at leaf level.
- Resolved in CPURAST: the oracle is fully independent of wgpu (no software-adapter reuse).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.RENDER.ORACLE.CPURAST | CPU reference rasterizer for every render path (leaf) | 001 | none |
| VIZ.RENDER.ORACLE.GATE | Tolerance masks, golden images and CI comparison gate (leaf) | 002 003 004 | none |
| VIZ.RENDER.ORACLE.PROBE | Pixel-probe harness (leaf) | 005 | none |
