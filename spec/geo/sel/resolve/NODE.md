---
id: GEO.SEL.RESOLVE
title: CPU resolution of picks and lassos
level: 3
parent: GEO.SEL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.SEL-002, REQ-GEO.SEL-003]
covers: [GOM.CAD.01]
depends_on: [GEO.SEL.MODEL, GEO.INDEX.BVH, GEO.FRAME.XFORM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.SEL.RESOLVE: CPU resolution of picks and lassos

## Purpose

Turns what a user did in a view (a click or a lasso drawn with a given camera) into an index set on the CPU, so the journal records geometry, not screen state (C2). VIZ passes explicit camera parameters and pixel coordinates; this leaf owns the arithmetic and its brute-force-checked correctness.

## Scope

**In:**
- `Camera`: view transform (element frame to camera), perspective or orthographic projection, viewport size in pixels
- Pick: the pixel's ray; nearest triangle hit via BVH, or nearest point within a pixel tolerance
- Lasso: polygon in pixel coordinates (up to 4 096 vertices, even-odd fill rule); selects points, or triangles by a rule (all vertices, any vertex, centroid)
- Modes: `through` (everything inside the lasso) and `visible` (only elements not occluded, using a CPU depth buffer at viewport resolution)
- Face picks and face lassos on CAD tessellations, mapping triangles to their face IDs (GOM.CAD.01 selection part), once tessellations exist

**Out:**
- Drawing the lasso, highlighting and GPU picking buffers (owned by `VIZ`)
- Storing and journaling the result (owned by `GEO.SEL.CMD`)
- Tessellation (owned by `GEO.CAD.TESS`)

## Requirements

- **REQ-GEO.SEL.RESOLVE-001** When given a camera and a lasso polygon, the resolver shall return the index set of points or triangles inside the polygon under the chosen rule and mode, computed on the CPU only.
- **REQ-GEO.SEL.RESOLVE-002** The resolver shall agree with a brute-force reference implementation on the selection fixtures with zero differing indexes.
- **REQ-GEO.SEL.RESOLVE-003** Projection to pixel coordinates shall use a fixed operation order with no fused multiply-add, and point-in-polygon tests shall use the even-odd rule with a documented half-open edge convention, so results are bit-identical across targets.
- **REQ-GEO.SEL.RESOLVE-004** In `visible` mode, an element shall be selected only if its depth is within a stated depth tolerance (default 1e-6 of the near-far range [unverified]) of the CPU depth buffer at its pixel.
- **REQ-GEO.SEL.RESOLVE-005** If the camera is degenerate (non-finite, zero viewport, near ≥ far) or the polygon has fewer than 3 or more than 4 096 vertices, then the resolver shall fail with a typed error.
- **REQ-GEO.SEL.RESOLVE-006** When the target mesh carries per-triangle face IDs, the resolver shall offer a face domain result containing every face with at least one selected triangle.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/sel/lasso_cases` (the `mesh_sphere_1e5` mesh, `pointset_small`, 200 seeded cameras and lassos, convex and concave), when resolved in both modes and all triangle rules, then results equal the brute-force reference with zero differing indexes.
- **AC-2** Given 100 seeded pick pixels, when picked, then the nearest-hit triangle equals the brute-force ray test over all triangles.
- **AC-3** Given points lying exactly on lasso edges (in the fixture), when resolved, then the half-open convention decides consistently and matches the reference.
- **AC-4** Given AC-1 inputs on x86-64, ARM64 and wasm32, then the result digest is identical.
- **AC-5** (error path) Given near = far or a 2-vertex polygon, then a typed error is returned.
- **AC-6** Given a mesh whose triangles carry face IDs (synthetic, two faces), when lassoed in the face domain, then the faces touched are returned.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Camera { view: Rigid, proj: Perspective { fov_y, aspect, near, far } | Ortho { height, aspect, near, far }, viewport: (u32, u32) }`; `Lasso { pixels: Vec<(f64, f64)> }`; `ResolveMode { Through, Visible { depth_tol } }`; `TriangleRule`.
- **Algorithm:** transform to camera space via GEO.FRAME, project, convert to pixels; point-in-polygon by crossing number with half-open edges; the depth buffer by scanline rasterisation of triangles on the CPU at viewport resolution with a fixed triangle order and a total-order depth comparison; picks via `PrimitiveBvh::ray` and exact nearest-hit selection (ties by triangle index).
- **Errors:** `BadCamera`, `BadPolygon`, `EmptyTarget`.
- **Determinism:** D0 (the parent allows D1; fixed-order arithmetic gives D0).
- **Performance budget:** lasso over 10⁷ points in `through` mode under 500 ms single-threaded [unverified].
- **Crate:** `fid-geo-sel` (kernel graph, KNP-1 applies).

## Test fixtures

- `fixtures/geo/sel/lasso_cases`: seeded cameras and polygons (seed 72) with brute-force results; the brute-force resolver is a test-only module in the same crate. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-sel` (pure Rust, no I/O, KNP-1), add module `resolve`: `Camera` (view as `Rigid`, perspective or orthographic projection, viewport pixels), `resolve_lasso(element, camera, lasso, domain, rule, mode) -> Result<IndexSet, SelError>` and `resolve_pick(element, bvh, camera, pixel, tol_px)`. Project with fixed-order arithmetic (no `mul_add`), test points with the even-odd crossing rule using half-open edges, implement `Visible` mode with a CPU scanline depth buffer at viewport resolution (fixed triangle order, total-order depth compare, depth tolerance), select triangles by rule (all vertices, any vertex, centroid), map triangles to faces when per-triangle face IDs exist, and pick via `PrimitiveBvh::ray` with ties by triangle index. Write a brute-force reference resolver for tests. Errors: `BadCamera`, `BadPolygon` (< 3 or > 4 096 vertices), `EmptyTarget`. Acceptance: AC-1..AC-6 (zero differing indexes vs brute force; picks; edge convention; identical digests on three targets; typed errors; face domain). Constraints: CONSTITUTION C2 (CPU only), §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-sel/src/resolve.rs`, `src/raster.rs`, `tests/resolve.rs`.

## Open questions

- CAD face picks need tessellations (GEO.CAD.TESS, blocked on D-024); REQ-GEO.SEL.RESOLVE-006 works on any mesh with per-triangle face IDs, so this leaf is not blocked.

## Children

_Leaf: none._
