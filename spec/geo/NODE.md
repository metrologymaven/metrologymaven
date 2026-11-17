---
id: GEO
title: Geometry and spatial data
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-010, REQ-FID-016, REQ-FID-020]
covers: [GOM.CAD.*, SA.CAD.03]
depends_on: [CORE]
cli: [fid geo *]
mcp: [geo_*]
journal: [geo.*]
updated: 2026-11-16
---

# GEO: Geometry and spatial data

## Purpose

The shared geometry model:
- point sets with normals and attributes;
- triangle meshes;
- large out-of-core point clouds;
- exact CAD (B-rep faces, NURBS curves and surfaces);
- coordinate frames and transforms;
- spatial indexes (k-d trees, BVH);
- selections expressed as index sets or semantic queries.

## Scope

**In:**
- Geometry types and their invariants
- Units at the boundary (SI metres inside)
- Frames and transform chains
- Spatial indexing and nearest-neighbour queries
- Point-to-mesh and point-to-CAD distance and projection
- NURBS evaluation
- CAD body, face and group handling, plus tessellation control
- Out-of-core chunking and LOD for clouds of 10⁸+ points
- Selection model

**Out:**
- File parsing (IO)
- Mesh repair and processing operations (SCAN)
- Feature fitting (KRN)
- Rendering (VIZ)

## Requirements

- **REQ-GEO-001** The geometry model shall represent point sets, meshes, point clouds and exact CAD (B-rep with NURBS) as distinct, typed elements that share one frame system.
- **REQ-GEO-002** Every geometric element shall carry its coordinate frame. Transforms between frames shall be explicit and journaled.
- **REQ-GEO-003** The system shall compute point-to-mesh and point-to-CAD-face closest-point projection and signed distance on the CPU, deterministically (class D0 or D1).
- **REQ-GEO-004** Spatial indexes shall answer k-nearest and radius queries on 10⁸ points within memory budgets set per leaf [unverified].
- **REQ-GEO-005** Point clouds larger than memory shall be handled out of core in chunks, with level-of-detail summaries for display.
- **REQ-GEO-006** Selections shall be stored as index sets identified by digest, or as semantic queries, never as screen-space state.
- **REQ-GEO-007** CAD bodies shall expose face, edge and group structure for selection and nominal-element generation, with controllable tessellation tolerance.

## Interfaces

- **Offers:** geometry types, `Frame`, `Transform`, `SpatialIndex`, `Project::closest`, `Selection`, NURBS evaluator, tessellator.
- **Consumes:** CORE (element registration, dependency graph).

## Decomposition hints

TYPES (core types and invariants), FRAME (frames and transforms), INDEX (spatial indexes), DIST (distance and projection), CAD (B-rep, NURBS, tessellation), OOC (out-of-core and LOD), SEL (selection model).

## Split rationale

The split follows the **kind of geometric responsibility**, with data types at the bottom and services layered on top: element types (TYPES) and the frame system (FRAME) are the foundation; exact CAD is split into its numeric evaluator (NURBS) and its topology, grouping and tessellation layer (CAD); spatial services are split into indexing (INDEX), distance and projection (DIST) and out-of-core scale (OOC); and the selection model (SEL) sits on top because it refers to all of them. Each child maps to one crate-sized concern and can be tested on its own fixtures.

Alternatives considered:
- **Split by geometry kind** (points, meshes, clouds, CAD). Rejected: indexing, distance, frames and selection would be duplicated across kinds, and REQ-GEO-001 asks for one shared frame system.
- **One CAD child covering NURBS, B-rep and tessellation**, as the decomposition hint suggested. Rejected: too large for one subsystem, and the NURBS evaluator is a pure numeric component that DIST needs independently of the B-rep kernel choice still open below.
- **Units as a separate child.** Rejected: units only matter at the boundary and are small; they sit with frames in FRAME.

Phase tags on children follow CONSTITUTION §7 (CAD and scan-to-CAD in P1b; types, frames, indexes and selections in P0). They need human confirmation in review.

## Open questions

- Should the B-rep kernel be its own crate, or OCCT in a separate process (the baseline uses LGPL OCCT out of process)? Delegated to GEO.CAD, which will likely need a decision record.
- GEO.SEL (P0) depends on GEO.CAD (P1b) for face selection; P0 selections cover points and triangles only until CAD lands.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GEO.TYPES | Geometry element types and invariants | 001 002 | none |
| GEO.FRAME | Coordinate frames, transforms and units | 001 002 | none |
| GEO.INDEX | Spatial indexes and neighbour queries | 004 | none |
| GEO.NURBS | NURBS curve and surface evaluation | 001 003 | none |
| GEO.CAD | CAD bodies, topology, groups and tessellation | 001 007 | GOM.CAD.02, GOM.CAD.03, GOM.CAD.04, SA.CAD.03 |
| GEO.DIST | Distance and closest-point projection | 003 | none |
| GEO.OOC | Out-of-core point clouds and level of detail | 004 005 | none |
| GEO.SEL | Selection model | 006 007 | GOM.CAD.01 |
