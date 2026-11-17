---
id: SCAN.POLY.SURF.STITCH
title: Fan agreement, manifold assembly and orientation
level: 4
parent: SCAN.POLY.SURF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY.SURF-003, REQ-SCAN.POLY.SURF-006]
covers: []
depends_on: [SCAN.POLY.SURF.FAN, GEO.TYPES.MESH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.POLY.SURF.STITCH: Fan agreement, manifold assembly and orientation

## Purpose

The second half of the reconstruction: keep a triangle only when the fans of its vertices agree on it, then resolve any remaining non-manifold edges by a fixed rule, orient triangles by the input normals, and produce the indexed mesh. Holes wider than the gap stay open because no fan proposes triangles across them.

## Scope

**In:**
- Agreement: a triangle (as a sorted vertex triple) is accepted when it appears in the fans of all three vertices; when exactly two agree, it is accepted only if it does not create a non-manifold edge with accepted triangles
- Non-manifold cleanup: on an edge used by more than two triangles, keep the two with the smallest maximum angle deviation of their face normal from the vertex normals, ties lowest triples
- Orientation of each triangle so its face normal agrees with the mean of its vertex normals
- `reconstruct_in_memory(vertex_set, params) -> Mesh`

**Out** (and which node owns it instead):
- Fans (owned by `SCAN.POLY.SURF.FAN`); chunking (owned by `SCAN.POLY.SURF.OOC`)

## Requirements

- **REQ-SCAN.POLY.SURF.STITCH-001** A triangle shall be accepted when all three of its vertices' fans contain it, and a triangle in exactly two fans shall be accepted only if no accepted triangle set would then use any of its edges more than twice, evaluated in ascending sorted-triple order.
- **REQ-SCAN.POLY.SURF.STITCH-002** The output shall have zero non-manifold edges, with excess triangles removed by the stated rule.
- **REQ-SCAN.POLY.SURF.STITCH-003** Each triangle's winding shall make its face normal have a positive dot product with the sum of its vertex normals.
- **REQ-SCAN.POLY.SURF.STITCH-004** Regions where no fan triangle exists (gaps wider than the maximum gap) shall remain open boundary loops.
- **REQ-SCAN.POLY.SURF.STITCH-005** The output mesh shall depend only on the set of fans, not on their order, and shall be D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/poly/sphere_points` (oriented points on a 50 mm sphere, 0.5 mm spacing) and g = 1.5 mm, the mesh is closed, has zero non-manifold edges and all face normals point outward.
- **AC-2** Given fixture `plate_gap` with a 5 mm slot and g = 1.5 mm, the slot remains one boundary loop.
- **AC-3** Given the fans in reversed order, the mesh digest is identical.

## Design

- **API** (`fid-scan-poly::stitch`): `fn stitch(fans: &[Fan], vs: &VertexSet) -> Result<Mesh, PolyError>`; `fn reconstruct_in_memory(cloud, valid, params) -> Result<Mesh, PolyError>` = PREP → FAN → STITCH.
- **Algorithm:** collect (triple, vote count) by sorting all fan triangles; accept 3-vote triples, then 2-vote triples in ascending order with an edge-use table; cleanup; orientation by sign of a dot product.
- **Errors:** `scan.poly.empty_mesh`.
- **Determinism:** D0.
- **Crate:** `fid-scan-poly`.

## Test fixtures

`sphere_points`, `plate_gap` (seeded synthetic, CC0).

## Build prompt

> In the Rust crate `fid-scan-poly`, module `stitch`, implement `stitch(fans, vertex_set) -> Mesh`: sort all fan triangles as sorted triples and count votes; accept 3-vote triples, then 2-vote triples in ascending order if they keep every edge at most twice used; remove excess triangles on any remaining non-manifold edge (smallest maximum normal deviation, ties lowest triple); orient each triangle so its face normal agrees with the sum of its vertex normals. Add `reconstruct_in_memory` composing `prep::prepare`, `fan::fans` and `stitch`. Output independent of fan order. Constraints: KNP-1 (no std HashMap/HashSet), §8, D0. Tests: AC-1..AC-3.

## Open questions

- Mesh quality on thin walls (two sheets closer than g) is a known weakness of local methods; the 60° normal compatibility in FAN mitigates it [unverified]. Measure on the corpus before release.

## Children

_Leaf: none._
