---
id: GEO.CAD.NOMINAL.DIRECT
title: Nominal element types and direct analytic mapping
level: 4
parent: GEO.CAD.NOMINAL
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.NOMINAL-001, REQ-GEO.CAD.NOMINAL-006]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.ADJ, GEO.NURBS.ANALYTIC]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.NOMINAL.DIRECT: Nominal element types and direct analytic mapping

## Purpose

Defines the typed nominal element (plane, cylinder, cone, sphere, torus, line, circle, point) with its extent and material side, and maps faces, edges and vertices whose CAD geometry is already analytic straight to it, copying parameters exactly so inspection refers to the designer's values rather than a refit.

## Scope

**In:**
- `NominalElement { kind, placement, params, extent, material, sources, method, max_deviation_m }`
- `NominalKind { Plane, Cylinder, Cone, Sphere, Torus, Line, Circle, Point }`
- Extent: parameter range of the face (axial range for cylinder and cone, angular range for arcs, in-plane box for planes) computed from boundary vertices and edge extrema
- Material side from face sense: `Outside` (shaft, boss) or `Inside` (hole, pocket); plane normal pointing out of material
- Mapping: analytic face → surface kind; line and circle edges → `Line` and `Circle` (arc start and end angles kept); vertex → `Point`
- `DirectResult { Element(NominalElement), NotAnalytic, UnsupportedKind { kind } }` (for example ellipse edges)

**Out** (and which node owns it instead):
- Recognising analytic shapes encoded as NURBS (owned by `GEO.CAD.NOMINAL.RECOG`)
- Merging faces of one primitive (owned by `GEO.CAD.NOMINAL.MERGE`)
- The command (owned by `GEO.CAD.NOMINAL.CMD`)

## Requirements

- **REQ-GEO.CAD.NOMINAL.DIRECT-001** When a face carries an analytic plane, cylinder, cone, sphere or torus, the system shall return the matching `NominalElement` whose placement and radii are bit-identical to the CAD definition, with `method = Direct` and `max_deviation_m = 0`.
- **REQ-GEO.CAD.NOMINAL.DIRECT-002** When an edge carries a line or circle, or the input is a vertex, the system shall return a `Line`, `Circle` or `Point` with parameters copied exactly.
- **REQ-GEO.CAD.NOMINAL.DIRECT-003** The system shall set `material` from the face sense and the surface's natural normal, so a reversed cylinder face (a hole) gives `Inside`.
- **REQ-GEO.CAD.NOMINAL.DIRECT-004** If the geometry is NURBS, then the system shall return `NotAnalytic`; if it is an analytic kind with no nominal type (for example an ellipse), then `UnsupportedKind`; never an approximate element.
- **REQ-GEO.CAD.NOMINAL.DIRECT-005** Extents shall be computed with fixed-order arithmetic under KNP-1, so results are class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/nominal/analytic_block.json` (a block with a through hole, a boss, a spherical dimple, a conical countersink and a torus fillet, all analytic), when every face is mapped, then each element's parameters are bit-identical to the fixture definitions and the hole and countersink are `Inside`.
- **AC-2** Given the circular hole edge, then a `Circle` with the exact radius and a full 2π range is returned; given a corner vertex, a `Point`.
- **AC-3** (error path) Given a NURBS face, then `NotAnalytic`; given an ellipse edge, `UnsupportedKind { Ellipse }`.
- **AC-4** Given AC-1 on three OSes, then extents are bit-identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** as in Scope; `Extent { Axial { h_min, h_max }, Angular { a0, a1 }, Planar { u: [f64; 2], v: [f64; 2] }, Unbounded }`; `Material { Outside, Inside }`; `Method { Direct, Recognised }`.
- **Algorithm:** match on GEO.NURBS.ANALYTIC surface and curve kinds; extents from vertex positions and edge extrema in the local placement (circle extrema in closed form).
- **Errors:** none beyond the result variants.
- **Determinism:** D0.
- **Performance budget:** under 5 µs per face [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `fixtures/geo/cad/nominal/analytic_block.json`: a hand-written body description with binary-exact parameters. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, no I/O, KNP-1), add module `nominal::types` with `NominalElement`, `NominalKind`, `Extent`, `Material`, `Method`, and module `nominal::direct` with `map_face`, `map_edge`, `map_vertex` returning `DirectResult`. Copy placement and radii bit-identically from `fid-geo-nurbs` analytic surfaces and curves (`method = Direct`, deviation 0); set material from face sense; compute extents in the local placement with fixed-order arithmetic. NURBS gives `NotAnalytic`, ellipse gives `UnsupportedKind`; never approximate. Acceptance: AC-1 analytic block bit-identical with correct material sides; AC-2 circle and point; AC-3 `NotAnalytic` and `UnsupportedKind`; AC-4 identical extents on three OSes. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-cad/src/nominal/types.rs`, `src/nominal/direct.rs`, `tests/nominal_direct.rs`, `fixtures/geo/cad/nominal/analytic_block.json`.

## Open questions

- The nominal element type is defined here in GEO (L0). KRN.CONS.AUTO and ALN.DATUM.ELEM also handle nominal elements; whether they reuse this type or a KRN-owned feature type should be agreed with KRN (inherited from the parent's open question). Both are L0, so either direction is layer-legal.

## Children

_Leaf: none._
