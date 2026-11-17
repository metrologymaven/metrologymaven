---
id: VIZ.PICK.LASSO
title: Lasso selection as semantic queries
level: 3
parent: VIZ.PICK
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK-001, REQ-VIZ.PICK-002, REQ-VIZ.PICK-004, REQ-VIZ.PICK-005, REQ-VIZ.PICK-009]
covers: []
depends_on: [VIZ.PICK.RESOLVE, VIZ.VIEW.STATE, GEO.SEL.RESOLVE, GEO.SEL.CMD]
uses_artifacts: []
side_effect: R1
determinism: D0
cli: [fid view resolve-lasso]
mcp: [view_resolve_lasso]
journal: [view.resolve_lasso@1]
updated: 2026-11-17
---

# VIZ.PICK.LASSO: Lasso selection as semantic queries

## Purpose

Turns rectangle, polygon and brush gestures over clouds, meshes and CAD tessellations into a `LassoQuery`: the f64 camera of the viewport (from its ViewState), the lasso polygon or polygons in the viewport's pixel frame, the mode (`through` or `visible`) and the target entity IDs. `view.resolve_lasso@1` (R1) resolves it on the CPU through GEO's resolver for preview counts and highlighting. Committing the selection calls `geo.resolve_selection@1` (W2) with the same camera and polygon, so the journal holds the query, never pixels from the GPU, and re-targeted replay re-resolves it on new data.

## Scope

**In:**
- Gesture capture: rectangle (4-vertex polygon), polygon (3 to 4,096 vertices), brush (stroke stamped into at most 64 convex 24-gons of the brush radius)
- `LassoQuery` construction from the ViewState f64 camera and viewport size
- `view.resolve_lasso@1` (R1): per-target sorted index sets, counts and digests, plus the query
- Commit: one `geo.resolve_selection@1` per target and polygon, and `geo.combine_selection@1` unions for brush stamps
- Parallel resolution over targets with deterministic merge order (target ID order)

**Out** (and which node owns it instead):
- Resolution algorithm and the visible-mode CPU depth test (`GEO.SEL.RESOLVE`); selection commands and storage (`GEO.SEL.CMD`)
- Current selection and set algebra (`VIZ.PICK.SELSET.STATE`); single picks (`VIZ.PICK.RESOLVE`)

## Requirements

- **REQ-VIZ.PICK.LASSO-001** When a lasso gesture completes, the module shall build a `LassoQuery` from the viewport's f64 camera, viewport size, polygon vertices in pixel coordinates as f64, mode and target entity IDs before any resolution.
- **REQ-VIZ.PICK.LASSO-002** Membership shall be decided only by `GEO.SEL.RESOLVE` on the CPU, and no GPU buffer shall be read to decide it.
- **REQ-VIZ.PICK.LASSO-003** Resolving the same query twice on the same document state shall return identical index sets and digests.
- **REQ-VIZ.PICK.LASSO-004** When a lasso over 10⁸ points in `through` mode is resolved, the result shall be returned within 2 s on the reference profile [unverified].
- **REQ-VIZ.PICK.LASSO-005** In `visible` mode, occlusion shall be decided by GEO's CPU depth test, never by the GPU depth buffer.
- **REQ-VIZ.PICK.LASSO-006** Committing a lasso shall record it only through `geo.resolve_selection@1` (and `geo.combine_selection@1` for brush stamps) with the query's camera and polygons as arguments, so that re-targeted replay re-resolves it against new data.
- **REQ-VIZ.PICK.LASSO-007** If a brush stroke would need more than 64 stamps, then the module shall stop adding stamps and tell the user the stroke was truncated.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/pick/lasso-cloud` (1 M seeded points, a saved camera and a 6-vertex polygon), when `fid view resolve-lasso --view main --polygon <file> --mode through --json` runs, then the index set equals the GEO brute-force reference with zero differing indices and its digest matches the fixture.
- **AC-2** Given the same query twice, then outputs are byte-identical.
- **AC-3** Given `visible` mode on a mesh behind another mesh, then occluded triangles are excluded, and an instrumented GPU device shows no depth-buffer read during resolution.
- **AC-4** Given a commit in the GUI, then the journal holds `geo.resolve_selection@1` entries whose arguments are the query's camera and polygon, and no entry holds render IDs or pixel buffers.
- **AC-5** (replay) Given the journal of AC-4 replayed in re-targeted mode on `lasso-cloud-v2` (the cloud perturbed by seed 9), then the selection re-resolves to the brute-force reference set on the new cloud.
- **AC-6** Given the nightly benchmark `lasso_1e8_through` on the reference profile, then the time is reported against REQ-004 (informative until measured).

## Design

- **Registry command(s):** `view.resolve_lasso@1`, R1, D0; params `{ view: ViewportId | NamedViewId, shape: Rect { x0, y0, x1, y1 } | Polygon { pts: [(f64, f64)] } | Brush { path: [(f64, f64)], radius_px: f64 }, mode: Through | Visible { depth_tol? }, targets?: [EntityId] }` (default targets: visible entities in the view); result `{ query: LassoQuery, results: [{ entity, domain, count, digest, indices? }] }` (indices omitted above 10⁶ entries; use the digest and commit).
- **Data structures:** `LassoQuery { camera: geo::Camera, polygons: Vec<geo::Lasso>, mode, targets }` with canonical JSON digest.
- **Algorithm:** camera from ViewState; shape to polygons; for each target in ID order (parallel across targets), call `geo_sel::resolve`; union stamp results with GEO algebra; collect.
- **Errors:** `BadPolygon`, `UnknownView`, `EmptyTarget`, `BrushTruncated` (warning).
- **Determinism:** D0 (GEO resolver is D0; merge order fixed).
- **Performance budget:** REQ-004 [unverified]; GEO states 10⁷ points in 500 ms single-threaded [unverified], so 10⁸ needs parallel resolution.
- **Crate:** `fid-pick` (module `lasso`).

## Test fixtures

`fixtures/viz/pick/lasso-cloud` and `lasso-cloud-v2`: seeded synthetic clouds generated by a committed script; CC-BY-4.0.

## Build prompt

> In crate `fid-pick`, add module `lasso` and registry command `view.resolve_lasso@1` (R1; CLI `fid view resolve-lasso`, MCP `view_resolve_lasso`). Build a `LassoQuery` from the viewport's f64 camera (ViewState), viewport size, rectangle/polygon (3–4,096 vertices)/brush (≤ 64 convex 24-gon stamps) in pixel coordinates, mode `through|visible` and targets; resolve membership only with `fid-geo-sel`'s CPU resolver (visible mode uses its CPU depth test, never the GPU depth buffer), in parallel across targets with deterministic target-ID order; return per-target counts, digests and (≤ 10⁶) indices. Commit from the GUI by calling `geo.resolve_selection@1` per target and polygon (and `geo.combine_selection@1` to union brush stamps) with the query's camera and polygon, so replay re-resolves the query. Tests AC-1..AC-6. Rules: picks and lassos resolved on the CPU before journaling (C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- A brush commit costs one `geo.resolve_selection@1` per stamp plus unions; GEO accepting several polygons in one call would make it one entry. Requested from `GEO.SEL.CMD`.
- Whether 10⁸ points within 2 s is reachable depends on GEO's resolver; parallelism within one large target needs an index-range restriction in the GEO API [unverified].

## Children

_Leaf: none._
