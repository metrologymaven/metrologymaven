---
id: GEO.FRAME
title: Coordinate frames, transforms and units
level: 2
parent: GEO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO-001, REQ-GEO-002]
covers: []
depends_on: [GEO.TYPES, CORE]
cli: [fid geo *-frame, fid geo *-transform, fid geo convert-units]
mcp: [geo_*_frame, geo_*_transform, geo_convert_units]
journal: [geo.*_frame@*, geo.*_transform@*, geo.convert_units@*]
updated: 2026-11-16
---

# GEO.FRAME: Coordinate frames, transforms and units

## Purpose

The single frame system that points, meshes, clouds and CAD share. It defines named coordinate frames, the frame tree, rigid and similarity transforms between frames, how transform chains are composed and resolved, and how lengths enter and leave the kernel's SI-metre interior. Alignment (ALN) and networks (LVM) produce transforms; this node defines what a transform is and how it is applied and journaled.

## Scope

**In:**
- `Frame` entities: world, part, instrument, alignment and user frames, with stable IDs
- The frame tree (each frame has at most one parent frame and one transform to it)
- Transform types: rigid (rotation plus translation) and similarity (rigid plus uniform scale)
- Composition, inversion and chain resolution between any two frames in the tree
- Applying a transform to an element, producing a new element in the target frame
- Rotation parameterisations at the boundary (matrix, quaternion, axis-angle, Euler sequences) and their conversion
- Units at the boundary: authored value plus unit kept, single journaled rounding into metres and out to display units
- Angle units (radians inside)

**Out** (and which node owns it instead):
- Computing alignment transforms (owned by `ALN`)
- Network adjustment between instruments (owned by `LVM`)
- Uncertainty of a transform (owned by `UNC`)
- Element types themselves (owned by `GEO.TYPES`)

## Requirements

- **REQ-GEO.FRAME-001** The frame system shall organise frames as a tree rooted in one world frame per document, and an attempt to create a cycle shall fail with a typed error.
- **REQ-GEO.FRAME-002** When the transform between two frames is requested, the system shall compose the chain through their common ancestor in a fixed, documented order.
- **REQ-GEO.FRAME-003** Every transform that changes a frame relationship or moves an element between frames shall be a registry command of class W2 and shall produce one journal event.
- **REQ-GEO.FRAME-004** If a transform's rotation part deviates from orthonormal by more than 1e-12 in Frobenius norm [unverified], then the system shall reject it with a typed error.
- **REQ-GEO.FRAME-005** Composition and application of rigid transforms shall be deterministic class D0, using fid-num for every transcendental function.
- **REQ-GEO.FRAME-006** A round trip of a point through a transform and its inverse shall return within 1e-12 m of the input for coordinates up to 100 m [unverified].
- **REQ-GEO.FRAME-007** When a length is authored in a non-SI unit, the system shall keep the authored value and unit, convert to metres with a single rounding, and journal the conversion.
- **REQ-GEO.FRAME-008** If an operation receives two elements in different frames without an explicit transform between them, then the system shall fail with a typed frame-mismatch error rather than convert silently.

## Interfaces

- **Offers:** `Frame`, `FrameId`, `Transform` (rigid, similarity), `FrameTree::resolve(a, b)`, `Quantity<Length>`, unit conversion; commands such as `geo.create_frame@1`, `geo.set_transform@1`, `geo.apply_transform@1`, `geo.convert_units@1`.
- **Consumes:** GEO.TYPES elements; CORE registry and journal; fid-num (KRN) for transcendentals.

## Decomposition hints

TREE (frame tree and resolution), XFORM (transform types, composition, rotation parameterisations), APPLY (transforming elements), UNITS (authored quantities and conversion).

## Open questions

- Is a non-uniform scale (e.g. thermal anisotropy compensation) ever needed as a frame transform, or always as a separate correction? Currently out.

## Children

_None yet: status `open`._
