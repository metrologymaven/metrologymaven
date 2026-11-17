---
id: GEO.CAD
title: CAD bodies, topology, groups and tessellation
level: 2
parent: GEO
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-GEO-001, REQ-GEO-007]
covers: [GOM.CAD.02, GOM.CAD.03, GOM.CAD.04, SA.CAD.03]
depends_on: [GEO.TYPES, GEO.FRAME, GEO.NURBS]
cli: [fid geo tessellate-*, fid geo extract-nominal*, fid geo *-group, fid geo *-layer]
mcp: [geo_tessellate_*, geo_extract_nominal*, geo_*_group, geo_*_layer]
journal: [geo.tessellate_*@*, geo.extract_nominal*@*, geo.*_group@*, geo.*_layer@*]
updated: 2026-11-16
---

# GEO.CAD: CAD bodies, topology, groups and tessellation

## Purpose

The exact-CAD side of the geometry model: B-rep bodies with shells, faces, loops, edges and vertices on top of NURBS and analytic geometry; CAD groups, parts and layers; controlled tessellation into meshes for display and indexing; and generation of nominal geometric elements (planes, cylinders, circles and so on) from CAD faces and edges. It gives selection, comparison, alignment and GD&T a stable, addressable view of the nominal part.

## Scope

**In:**
- B-rep topology model: body, shell, face, loop, edge, vertex, with stable sub-entity IDs preserved from import
- Trimmed faces (surface plus trimming loops) and edge curves
- Topology queries: adjacency, face-to-edge, edge-to-face, body membership
- CAD groups, parts, assemblies (flattened instance placement) and layers: create, rename, assign, show/hide state as data
- Tessellation of faces and bodies with chordal-deviation and angular tolerances, watertight along shared edges
- Nominal element generation from faces and edges: recognise analytic primitives and produce typed nominal elements
- The choice of B-rep kernel (own crate vs out-of-process OCCT), which is an open question here

**Out** (and which node owns it instead):
- STEP, IGES and native CAD parsing, and PMI (owned by `IO`)
- Curve and surface evaluation (owned by `GEO.NURBS`)
- Picking faces and storing face selections (owned by `GEO.SEL`)
- CAD display and shading (owned by `VIZ`)
- Point-to-face distance (owned by `GEO.DIST`)
- Fitting features to measured points (owned by `KRN`)

## Requirements

- **REQ-GEO.CAD-001** A CAD body shall expose its faces, edges and vertices with stable IDs, preserving imported IDs where the source provides them and deriving UUIDv5 IDs otherwise.
- **REQ-GEO.CAD-002** The system shall answer adjacency queries (faces of an edge, edges of a face, faces of a body) for every topology entity.
- **REQ-GEO.CAD-003** When a body is tessellated with chordal tolerance t and angular tolerance a, every mesh vertex shall lie on its source surface within 1e-9 m [unverified] and every triangle shall deviate from the surface by at most t.
- **REQ-GEO.CAD-004** Tessellations of adjacent faces shall share vertices along their common edge, so the mesh of a closed body is watertight.
- **REQ-GEO.CAD-005** Tessellation shall be deterministic class D1 for a given body, tolerance pair and kernel version.
- **REQ-GEO.CAD-006** Changing tessellation tolerance shall be a journaled W2 command, and dependents of the tessellation shall recompute through the CORE dependency graph.
- **REQ-GEO.CAD-007** The system shall let users create, rename and delete CAD groups and layers and assign bodies and faces to them, each as a journaled W2 command.
- **REQ-GEO.CAD-008** When asked to generate a nominal element from a face or edge whose underlying geometry is analytic (plane, cylinder, cone, sphere, torus, line, circle), the system shall produce the corresponding typed nominal element with exact parameters.
- **REQ-GEO.CAD-009** If a face is freeform and no analytic primitive matches within a recognition tolerance, then nominal generation shall return a typed not-recognised result rather than an approximate primitive.
- **REQ-GEO.CAD-010** If a B-rep body is invalid (open trimming loop, dangling edge, inconsistent orientation), then the system shall report each defect by entity ID and shall not silently repair it.

## Interfaces

- **Offers:** `Body`, `Shell`, `Face`, `Loop`, `Edge`, `Vertex`, `TopologyId`, `CadGroup`, `CadLayer`, `Tessellator` with `TessTolerance`, `NominalElement`; commands `geo.tessellate_body@1`, `geo.set_tessellation_tolerance@1`, `geo.extract_nominal@1`, `geo.create_group@1`, `geo.assign_layer@1`.
- **Consumes:** GEO.NURBS evaluators; GEO.FRAME for part and instance placement; GEO.TYPES `Mesh` as tessellation output; IO for imported bodies; CORE dependency graph.

## Decomposition hints

TOPO (B-rep topology and validation), TESS (tessellation), NOMINAL (primitive recognition and nominal elements), GROUP (groups, parts, layers), KERNEL (B-rep kernel boundary; blocked until the open question is decided).

## Open questions

- Should the B-rep kernel be its own crate, or OCCT in a separate process (the baseline uses LGPL OCCT out of process)? This touches licensing (D-001) and the C1 kernel boundary, so whoever expands this node will likely need a decision record.
- Phase set to P1b to match CONSTITUTION §7 (CAD + PMI); confirm in review.

## Children

_None yet: status `open`._
