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
built_in: [fid-geo-dist]
built_pr: 599
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

- **AC-1** Given the cube from fixture `fixtures/geo/cad/topo/cube_labelled.json`, when query points outside and inside near every edge and corner are signed (seed 29, 10 000 points), then all outside points are positive and all inside points negative. (The fixture gives each line's direction in `axis`, while a GEO.NURBS.ANALYTIC line runs along `ref_dir`; the test reads the cube's lines with the two vectors swapped, see Open questions.)
- **AC-2** Given the cylinder-with-hole `fixtures/geo/dist/face/incid/tube.json` (invented, written by `make_incid_cases.py`; to be re-pointed to the cylinder-with-hole of `fixtures/geo/cad/tess/analytic_set` when GEO.CAD.TESS lands), then points inside the hole (outside the material) are positive at the hole's edge rim.
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

- **Built in `fid-geo-dist` `face::incid`.** The prepared form is `Incid::new(&Body)` (or `Incid::from_parts(Adjacency, Pcurves)`, to share GEO.CAD.TOPO.PCURVE's pcurves with GEO.DIST.FACE.ONE) with `incident_at(&FaceHit) -> Result<Vec<Incident>, Undefined>` and `signed(p, &FaceHit) -> Result<Signed, IncidError>`: the Design's free functions need the pcurves as well as the body and adjacency (the trim curves of the edge rule), and computation failures (an uncertified inversion, an evaluation error) are errors, never a guessed sign. `Undefined` is either the sign flag of REQ-004 (`Sign { reason, entity }`) or such an error. `Incident` also records whether the degenerate-normal fallback was used.
- Judgement calls, stated in the module documentation: an interior foot has weight 2 pi; an edge "with exactly two faces" is read as an edge with exactly two coedges, so a seam (one face used twice) is a smooth edge, not a free one; a vertex is also undefined when an edge at it has one or more than two coedges (`Boundary`, `NonManifold`) or when its corners do not link its edges into one fan (`NonManifoldVertex`, as in GEO.DIST.SIGN); pole coedges (no 3-D curve, both ends one vertex, image within 1e-8 m of it) are passed over. A `uv` is used only when its surface point is within `MAP_TOL_M` = 1e-6 m [unverified] of the foot; otherwise the foot is inverted onto the face (certified only) and refused if still off. A corner angle within 2^-30 rad of 0 or 2 pi is `Degenerate`.
- Degenerate normals (review of #599): a point is near-degenerate when `|Su x Sv| <= 1e-6 Lu Lv` (face derivative scales). There the normal is sampled on a ring of 17 parameters of the collapsing direction, 1e-9 of the domain off the singular point, on the face's own sheet only (the side where its trim curves lie). Agreeing ring normals (a sphere or NURBS pole) give their mean; disagreeing ones (a cone apex) give `SignUndefined::Degenerate`. A foot at a cone apex is therefore unsigned by rule, never guessed (tests `cone_apex_r0_0_half_angle_30`, `cone_apex_r0_3_half_angle_20_beyond_the_tip`, oracle sections `cone C30` and `cone C20`). The Design's "fixed small inward parameter offset" with one normal is replaced by this rule.
- NURBS seams: an interior foot of a NURBS face on a seam edge (within 1e-8 m) is signed by the two-coedge edge rule, because the surface may be only C0 there. A seam that cannot be tested exactly gives `Degenerate`. Concave edges and corners are covered by the oracle's `lblock` section (`lblock.json`).
- AC-2 uses `fixtures/geo/dist/face/incid/tube.json` (an invented solid cylinder with a coaxial through hole, written by `make_incid_cases.py` with the oracle) because the GEO.CAD.TESS `analytic_set` fixtures do not exist yet; re-point the test when they land.
- `fixtures/geo/cad/topo/cube_labelled.json` gives each line's direction in `axis` (its `ref_dir` is a perpendicular), while a GEO.NURBS.ANALYTIC line runs along the reference direction, so read literally its edge curves miss their far vertices. The AC-1 test reads them with the two vectors swapped; read literally, a corner foot is refused (`Mapping`), not signed with a guessed angle (test `inconsistent_edge_curves_fail_closed`). Raised for GEO.CAD.TOPO.MODEL.

## Children

_Leaf: none._
