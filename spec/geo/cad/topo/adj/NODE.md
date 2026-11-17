---
id: GEO.CAD.TOPO.ADJ
title: Topology adjacency queries
level: 4
parent: GEO.CAD.TOPO
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TOPO-002, REQ-GEO.CAD.TOPO-006]
covers: []
depends_on: [GEO.CAD.TOPO.MODEL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TOPO.ADJ: Topology adjacency queries

## Purpose

Answers "what touches what" on a built body: faces of an edge, edges and loops of a face, faces of a body, faces around a vertex and neighbouring faces, always in ascending `TopologyId` order, so selection growth, sign rules at edges and validation all see the same deterministic neighbourhoods.

## Scope

**In:**
- An `Adjacency` index built once per body from sorted incidence vectors
- Queries: `faces_of_edge`, `edges_of_face`, `loops_of_face`, `faces_of_body`, `faces_of_vertex`, `edges_of_vertex`, `face_neighbours` (faces sharing at least one edge), `coedges_of_edge`

**Out** (and which node owns it instead):
- The arena and IDs (owned by `GEO.CAD.TOPO.MODEL`)
- Defect detection such as non-manifold edges (owned by `GEO.CAD.TOPO.VALID`; it uses these queries)

## Requirements

- **REQ-GEO.CAD.TOPO.ADJ-001** Each adjacency query shall return entity IDs in ascending `TopologyId` order with no duplicates.
- **REQ-GEO.CAD.TOPO.ADJ-002** If a query names an ID that is not in the body or is of the wrong kind, then it shall return `UnknownEntity` or `WrongKind` rather than an empty list.
- **REQ-GEO.CAD.TOPO.ADJ-003** Building the adjacency index shall take O(n log n) time in the number of coedges and use no hash containers.
- **REQ-GEO.CAD.TOPO.ADJ-004** `face_neighbours(f)` shall exclude `f` itself even when `f` meets itself along a seam edge.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/topo/cube_labelled.json`, when every query runs for every entity, then results equal the hand-written expectations in `cube_adjacency.json` (each face has 4 neighbours, each edge 2 faces).
- **AC-2** Given fixture `fixtures/geo/cad/topo/cylinder_seam.json` (a cylinder face closed by a seam edge), when `faces_of_edge(seam)` runs, then it returns the one face once, and `face_neighbours` of that face excludes it.
- **AC-3** (error path) Given an edge ID passed to `edges_of_face`, then the result is `WrongKind`.
- **AC-4** Given a random 10⁵-face body generated with seed 3, when queries run against a brute-force scan, then all results agree.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Adjacency { edge_faces: Vec<(EdgeIx, FaceIx)>, vertex_edges: Vec<(VertexIx, EdgeIx)>, ... }` sorted, with range lookups by binary search; outputs mapped to `TopologyId` and sorted.
- **Algorithm:** one pass over coedges emitting incidence pairs, stable sort, deduplicate.
- **Errors:** `UnknownEntity`, `WrongKind`.
- **Determinism:** D0.
- **Performance budget:** index build for 10⁵ faces under 50 ms [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies).

## Test fixtures

- `cube_labelled.json` (from GEO.CAD.TOPO.MODEL), `cube_adjacency.json` (hand-written), `cylinder_seam.json` (hand-written), seeded random generator in `tests/`. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, KNP-1, no I/O), add module `topo::adj` with `Adjacency::build(&Body)` and queries `faces_of_edge`, `edges_of_face`, `loops_of_face`, `faces_of_body`, `faces_of_vertex`, `edges_of_vertex`, `face_neighbours`, `coedges_of_edge`, each returning `Vec<TopologyId>` ascending and deduplicated; unknown or wrong-kind IDs return `UnknownEntity` / `WrongKind`. Build from sorted incidence vectors (no hash maps); `face_neighbours` excludes the face itself. Acceptance: AC-1 cube expectations; AC-2 seam edge; AC-3 `WrongKind`; AC-4 brute-force agreement on a seeded 10⁵-face body. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-cad/src/topo/adj.rs`, `tests/topo_adj.rs`, `fixtures/geo/cad/topo/cube_adjacency.json`, `cylinder_seam.json`.

## Open questions

- None.

## Children

_Leaf: none._
