---
id: VIZ.RENDER.SHADE
title: Mesh and CAD shading, transparency and clip raster
level: 3
parent: VIZ.RENDER
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-001]
covers: [SA.VIZ.01]
depends_on: [VIZ.RENDER.SCENE, VIZ.RENDER.FRAME, GEO.CAD]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.RENDER.SHADE: Mesh and CAD shading, transparency and clip raster

## Purpose

Draw surfaces: meshes and CAD tessellations with face and edge shading, silhouettes, large-assembly culling and instancing, order-independent transparency, and clip-plane and section-plane rasterization from the clip state that VIZ.VIEW owns. Display clips are visual only and never presented as metrology sections.

## Scope

**In:**
- Mesh and CAD tessellation shading, feature edges, silhouettes
- Large assemblies: per-part culling, instancing, bounding-box first pixels
- Weighted blended OIT interactively and depth peeling in report-quality mode
- Clip planes and boxes with stencil caps on closed solids
- Wireframe, hidden-line and shaded-with-edges modes

**Out** (and which node owns it instead):
- Clip and transparency state and commands (`VIZ.VIEW`)
- Kernel-computed metrology sections (`SCAN.SECT`)
- Tessellation of CAD B-reps (`GEO.CAD`)
- Points (`VIZ.RENDER.LOD`)

## Requirements

- **REQ-VIZ.RENDER.SHADE-001** While displaying a mesh of 10⁷ triangles on the reference mid-range profile, the renderer shall sustain at least 30 fps [unverified].
- **REQ-VIZ.RENDER.SHADE-002** When a CAD assembly is opened, the renderer shall draw part bounding boxes within 1 s and refine to shaded tessellations progressively [unverified].
- **REQ-VIZ.RENDER.SHADE-003** The renderer shall render transparency order-independently in interactive mode and by depth peeling in report-quality mode.
- **REQ-VIZ.RENDER.SHADE-004** The renderer shall rasterize at least 6 simultaneous clip planes per viewport, with stencil caps on closed solids.
- **REQ-VIZ.RENDER.SHADE-005** Display clip caps shall be styled distinctly from kernel-computed sections and shall carry no metrology label.
- **REQ-VIZ.RENDER.SHADE-006** Every triangle drawn shall write its render ID and primitive index to the ID buffer.

## Interfaces

- **Offers:** surface passes for the frame graph; displayed and total triangle counts.
- **Consumes:** `GEO.CAD` tessellations; `VIZ.RENDER.SCENE` nodes; clip and transparency state (from `VIZ.VIEW`, as data).

## Decomposition hints

MESH, CADFACE (faces, edges, silhouettes), ASSY (culling, instancing), OIT, CLIPR (clip raster and caps), MODES (display modes).

## Open questions

- GEO.CAD is tagged P1b; P1a shades meshes only, and CAD faces, edges and assemblies land with GEO.CAD.
- Report-quality depth peeling layer count and its budget are to be set with RPT.

## Children

_None yet: status `open`._
