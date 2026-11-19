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
built_in: [fid-geo-frame]
built_pr: 263
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
- Normals rotated (and not scaled) under rigid and similarity transforms, then renormalised; scan origins transformed as points
- `require_same_frame(a, b)` and the typed `FrameMismatch` error
- A refusal for `CadBodyRef`, whose placement belongs to CAD instances

**Out:**
- Registering the new element and journaling (owned by `GEO.FRAME.CMD`)
- Placing CAD bodies (owned by `GEO.CAD.ASSY`)
- Chunked clouds (owned by `GEO.OOC`, which streams through this function per chunk)

## Requirements

- **REQ-GEO.FRAME.APPLY-001** When an element is transformed, the system shall return a new element whose frame is the target frame, whose topology and attribute channels other than normals and scan origins are copied unchanged, and shall leave the input untouched.
- **REQ-GEO.FRAME.APPLY-002** Normals shall be transformed by the rotation part only and then renormalised, so that every output normal is unit length within 1e-12.
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
- **Algorithm:** column-wise point application via `XFORM::apply_point`; normals via `apply_dir`, then renormalised (pinned below). (Build note: the original "no renormalisation" was changed in review of #263: an input normal accepted at GEO.TYPES' ±1e-9 edge could be pushed past it by an accepted rotation residual and then fail validation, so a valid element would fail to transform.)
- **Errors:** `FrameMismatch`, `Unsupported`.
- **Determinism:** D0.
- **Performance budget:** 10⁸ points with normals in under 2 s single-threaded on reference hardware [unverified].
- **Crate:** `fid-geo-frame` (kernel graph, KNP-1 applies).
- **Pinned choices (build, value-affecting).** The module docs of `crates/fid-geo-frame/src/apply.rs` repeat these and are part of this design:
  - Points and vertices are mapped one at a time in index order by `apply_point`, `y[i] = s·dot3(R[i], x) + t[i]` with `dot3(a, b) = (a0·b0 + a1·b1) + a2·b2` (GEO.FRAME.XFORM). Scan origins are points and are mapped the same way, in stored (ascending scanner-ID) order.
  - Normals are mapped by `apply_dir`, `m[i] = dot3(R[i], n)`, the rotation only, for rigid and similarity transforms alike. The normal transform of `x ↦ sRx + t` is `(sR)⁻ᵀ = R/s`, which has the direction of `R`; `Transform` has no non-uniform scale or shear, so no other inverse-transpose case exists. Each rotated normal is then renormalised: `len = fid_num::f::sqrt(dot3(m, m))` (correctly rounded), `n'[i] = m[i] / len`, a division per component (not a multiplication by `1/len`). The output is unit within 1e-12 for every accepted input, including normals at GEO.TYPES' ±1e-9 edge. A zero or non-finite length is `BadNormal { index }` (unreachable for validated input).
  - Identity: under `Transform::IDENTITY` (translation +0.0) points and scan origins are bit-exact except that −0.0 becomes +0.0 (the final `+ t[i]` with `t[i] = +0.0`); normals become `n/‖n‖`, so they are bit-exact only when the computed `‖n‖` is exactly 1.
  - Every other channel, the grid shape, the triangles, the per-triangle attributes and the mesh index width are copied unchanged. The output is rebuilt through the validating constructor of its kind (`PointSet::new`, `PointCloud::new`, `Mesh::with_options`); mesh topology facts are recomputed and equal the input's.
  - The target frame is not compared with the source frame (a correction transform within one frame is allowed); which transform links two frames is the caller's (TREE resolves, CMD journals). `transform_element_with_provenance` returns `Transformed { element, provenance: TransformProvenance { source, target, transform } }` for that record.
  - Errors, first failure wins: `CadBodyRef` is `Unsupported { kind: CadBodyRef }` before any arithmetic; then points in index order (an overflowing result is `Element(NonFinite { index })`), normals (`Element(BadNormal { index })`), scan origins (`Element(ScanOrigin { scanner_id })`), then the constructor's own checks. Output columns are reserved fallibly (`Element(AllocationFailed)`); copied attribute columns and per-triangle attributes use `try_reserve_exact` then `extend_from_slice`, and the triangle list is reserved the same way before it is filled, so no copy sized by the input allocates infallibly.
  - `GeoError { FrameMismatch { left, right }, Unsupported { kind }, Element(GeoTypeError) }` lives in `fid_geo_frame::apply` (no shared GEO error type existed) with codes `frame_mismatch`, `unsupported`, else the GEO.TYPES code. `require_same_frame(a, b)` returns the shared frame or `FrameMismatch { left: a.frame(), right: b.frame() }` for any kinds, CAD bodies included, and never converts; `require_same_frame_ref(left, right)` is the same guard on bare frame references for operands that are not `GeometryElement`s (for example GEO.DIST.BATCH's `ProjectionTarget`).
  - AC-1 build note: `cloud_grid_2scans` carries no normals, so the normal half of AC-1 runs on `pointset_small` with the normals of `pointset_small_attrs.txt`, through the same transform 17; the cube mesh is checked as well. All three expected sets are in `apply_expected.bin`.

## Test fixtures

- Reuses `cloud_grid_2scans`, `pointset_small` (with its normals), `mesh_cube` and `xform_cases.json`; `apply_expected.bin` is written by the independent Python script `tools/fixtures/geo_apply_cases.py` (stdlib only; bit-exact float model of the pinned expressions including the renormalisation, each value checked against exact `fractions` arithmetic (normal lengths through 60-digit `decimal`), and the AC-2 round trip checked in exact arithmetic; CI runs it with `--check`). The byte layout is documented in the script. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-frame` (pure Rust, no I/O, KNP-1), add module `apply` with `transform_element(e: &GeometryElement, t: &Transform, target: FrameRef) -> Result<GeometryElement, GeoError>` and `require_same_frame(a: &GeometryElement, b: &GeometryElement) -> Result<FrameRef, GeoError>`. Points and scan origins use `apply_point`; normals use `apply_dir` (rotation only); every other channel, the triangles and the grid shape copy unchanged; the input is not modified. `CadBodyRef` returns `Unsupported{kind}`. Different frames return `FrameMismatch{left, right}`. Acceptance: AC-1 bit-exact against `apply_expected.bin`, unit normals within 1e-12, grid and scanner IDs kept; AC-2 inverse round trip ≤ 1e-12 m; AC-3 mismatch error; AC-4 CAD refusal. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-frame/src/apply.rs`, `tests/apply.rs`, `fixtures/geo/frame/apply_expected.bin`.

## Open questions

- None.

## Children

_Leaf: none._
