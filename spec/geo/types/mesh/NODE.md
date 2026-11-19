---
id: GEO.TYPES.MESH
title: Indexed triangle mesh and topology facts
level: 3
parent: GEO.TYPES
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-geo-types]
satisfies: [REQ-GEO.TYPES-001, REQ-GEO.TYPES-003, REQ-GEO.TYPES-004, REQ-GEO.TYPES-006]
covers: []
depends_on: [GEO.TYPES.ENV, GEO.TYPES.POINTS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.TYPES.MESH: Indexed triangle mesh and topology facts

## Purpose

The `Mesh` element: a vertex buffer with per-vertex attributes and an indexed triangle list, validated at construction, plus a set of topology facts (boundary edges, non-manifold edges, orientation consistency, closedness) reported as data so that consumers know what they hold. Facts are never repairs; repair belongs to SCAN.

## Scope

**In:**
- `Mesh { frame, vertices: PointBuffer, vertex_attrs: AttributeSet, triangles }`
- Triangle index storage that is 32-bit when the vertex count fits and 64-bit otherwise
- Validation: finite vertices, indexes in range, attribute lengths
- `TopologyFacts`: counts of degenerate triangles (repeated index), boundary edges, non-manifold edges, inconsistently oriented edge pairs, connected components; `closed` and `oriented` flags
- Optional per-triangle attributes (scanner ID, quality)

**Out:**
- Mesh repair, decimation, smoothing (owned by `SCAN`)
- Spatial index over triangles (owned by `GEO.INDEX`)
- Digest (owned by `GEO.TYPES.DIGEST`)

## Requirements

- **REQ-GEO.TYPES.MESH-001** If a triangle references a vertex index not less than the vertex count, then construction shall fail with `IndexOutOfRange { triangle, index }` naming the first offender.
- **REQ-GEO.TYPES.MESH-002** The mesh shall store triangle indexes as u32 when the vertex count is at most 2³² and as u64 otherwise, and the public API shall expose indexes as u64 in both cases.
- **REQ-GEO.TYPES.MESH-003** When a mesh is constructed, the system shall compute `TopologyFacts` and shall not alter any vertex or triangle.
- **REQ-GEO.TYPES.MESH-004** `TopologyFacts` shall report degenerate triangles, boundary edges, non-manifold edges, inconsistently oriented edge pairs and connected components as exact counts, and `closed` shall be true only when boundary and non-manifold counts are both zero.
- **REQ-GEO.TYPES.MESH-005** Topology fact computation shall use a sort-based edge table, not a hash map, so the result is independent of platform and hash seed.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/types/mesh_cube`, when constructed, then facts are: 0 degenerate, 0 boundary, 0 non-manifold, 0 misoriented, 1 component, `closed = true`, `oriented = true`.
- **AC-2** Given fixture `fixtures/geo/types/mesh_open_flipped` (cube with one triangle removed and one flipped), when constructed, then boundary = 3, misoriented edge pairs = 3, `closed = false`, `oriented = false`, and vertex and triangle arrays are unchanged bitwise.
- **AC-3** Given fixture `fixtures/geo/types/mesh_nonmanifold` (three triangles sharing one edge), when constructed, then non-manifold edges = 1.
- **AC-4** (error path) Given `mesh_cube` with triangle 5 index set to 8, when constructed, then it fails with `IndexOutOfRange { triangle: 5, index: 8 }`.
- **AC-5** Given the same mesh built with u32 and forced-u64 storage, when facts and `triangle(i)` are read, then they are identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `enum TriIndex { U32(Vec<[u32; 3]>), U64(Vec<[u64; 3]>) }`; `struct TopologyFacts { degenerate: u64, boundary_edges: u64, nonmanifold_edges: u64, misoriented_pairs: u64, components: u64, closed: bool, oriented: bool }`.
- **Algorithm:** build a list of directed half-edges `(min(a,b), max(a,b), dir, tri)`, sort it with a stable total order, then scan runs: run length 1 is boundary, more than 2 is non-manifold, 2 with equal direction is misoriented. Components by union-find over triangles sharing an edge, visited in index order.
- **Errors:** `NonFinite`, `IndexOutOfRange`, `AttributeLength`.
- **Determinism:** D0; integer sorting and counting only.
- **Performance budget:** facts for a 10⁷-triangle mesh under 3 s single-threaded on reference hardware [unverified].
- **Crate:** `fid-geo-types` (kernel graph, KNP-1 applies).

## Test fixtures

- `fixtures/geo/types/mesh_cube`, `mesh_open_flipped`, `mesh_nonmanifold`: hand-written small meshes with expected facts in a sidecar JSON. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-types` (pure Rust, no I/O, kernel graph), add module `mesh` with `Mesh::new(frame: FrameRef, vertices: PointBuffer, vertex_attrs: AttributeSet, triangles: Vec<[u64;3]>) -> Result<Mesh, GeoTypeError>`. Store indexes as `u32` when vertex count ≤ 2³², else `u64`; expose them as `u64`. Validate finite vertices (`NonFinite`), index range (`IndexOutOfRange{triangle, index}`, first offender) and attribute lengths. Compute `TopologyFacts { degenerate, boundary_edges, nonmanifold_edges, misoriented_pairs, components, closed, oriented }` with a sorted half-edge table and union-find (no HashMap); never modify input. Acceptance: closed cube → all zero, closed and oriented; cube missing one triangle and with one flipped → boundary 3, misoriented 3; three triangles on one edge → non-manifold 1; bad index → `IndexOutOfRange{triangle:5,index:8}`; u32 and u64 storage give identical results. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, deny `unwrap`/`expect`), KNP-1 (no std HashMap/HashSet, no FMA, stable sort only). Files: `crates/fid-geo-types/src/mesh.rs`, `tests/mesh.rs`, fixtures under `fixtures/geo/types/`.

## Open questions

- None.

## Children

_Leaf: none._
