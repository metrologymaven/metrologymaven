---
id: GEO.DIST.FACE.INCID
title: Incident normals and angles for CAD signed distance
level: 4
parent: GEO.DIST.FACE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.DIST.FACE-001, REQ-GEO.DIST.FACE-006]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL, GEO.CAD.TOPO.ADJ, GEO.CAD.TOPO.PCURVE, GEO.NURBS.EVAL, GEO.DIST.SIGN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.DIST.FACE.INCID: Incident normals and angles for CAD signed distance

## Purpose

GEO.DIST.SIGN defines one pseudo-normal rule for meshes and CAD; for CAD the caller supplies the outward normals of the faces incident at the foot point and their incident angles. This leaf computes those inputs from B-rep topology: one normal for an interior foot, the two face normals (each weighted π) at an edge foot, and the face normals weighted by the face's interior angle at a vertex foot, with face sense applied, then calls `cad_sign`.

## Scope

**In:**
- Interior foot: the face's outward normal at (u, v) (surface normal times face sense)
- Edge foot: the parameter of the foot on the edge, mapped to each incident face through its coedge's trim curve, then each face's outward normal there, angle π each
- Vertex foot: every face around the vertex, its outward normal at the vertex, and its interior angle between the two coedge tangents at the vertex (in the tangent plane)
- Degenerate normals (surface poles, such as a sphere's apex): fall back to the normal at a parameter offset of 1e-9 of the domain into the face, in fixed direction order
- `SignUndefined` passthrough for non-manifold or open edges

**Out** (and which node owns it instead):
- The weighting rule and zero-distance convention (owned by `GEO.DIST.SIGN`)
- Finding the foot (owned by `GEO.DIST.FACE.ONE`)

## Requirements

- **REQ-GEO.DIST.FACE.INCID-001** When the foot is interior to a face, the system shall use that face's outward normal at the foot parameters, with face sense applied.
- **REQ-GEO.DIST.FACE.INCID-002** When the foot is on an edge shared by two faces, the system shall pass both faces' outward normals at the foot with incident angle π each to GEO.DIST.SIGN's CAD rule.
- **REQ-GEO.DIST.FACE.INCID-003** When the foot is on a vertex, the system shall pass every incident face's outward normal at the vertex weighted by that face's interior angle at the vertex, in ascending face ID order.
- **REQ-GEO.DIST.FACE.INCID-004** If the edge has fewer or more than two faces, then the system shall return the sign as undefined (`sign_defined = false`) rather than guess.
- **REQ-GEO.DIST.FACE.INCID-005** The computation shall be deterministic class D0.

## Acceptance criteria

- **AC-1** Given the cube from fixture `fixtures/geo/cad/topo/cube_labelled.json`, when query points outside and inside near every edge and corner are signed (seed 29, 10 000 points), then all outside points are positive and all inside points negative.
- **AC-2** Given the cylinder-with-hole from `fixtures/geo/cad/tess/analytic_set`, then points inside the hole (outside the material) are positive at the hole's edge rim.
- **AC-3** Given fixture `fixtures/geo/dist/face/open_sheet.json` (a single face with free edges), then a foot on a free edge gives `sign_defined = false`.
- **AC-4** Given a sphere apex foot, then the fallback normal is used and the sign is correct.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Incident { normal: [f64; 3], angle: f64 }`; `fn incident_at(&Body, &Adjacency, foot: &FaceHit) -> Result<Vec<Incident>, Undefined>`.
- **Algorithm:** map the foot feature to faces via GEO.CAD.TOPO.ADJ; evaluate normals with GEO.NURBS.EVAL; vertex interior angles from coedge tangents via `fid-num::atan2`; call `dist::sign::cad_sign`.
- **Errors:** none; undefined sign is a flag.
- **Determinism:** D0.
- **Performance budget:** under 2 µs per edge or vertex foot [unverified].
- **Crate:** `fid-geo-dist` (L0, kernel graph, KNP-1 applies).

## Test fixtures

- `cube_labelled.json` (GEO.CAD.TOPO.MODEL), `analytic_set` (GEO.CAD.TESS), `open_sheet.json` (hand-written). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-dist` (L0, no I/O, KNP-1), add module `face::incid` with `incident_at(&Body, &Adjacency, &FaceHit) -> Result<Vec<Incident>, Undefined>` and `signed(&Body, &Adjacency, p, &FaceHit) -> Signed`. Interior: face outward normal (surface normal × face sense). Edge: map the foot to each incident face via its coedge trim curve (`fid-geo-cad::topo::pcurve`), normals weighted π each. Vertex: each incident face's outward normal weighted by its interior angle between coedge tangents (`fid-num::atan2`), ascending face ID. Edges without exactly two faces give `sign_defined = false`. Degenerate normals use a fixed small inward parameter offset. Pass to `dist::sign::cad_sign`. Acceptance: AC-1 cube edges and corners; AC-2 hole rim; AC-3 free edge undefined; AC-4 sphere apex. Constraints: CONSTITUTION §8, KNP-1. Files: `crates/fid-geo-dist/src/face/incid.rs`, `tests/face_incid.rs`, `fixtures/geo/dist/face/open_sheet.json`.

## Open questions

- None.

## Children

_Leaf: none._
