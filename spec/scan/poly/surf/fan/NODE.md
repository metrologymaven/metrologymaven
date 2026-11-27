---
id: SCAN.POLY.SURF.FAN
title: Local tangent-plane Delaunay fans
level: 4
parent: SCAN.POLY.SURF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-scan-poly]
satisfies: [REQ-SCAN.POLY.SURF-003, REQ-SCAN.POLY.SURF-006]
covers: []
depends_on: [SCAN.POLY.SURF.PREP, GEO.INDEX.KDTREE, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.POLY.SURF.FAN: Local tangent-plane Delaunay fans

## Purpose

The first half of the chosen reconstruction (localized tangent-plane Delaunay, after Gopi, Krishnan and Silva 2000 [unverified citation detail]): for each vertex independently, project its neighbours within the maximum gap onto its tangent plane, compute the 2D Delaunay triangulation of that neighbourhood with exact predicates, and keep the fan of triangles around the vertex. Because each fan depends only on the vertex's neighbourhood, the result is independent of processing order, which is what makes chunked processing give the same mesh.

## Scope

**In:**
- Neighbourhood: vertices within radius g (maximum gap) whose normals are within 60° of the centre's normal, at most 32 nearest (ties lowest index)
- Projection to the tangent plane of the centre's normal, with a fixed in-plane basis
- 2D Delaunay of the projected points with exact orientation and in-circle predicates; the centre's fan = its incident Delaunay triangles whose edges are all shorter than g
- `Fan { centre, neighbours: [u64] in angular order, triangles: [[u64;3]] }`

**Out** (and which node owns it instead):
- Agreement between fans and mesh assembly (owned by `SCAN.POLY.SURF.STITCH`)

## Requirements

- **REQ-SCAN.POLY.SURF.FAN-001** For each vertex, the fan shall be computed only from that vertex's neighbourhood (radius g, normal compatibility 60°, at most 32 neighbours ordered by (distance², index)), with no dependence on other fans or on processing order.
- **REQ-SCAN.POLY.SURF.FAN-002** The 2D Delaunay triangulation shall use exact-sign orientation and in-circle predicates without FMA, with cocircular ties broken by a symbolic perturbation ordered by vertex index.
- **REQ-SCAN.POLY.SURF.FAN-003** No fan triangle shall have an edge of length g or more, so gaps wider than the maximum gap produce no triangles.
- **REQ-SCAN.POLY.SURF.FAN-004** Fans shall be determinism class D0 and bit-identical for any thread count.

## Acceptance criteria

- **AC-1** Given a regular hexagonal grid patch, every interior vertex's fan has exactly 6 triangles matching the analytic triangulation.
- **AC-2** Given a square grid (all cocircular quadruples), fans are identical across runs and platforms and consistent with the index-ordered perturbation.
- **AC-3** Given two patches separated by a gap of 1.5 g, no fan triangle spans the gap.
- **AC-4** Given vertex order shuffled, each vertex's fan is identical.

## Design

- **API** (`fid-scan-poly::fan`): `fn fans(vs: &VertexSet, g: f64, threads: usize) -> Result<Vec<Fan>, PolyError>`.
- **Algorithm:** k-d tree radius query; basis from the normal by a fixed rule (first axis = normalised cross of n with the coordinate axis least aligned to n); incremental Delaunay in index order with exact predicates; extract the star of the centre.
- **Errors:** `scan.poly.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-poly`.

## Test fixtures

`hex_grid`, `square_grid`, `two_patches_gap` (generated, CC0).

## Build prompt

> In the Rust crate `fid-scan-poly`, module `fan`, implement `fans(vertex_set, g, threads)`: for each vertex independently, gather up to 32 neighbours within g with normals within 60° (k-d tree, (dist², index) order), project to the centre's tangent plane with a fixed basis rule, build the 2D Delaunay triangulation with exact-sign orientation and in-circle predicates (no FMA; symbolic perturbation by vertex index for cocircular ties), and keep the centre's incident triangles with all edges shorter than g. Output must not depend on vertex order or threads. Constraints: KNP-1, §8. Tests: AC-1..AC-4.

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** use KRN.NUM.PRED for all in-circle and orientation tests.
- Exact predicates are assumed from `KRN.NUM`, which has no predicates child yet (shared gap with `SCAN.MESHOP.CUT`); `KRN.MZ.VOR` holds the only current implementation.
- **Built (fid-scan-poly `fan`):** `KRN.NUM.PRED` is not built yet, so the fans use the exact-sign `orient2d`/`incircle` of `KRN.MZ.VOR` (`fid_fit::vor`), the code that `KRN.NUM.PRED` is scoped to migrate; switch the import when it lands. The output is the unique Delaunay triangulation of the perturbed neighbourhood, computed by an empty-circumcircle test over the centre's candidate triangles rather than by incremental insertion (same triangles, no order dependence). "Vertex index" for ties and the perturbation is the vertex's `source`, so AC-4 holds for a shuffled `VertexSet`. Pinned choices beyond this node: `g` in [2^-100, 2^100] m, projected coordinates below 2^-200 m flushed to zero (keeps the predicates exact), short edge = `dist2 < (g*g)(1 - 2^-46)`, the largest source is lifted most, fan order by angle from the first basis axis. Performance against REQ-SCAN.POLY.SURF-007 is not measured.

## Children

_Leaf: none._
