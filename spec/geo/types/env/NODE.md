---
id: GEO.TYPES.ENV
title: Element envelope and kind tags
level: 3
parent: GEO.TYPES
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-geo-types]
satisfies: [REQ-GEO.TYPES-001, REQ-GEO.TYPES-002]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.TYPES.ENV: Element envelope and kind tags

## Purpose

The common `GeometryElement` envelope that every geometric element sits in: the closed set of element kinds, the mandatory frame reference slot, the axis-aligned bounding box type, the typed error enum shared by all element constructors, and the only sanctioned conversions between kinds. POINTS and MESH fill in the concrete kinds; this leaf fixes the shape they plug into.

## Scope

**In:**
- `ElementKind` tag: `PointSet`, `PointCloud`, `Mesh`, `CadBodyRef`
- `GeometryElement` enum wrapping the four concrete types, with `kind()`, `frame()`, `aabb()`
- `FrameRef` (an opaque 128-bit frame ID; the frame model is GEO.FRAME)
- `CadBodyRef`: an opaque handle (body ID plus the source blob digest), with no topology
- `Aabb` (min and max corners, f64 metres) and its union and containment helpers
- `GeoTypeError`, the typed error enum for every constructor in GEO.TYPES
- Explicit kind conversions: `PointCloud -> PointSet`, `Mesh -> PointSet` (vertices)
- The `PointSource` trait (blocks of full-resolution points with original indexes), implemented here for in-memory clouds and by GEO.OOC.STREAM for chunked clouds, so kernel code can stream points without I/O

**Out:**
- Point and attribute storage (owned by `GEO.TYPES.POINTS`)
- Triangle storage and topology facts (owned by `GEO.TYPES.MESH`)
- Digests (owned by `GEO.TYPES.DIGEST`)
- Frame tree and transforms (owned by `GEO.FRAME`)
- B-rep content behind `CadBodyRef` (owned by `GEO.CAD`)

## Requirements

- **REQ-GEO.TYPES.ENV-001** The crate shall define `PointSet`, `PointCloud`, `Mesh` and `CadBodyRef` as distinct types and shall implement no `From`, `Into` or `Deref` conversion between any two of them.
- **REQ-GEO.TYPES.ENV-002** Every element constructor shall take a `FrameRef` argument, and no constructor, `Default` implementation or builder shall produce an element without one.
- **REQ-GEO.TYPES.ENV-003** When `GeometryElement::kind()` is called, the envelope shall return the `ElementKind` of the wrapped element, and an operation that requires a specific kind shall return `GeoTypeError::WrongKind { expected, found }` for any other kind.
- **REQ-GEO.TYPES.ENV-004** The only conversions between kinds shall be the named functions `PointCloud::to_point_set` and `Mesh::vertices_to_point_set`, each returning a new element in the same frame.
- **REQ-GEO.TYPES.ENV-005** When `aabb()` is called on an element with zero points, the envelope shall return `GeoTypeError::Empty` rather than an infinite or NaN box.
- **REQ-GEO.TYPES.ENV-006** The crate shall define the `PointSource` trait (`next_block`, `len`, `frame`) and implement it for `PointSet` and `PointCloud`, emitting points in index order with their original indexes.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/types/pointset_small` (1 000 points), when it is wrapped in `GeometryElement`, then `kind()` is `PointSet`, `frame()` equals the fixture's frame ID and `aabb()` equals the fixture's recorded min and max corners exactly (bitwise).
- **AC-2** (error path) Given a `GeometryElement::Mesh` passed to a function requiring `PointSet`, when the kind guard runs, then it returns `WrongKind { expected: PointSet, found: Mesh }`.
- **AC-3** (error path) Given an empty `PointSet`, when `aabb()` runs, then it returns `GeoTypeError::Empty`.
- **AC-4** Given fixture `fixtures/geo/types/mesh_cube` (8 vertices), when `vertices_to_point_set` runs, then the result has 8 points, the same frame and the same coordinates in vertex order.
- **AC-5** A compile-fail test (trybuild) shows that `let p: PointSet = mesh.into();` and `PointSet::default()` do not compile.
- **AC-6** Given `pointset_small` read through `PointSource` with block size 128, when the blocks are concatenated, then they equal the point buffer in index order with original indexes 0..999.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `pub struct FrameRef([u8; 16])`; `pub struct CadBodyRef { body: [u8; 16], source_digest: [u8; 32], aabb: Aabb }`; `pub struct Aabb { min: [f64; 3], max: [f64; 3] }`; `pub enum GeometryElement { PointSet(PointSet), PointCloud(PointCloud), Mesh(Mesh), CadBody(CadBodyRef) }`; `pub enum GeoTypeError { WrongKind{..}, Empty, NonFinite{index}, IndexOutOfRange{triangle, index}, AttributeLength{name, expected, found}, BadNormal{index} }`.
- **Algorithm:** `aabb` is a fixed-order fold using the `fid-num` total-order min and max (KNP-1); `CadBodyRef` has no geometry, so its `aabb()` returns the box stored at import time.
- **Errors:** as listed; every error is a value, never a panic.
- **Determinism:** D0. Only comparisons and copies.
- **Performance budget:** `aabb` over 10⁸ points in under 300 ms single-threaded on reference hardware [unverified].
- **Crate:** `fid-geo-types` (kernel graph, KNP-1 applies).

## Test fixtures

- `fixtures/geo/types/pointset_small`: 1 000 points from the seeded synthetic generator (seed 1, uniform in a 0.1 m cube), with its frame ID and expected AABB. Generated, CC-BY-4.0.
- `fixtures/geo/types/mesh_cube`: a closed unit-cube mesh, 8 vertices and 12 triangles, hand-written. CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-geo-types` (pure Rust, f64, no I/O, part of the kernel dependency graph), create the module `envelope` with: `FrameRef` (16-byte opaque ID), `CadBodyRef { body: [u8;16], source_digest: [u8;32], aabb: Aabb }`, `Aabb { min: [f64;3], max: [f64;3] }` with `union` and `contains`, `ElementKind` (`PointSet`, `PointCloud`, `Mesh`, `CadBodyRef`), and `GeometryElement` wrapping the concrete types with `kind()`, `frame()` and `aabb() -> Result<Aabb, GeoTypeError>`. Define `GeoTypeError` with variants `WrongKind { expected, found }`, `Empty`, `NonFinite { index: u64 }`, `IndexOutOfRange { triangle: u64, index: u64 }`, `AttributeLength { name, expected: u64, found: u64 }`, `BadNormal { index: u64 }`. Requirements: the four kinds are distinct types with no `From`/`Into`/`Deref` between them; every constructor takes a `FrameRef` and there is no `Default`; the only kind conversions are `PointCloud::to_point_set` and `Mesh::vertices_to_point_set`, keeping the frame; `aabb()` on zero points returns `Empty`. Acceptance: (1) fixture `pointset_small` gives the recorded AABB bit-exactly; (2) the kind guard returns `WrongKind`; (3) empty set gives `Empty`; (4) `mesh_cube` vertices convert to 8 points in order; (5) trybuild compile-fail tests for implicit conversion and `Default`. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`), KNP-1 (min/max through `fid-num` total order, no std HashMap/HashSet, no FMA). Also define `trait PointSource { fn next_block(&mut self) -> Option<Result<PointBlock, GeoTypeError>>; fn len(&self) -> u64; fn frame(&self) -> FrameRef; }` with `PointBlock { original_index: Vec<u64>, x, y, z, channels }`, implemented for `PointSet` and `PointCloud` in index order (AC-6: blocks of 128 concatenate to the buffer). Files: `crates/fid-geo-types/src/envelope.rs`, `src/source.rs`, `src/error.rs`, `src/lib.rs`, `tests/envelope.rs`, `tests/ui/*.rs`.

## Open questions

- Whether `CadBodyRef` should carry its AABB (convenient for indexing) or fetch it from GEO.CAD on demand; this leaf stores it at import time.

## Children

_Leaf: none._
