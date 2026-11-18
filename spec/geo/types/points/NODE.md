---
id: GEO.TYPES.POINTS
title: Point sets, point clouds and per-point attributes
level: 3
parent: GEO.TYPES
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-geo-types]
satisfies: [REQ-GEO.TYPES-001, REQ-GEO.TYPES-003, REQ-GEO.TYPES-004, REQ-GEO.TYPES-006]
covers: []
depends_on: [GEO.TYPES.ENV]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.TYPES.POINTS: Point sets, point clouds and per-point attributes

## Purpose

The in-memory storage for measured and scanned points: a structure-of-arrays coordinate buffer in SI metres, an attribute set with the per-point channels scanners and probes produce, validation at construction, and the two element kinds built on them (`PointSet` for sparse or probed points, `PointCloud` for dense scans, including scanner structure).

## Scope

**In:**
- `PointBuffer`: three `Vec<f64>` columns (x, y, z) in metres, 64-bit counts
- `AttributeSet`: optional channels `normal` (3 × f64), `intensity` (f32), `colour` (3 × u8), `scanner_id` (u32), `timestamp_ns` (i64), `quality` (u16 flags), plus per-scanner `scan_origin` and an optional organised-grid shape (`rows`, `cols`)
- Validation: finite coordinates, attribute length equal to point count, unit normals within 1e-9
- `PointSet` and `PointCloud` constructors on top of `PointBuffer` and `AttributeSet`
- Basic statistics: count, AABB (via GEO.TYPES.ENV), attribute presence

**Out:**
- Envelope, frame slot and error enum (owned by `GEO.TYPES.ENV`)
- Digest (owned by `GEO.TYPES.DIGEST`)
- Chunked clouds that exceed memory (owned by `GEO.OOC`)
- Parsing scanner files (owned by `IO`)

## Requirements

- **REQ-GEO.TYPES.POINTS-001** The point buffer shall store coordinates as f64 metres in three separate columns and shall use 64-bit counts and indexes.
- **REQ-GEO.TYPES.POINTS-002** If any coordinate is NaN or infinite, then construction shall fail with `NonFinite { index }` naming the first offending point.
- **REQ-GEO.TYPES.POINTS-003** If any attribute channel's length differs from the point count, then construction shall fail with `AttributeLength { name, expected, found }`.
- **REQ-GEO.TYPES.POINTS-004** If a normal is non-finite or its length differs from 1 by more than 1e-9, then construction shall fail with `BadNormal { index }`, and the constructor shall not normalise it.
- **REQ-GEO.TYPES.POINTS-005** When a grid shape is given, construction shall fail unless `rows × cols` equals the point count.
- **REQ-GEO.TYPES.POINTS-006** A point cloud with scanner structure shall be represented by the `scanner_id`, `scan_origin` and grid attributes on `PointCloud`, not by a separate element kind.
- **REQ-GEO.TYPES.POINTS-007** The types shall support element sizes above 2³¹ points, limited only by available memory.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/types/pointset_small`, when `PointSet::new(frame, buffer, attrs)` runs, then it succeeds and `len()` is 1 000 and coordinates read back bit-identically.
- **AC-2** (error path) Given `pointset_small` with point 517's y set to NaN, when constructed, then it fails with `NonFinite { index: 517 }` and no element exists.
- **AC-3** (error path) Given an intensity channel of 999 values for 1 000 points, when constructed, then it fails with `AttributeLength { name: "intensity", expected: 1000, found: 999 }`.
- **AC-4** (error path) Given a normal of length 1 + 1e-6, when constructed, then it fails with `BadNormal`; a normal of length 1 + 1e-10 is accepted unchanged.
- **AC-5** Given fixture `fixtures/geo/types/cloud_grid_2scans` (two scans of 64 × 48 points with origins), when `PointCloud::new` runs, then grid shape, scanner IDs and both origins are preserved; a 64 × 47 grid declaration fails.
- **AC-6** A compile-time assertion shows count and index types are `u64`; an `#[ignore]`d test on a runner with ≥ 64 GiB RAM constructs 2³¹ + 1 points [unverified runner availability].

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `PointBuffer { x: Vec<f64>, y: Vec<f64>, z: Vec<f64> }`; `AttributeSet { normal: Option<[Vec<f64>; 3]>, intensity: Option<Vec<f32>>, colour: Option<Vec<[u8; 3]>>, scanner_id: Option<Vec<u32>>, timestamp_ns: Option<Vec<i64>>, quality: Option<Vec<u16>>, scan_origins: Vec<(u32, [f64; 3])>, grid: Option<(u64, u64)> }` with `scan_origins` sorted by scanner ID (no HashMap); `PointSet { frame, buf, attrs }`; `PointCloud { frame, buf, attrs }`.
- **Algorithm:** one linear validation pass per channel, in index order, reporting the first failure.
- **Errors:** `NonFinite`, `AttributeLength`, `BadNormal`, `GridShape { rows, cols, count }`.
- **Determinism:** D0. Validation reads values; normal length uses `fid-num::sqrt`.
- **Performance budget:** validation of 10⁸ points with normals under 1 s single-threaded on reference hardware [unverified].
- **Crate:** `fid-geo-types` (kernel graph, KNP-1 applies).

## Test fixtures

- `fixtures/geo/types/pointset_small` (shared with GEO.TYPES.ENV): seeded generator, seed 1, with normals and intensity. CC-BY-4.0.
- `fixtures/geo/types/cloud_grid_2scans`: synthetic organised grids from two simulated scanner origins, seed 2. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-types` (pure Rust, f64, no I/O, kernel graph), add module `points` with `PointBuffer` (three `Vec<f64>` columns, metres), `AttributeSet` (optional channels: normal as three f64 columns, intensity f32, colour `[u8;3]`, scanner_id u32, timestamp_ns i64, quality u16; plus `scan_origins: Vec<(u32, [f64;3])>` sorted by ID and optional `grid: (rows, cols)`), and the element types `PointSet` and `PointCloud`, each built by `new(frame: FrameRef, buf: PointBuffer, attrs: AttributeSet) -> Result<Self, GeoTypeError>`. Validate in index order: coordinates finite (`NonFinite{index}`), every channel length equals the count (`AttributeLength{name, expected, found}`), normals finite and |n| within 1e-9 of 1 (`BadNormal{index}`; never normalise), grid rows × cols equals count (`GridShape`). Counts and indexes are `u64`. Scanner structure lives in attributes, not a new kind. Acceptance: AC-1..AC-6 as in the spec (bit-identical read-back; NaN at index 517; intensity length 999; normal tolerance 1e-9 boundary; two-scan grid preserved and a wrong grid rejected; `u64` assertion plus an ignored 2³¹+1 test). Constraints: CONSTITUTION §8 (no panic or `unsafe`, deny `unwrap`/`expect`), KNP-1 (sqrt via `fid-num`, no std HashMap/HashSet, no FMA). Files: `crates/fid-geo-types/src/points.rs`, `tests/points.rs`, fixtures under `fixtures/geo/types/`.

## Open questions

- REQ-GEO.TYPES.POINTS-006 follows the parent's lean (scanner structure as attributes); IO's E57 and PTX readers should confirm that nothing more is needed.

## Children

_Leaf: none._
