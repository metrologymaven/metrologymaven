---
id: GEO.FRAME.APPLY
title: Transforming elements and the frame-mismatch guard
level: 3
parent: GEO.FRAME
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.FRAME-006, REQ-GEO.FRAME-008]
covers: []
depends_on: [GEO.FRAME.XFORM, GEO.TYPES.ENV, GEO.TYPES.POINTS, GEO.TYPES.MESH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.FRAME.APPLY: Transforming elements and the frame-mismatch guard

## Purpose

Moves a point set, cloud or mesh into another frame by producing a new element with transformed coordinates and normals, and provides the one guard every binary geometric operation calls so that elements in different frames are never combined silently.

## Scope

**In:**
- `transform_element(e, t, target_frame) -> GeometryElement` for PointSet, PointCloud and Mesh
- Normals rotated (and not scaled) under rigid and similarity transforms; scan origins transformed as points
- `require_same_frame(a, b)` and the typed `FrameMismatch` error
- A refusal for `CadBodyRef`, whose placement belongs to CAD instances

**Out:**
- Registering the new element and journaling (owned by `GEO.FRAME.CMD`)
- Placing CAD bodies (owned by `GEO.CAD.ASSY`)
- Chunked clouds (owned by `GEO.OOC`, which streams through this function per chunk)

## Requirements

- **REQ-GEO.FRAME.APPLY-001** When an element is transformed, the system shall return a new element whose frame is the target frame, whose topology and attribute channels other than normals and scan origins are copied unchanged, and shall leave the input untouched.
- **REQ-GEO.FRAME.APPLY-002** Normals shall be transformed by the rotation part only and shall remain unit length within 1e-12.
- **REQ-GEO.FRAME.APPLY-003** If two elements passed to `require_same_frame` have different frame references, then it shall return `FrameMismatch { left, right }` and never convert.
- **REQ-GEO.FRAME.APPLY-004** If `transform_element` is called on a `CadBodyRef`, then it shall fail with `Unsupported { kind }`.
- **REQ-GEO.FRAME.APPLY-005** Transforming an element and then transforming the result by the inverse shall reproduce every coordinate within 1e-12 m for coordinates up to 100 m.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/types/cloud_grid_2scans` and transform 17 of `fixtures/geo/frame/xform_cases.json`, when transformed, then coordinates match `fixtures/geo/frame/apply_expected.bin` bit-for-bit, normals have |n| within 1e-12 of 1, and grid shape and scanner IDs are unchanged.
- **AC-2** Given the AC-1 output transformed by the inverse transform, when compared with the input, then the maximum coordinate error is ≤ 1e-12 m.
- **AC-3** (error path) Given `pointset_small` in frame A and `mesh_cube` in frame B, when `require_same_frame` runs, then it returns `FrameMismatch { left: A, right: B }`.
- **AC-4** (error path) Given a `CadBodyRef`, when transformed, then `Unsupported { kind: CadBodyRef }` is returned.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `FrameMismatch { left: FrameRef, right: FrameRef }` as a variant of the shared GEO error; no new element types.
- **Algorithm:** column-wise point application via `XFORM::apply_point`; normals via `apply_dir`; no renormalisation (a valid rotation preserves length within rounding).
- **Errors:** `FrameMismatch`, `Unsupported`.
- **Determinism:** D0.
- **Performance budget:** 10⁸ points with normals in under 2 s single-threaded on reference hardware [unverified].
- **Crate:** `fid-geo-frame` (kernel graph, KNP-1 applies).

## Test fixtures

- Reuses `cloud_grid_2scans` and `xform_cases.json`; `apply_expected.bin` produced by the independent Python script. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-frame` (pure Rust, no I/O, KNP-1), add module `apply` with `transform_element(e: &GeometryElement, t: &Transform, target: FrameRef) -> Result<GeometryElement, GeoError>` and `require_same_frame(a: &GeometryElement, b: &GeometryElement) -> Result<FrameRef, GeoError>`. Points and scan origins use `apply_point`; normals use `apply_dir` (rotation only); every other channel, the triangles and the grid shape copy unchanged; the input is not modified. `CadBodyRef` returns `Unsupported{kind}`. Different frames return `FrameMismatch{left, right}`. Acceptance: AC-1 bit-exact against `apply_expected.bin`, unit normals within 1e-12, grid and scanner IDs kept; AC-2 inverse round trip ≤ 1e-12 m; AC-3 mismatch error; AC-4 CAD refusal. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-frame/src/apply.rs`, `tests/apply.rs`, `fixtures/geo/frame/apply_expected.bin`.

## Open questions

- None.

## Children

_Leaf: none._
