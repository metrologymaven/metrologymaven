---
id: GEO.TYPES
title: Geometry element types and invariants
level: 2
parent: GEO
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO-001, REQ-GEO-002]
covers: []
depends_on: [CORE]
cli: [fid geo describe-*]
mcp: [geo_describe_*]
journal: []
updated: 2026-11-16
---

# GEO.TYPES: Geometry element types and invariants

## Purpose

The typed, in-memory representation of every geometric element the platform holds: point sets (with optional normals and per-point attributes), triangle meshes, point clouds, and the handle types that refer to exact CAD bodies. It fixes the invariants each type guarantees, how elements are content-digested, and how they register with the CORE element tree. Every other GEO subsystem and every consuming domain builds on these types, so they live in one place.

## Scope

**In:**
- Element kinds: `PointSet`, `Mesh` (indexed triangles), `PointCloud` (in-memory form), `CadBody` handle, and the common `GeometryElement` envelope
- Per-point and per-vertex attributes: normals, intensity, colour, scanner ID, timestamp, quality flags
- Structural invariants (index bounds, finite coordinates, consistent attribute lengths, manifoldness flags as reported facts, not repairs)
- Canonical content digest of each element
- Bounding boxes and basic element statistics (counts, extents)
- The frame reference slot on every element (the frame model itself is GEO.FRAME)
- Registration of elements with the CORE element tree and dependency graph
- Read-only describe commands

**Out** (and which node owns it instead):
- Frames, transforms and unit conversion (owned by `GEO.FRAME`)
- B-rep topology, NURBS data and tessellation (owned by `GEO.CAD` and `GEO.NURBS`)
- Spatial indexes (owned by `GEO.INDEX`)
- Clouds larger than memory and LOD (owned by `GEO.OOC`)
- Mesh repair and processing (owned by `SCAN`)
- File parsing into these types (owned by `IO`)

## Requirements

- **REQ-GEO.TYPES-001** The geometry model shall provide distinct element kinds for point set, triangle mesh, point cloud and CAD body, and no operation shall accept one kind where another is required without an explicit typed conversion.
- **REQ-GEO.TYPES-002** Every geometric element shall carry exactly one frame reference, and construction of an element without a frame reference shall fail.
- **REQ-GEO.TYPES-003** If an element is constructed with a non-finite coordinate, an out-of-range vertex index or an attribute array whose length differs from its point count, then the system shall reject it with a typed error and create no element.
- **REQ-GEO.TYPES-004** Element coordinates shall be stored as f64 in SI metres.
- **REQ-GEO.TYPES-005** Each element shall have a content digest computed over a canonical byte layout, such that two elements with identical coordinates, topology, attributes and frame reference have the same digest on every supported platform.
- **REQ-GEO.TYPES-006** The geometry types shall represent at least 2³¹ points or triangles in a single in-memory element, bounded only by available memory [unverified].
- **REQ-GEO.TYPES-007** When an element is created or replaced, the system shall register it with the CORE element tree under a stable ID per CONSTITUTION §5.
- **REQ-GEO.TYPES-008** The describe commands shall be side-effect class R0 and return element kind, counts, extents, frame and digest as JSON.

## Interfaces

- **Offers:** `GeometryElement`, `PointSet`, `Mesh`, `PointCloud`, `CadBodyRef`, `AttributeSet`, `ElementDigest`, `Aabb`; registry commands `geo.describe_element@1` (R0) and similar describe reads.
- **Consumes:** CORE element tree, stable-ID generation and dependency graph; `Frame` IDs from GEO.FRAME (by reference only).

## Decomposition hints

POINTS (point set and attributes), MESH (indexed triangle mesh and topology facts), DIGEST (canonical layout and digest), REG (CORE registration and describe commands).

## Split rationale

The split follows the **layers of an element's life**: the shared envelope and error model (ENV), the two concrete storage families (POINTS for point sets and clouds, MESH for indexed triangles), the identity of content (DIGEST), and the bridge into the document and the command surface (REG). The first four are pure library code in `fid-geo-types` inside the kernel graph; REG lives in the command adapter crate `fid-geo-cmd`, because the kernel graph may not depend on CORE (C1).

Alternatives considered:
- **One child per element kind including the envelope in each.** Rejected: the frame slot, error enum and kind guards would be duplicated, and REQ-GEO.TYPES-001 needs one closed kind set.
- **Digest inside each storage child.** Rejected: the canonical layout must be one documented format across kinds (REQ-GEO.TYPES-005), so it is owned once.
- **A separate scanner-structured cloud kind.** Rejected for now, following the open question: scanner structure is attributes on `PointCloud` (REQ-GEO.TYPES.POINTS-006).

## Open questions

- Whether a point cloud with scanner structure (organised grids, per-scan origins) is a separate kind or attributes on `PointSet`. Leaning towards attributes; IO's E57/PTX readers should confirm.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GEO.TYPES.ENV | Element envelope and kind tags (leaf) | 001 002 | none |
| GEO.TYPES.POINTS | Point sets, point clouds and per-point attributes (leaf) | 001 003 004 006 | none |
| GEO.TYPES.MESH | Indexed triangle mesh and topology facts (leaf) | 001 003 004 006 | none |
| GEO.TYPES.DIGEST | Canonical byte layout and element digest (leaf) | 002 005 | none |
| GEO.TYPES.REG | Element registration and describe command (leaf) | 007 008 | none |
