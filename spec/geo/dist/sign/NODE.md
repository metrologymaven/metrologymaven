---
id: GEO.DIST.SIGN
title: Signed distance and pseudo-normal rules
level: 3
parent: GEO.DIST
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-dist]
built_pr: 313
satisfies: [REQ-GEO.DIST-003]
covers: []
depends_on: [GEO.DIST.MESH, GEO.TYPES.MESH, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.DIST.SIGN: Signed distance and pseudo-normal rules

## Purpose

Defines, once, which side of a surface a point is on: positive along the outward normal, with a documented pseudo-normal rule when the closest point lies on an edge or a vertex. The same rule is used for meshes and, through a generic interface, for CAD faces, so deviation colours, alignments and GD&T agree on sign.

## Scope

**In:**
- Angle-weighted pseudo-normals for mesh vertices and edges, precomputed per mesh
- Sign of a mesh projection from its foot feature (interior, edge, vertex)
- A generic rule for CAD: the caller supplies the incident face normals (with face orientation applied) and incident angles at the foot point; the same weighting is applied
- `SignUndefined` when the mesh is not consistently oriented or not manifold at the foot feature
- Zero-distance convention

**Out:**
- Finding the foot point (owned by `GEO.DIST.MESH` and `GEO.DIST.FACE`)
- Face orientation in B-rep (owned by `GEO.CAD.TOPO`)

## Requirements

- **REQ-GEO.DIST.SIGN-001** For a foot point in a triangle interior, the sign shall be the sign of (p − foot)·n with n the triangle's normal from its vertex order (right-hand rule).
- **REQ-GEO.DIST.SIGN-002** For a foot point on an edge or vertex, the sign shall use the angle-weighted pseudo-normal of that feature (Bærentzen and Aanæs, IEEE TVCG 11(3), 2005), which gives the correct sign for closed, consistently oriented manifold meshes.
- **REQ-GEO.DIST.SIGN-003** If a possible foot feature is on a boundary, non-manifold or inconsistently oriented edge, on a vertex touching one, on a vertex whose triangles form more than one fan, on a degenerate triangle, or anywhere in a connected component (triangles sharing edges) that contains a non-manifold or misoriented edge, then the result shall carry `SignUndefined` together with the unsigned distance. (Which side of a misoriented pair is flipped cannot be decided locally, so the whole component has no defined orientation.)
- **REQ-GEO.DIST.SIGN-004** When the computed distance is exactly zero and no possible foot feature is undefined (REQ-003), the result shall be `OnSurface` with signed distance +0.0 and a proven bound on the exact distance (which need not be zero); when a possible foot feature is undefined, the result shall be `SignUndefined` with distance +0.0.
- **REQ-GEO.DIST.SIGN-005** The CAD form of the rule shall take incident unit normals and incident angles as input, so GEO.DIST.FACE produces signs by the same weighting.
- **REQ-GEO.DIST.SIGN-006** Pseudo-normal precomputation shall be deterministic class D0 and use ordered (sorted) edge tables, not hash maps.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/types/mesh_cube` and 10⁵ seeded points inside and outside the cube, including points closest to edges and corners, when signed distances are computed, then every sign agrees with an exact inside/outside test.
- **AC-2** Given fixture `fixtures/geo/dist/sphere_concave_convex` (a closed mesh with concave and convex edges), when queried at 10⁴ points near edges, then signs agree with the analytic signed distance of the generating surface at points outside the mesh-approximation band (exact mesh side equals analytic side); every other point agrees with the exact mesh inside test or is Undecidable.
- **AC-3** Given fixture `fixtures/geo/types/mesh_open_flipped`, when a point projects onto a boundary edge or the flipped region, then the result carries `SignUndefined` and the correct unsigned distance.
- **AC-4** Given a point exactly on the surface whose computed distance is zero, then the signed distance is +0.0 (bit pattern 0x0): `OnSurface` with a bound on the exact distance, or `SignUndefined` on an undefined feature (REQ-004).
- **AC-5** Given a synthetic CAD-style input (two incident face normals at a convex edge with angles π), when the CAD rule is applied, then the sign matches the mesh rule for the same geometry.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `PseudoNormals { vertex: Vec<[f64; 3]>, edge: Vec<(u64, u64, [f64; 3])> sorted }`; `Signed { distance: f64, sign_defined: bool }`; `fn cad_sign(p, foot, incident: &[(normal, angle)])`.
- **Algorithm:** precompute triangle normals and incident angles; accumulate angle-weighted vertex normals in triangle index order; edge pseudo-normals as the sum of the two face normals (each weighted π); look up by foot feature.
- **Errors:** none; undefined sign is a flag, not an error.
- **Determinism:** D0.
- **Performance budget:** precomputation for 10⁶ triangles under 1 s [unverified].
- **Crate:** `fid-geo-dist` (kernel graph, KNP-1 applies).

## Test fixtures

- `mesh_cube`, `mesh_open_flipped` (GEO.TYPES); `sphere_concave_convex` generated from an analytic star-shaped surface by `tools/fixtures/geo_dist.py`. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-dist` (pure Rust, no I/O, KNP-1), add module `sign`: `PseudoNormals::build(mesh)` computing angle-weighted vertex pseudo-normals and edge pseudo-normals (Bærentzen and Aanæs 2005; write your own code) with sorted edge tables in triangle-index accumulation order; `signed(hit: &MeshHit, p, pn) -> Signed` using the triangle normal for interior feet and pseudo-normals for edges and vertices; `SignUndefined` (flag `sign_defined = false`) at boundary, non-manifold or misoriented features; zero distance gives +0.0; and `cad_sign(p, foot, incident: &[(normal, angle)])` applying the same weighting for CAD callers. Acceptance: AC-1 cube signs vs exact inside test for 10⁵ points; AC-2 concave/convex edges vs analytic sign; AC-3 undefined sign on open/flipped mesh; AC-4 +0.0; AC-5 CAD rule agrees with mesh rule. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no FMA, no HashMap, `fid-num` for sqrt and acos). Files: `crates/fid-geo-dist/src/sign.rs`, `tests/sign.rs`.

## Open questions

- None.

## Children

_Leaf: none._
