---
id: SCAN.POLY.QUAL
title: Mesh quality report
level: 3
parent: SCAN.POLY
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.POLY-005]
covers: []
depends_on: [GEO.TYPES]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# SCAN.POLY.QUAL: Mesh quality report

## Purpose

A pure kernel function that measures a triangle mesh and returns a fixed-shape quality report: counts of triangles, boundary loops, non-manifold edges and vertices, degenerate triangles, and minimum-angle statistics. Every polygonization result attaches it, and SCAN.MESHOP reuses it for before/after reports, so the metric definitions exist once.

## Scope

**In:**
- `MeshQualityReport` type and the function that computes it
- Topological counts: triangles, vertices, edges, boundary edges, boundary loops, non-manifold edges, non-manifold vertices, connected components
- Geometric statistics: minimum triangle angle (minimum, 1st percentile, 5th percentile, median), degenerate triangle count (area below a stated threshold)

**Out** (and which node owns it instead):
- Self-intersection detection (owned by `SCAN.MESHOP.REPAIR`)
- Any change to the mesh (owned by `SCAN.MESHOP`)
- Registry commands that expose the report (each consuming command attaches it to its own result)

## Requirements

- **REQ-SCAN.POLY.QUAL-001** The quality function shall report triangle, vertex, edge, boundary-edge, boundary-loop, non-manifold-edge, non-manifold-vertex and connected-component counts for any indexed triangle mesh.
- **REQ-SCAN.POLY.QUAL-002** An edge shall count as boundary when exactly one triangle uses it and as non-manifold when three or more triangles use it; a vertex shall count as non-manifold when its incident triangles form more than one edge-connected fan.
- **REQ-SCAN.POLY.QUAL-003** The function shall report the minimum, 1st-percentile, 5th-percentile and median of the smallest interior angle per triangle, in radians, using nearest-rank percentiles.
- **REQ-SCAN.POLY.QUAL-004** The function shall count triangles whose area is below a stated threshold in square metres (default 1e-18) as degenerate and shall exclude them from the angle statistics.
- **REQ-SCAN.POLY.QUAL-005** The function shall be pure and determinism class D0, with the report's canonical JSON byte-identical for the same mesh digest and threshold.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh_closed_cube` (12 triangles, watertight), when the quality function runs, then it reports 12 triangles, 8 vertices, 18 edges, 0 boundary edges, 0 boundary loops, 0 non-manifold edges and vertices, 1 component, and minimum angle π/4.
- **AC-2** Given fixture `fixtures/scan/mesh_open_cube` (the cube with one face's two triangles removed), then it reports 4 boundary edges and 1 boundary loop.
- **AC-3** Given fixture `fixtures/scan/mesh_bowtie` (two triangle fans sharing one vertex) and `fixtures/scan/mesh_fin` (three triangles on one edge), then it reports 1 non-manifold vertex and 1 non-manifold edge respectively.
- **AC-4** Given a mesh with one zero-area triangle, then degenerate count is 1 and angle statistics ignore it.
- **AC-5** Given the same mesh on 1 and 16 threads, then the canonical JSON reports are byte-identical.

## Design

- **API:** `pub fn mesh_quality(mesh: &Mesh, degenerate_area: f64) -> Result<MeshQualityReport, QualityError>` in `fid-scan-mesh::quality`. No registry command of its own; the class R1 applies to the pure computation.
- **Data:** `MeshQualityReport { triangles, vertices, edges, boundary_edges, boundary_loops, nonmanifold_edges, nonmanifold_vertices, components, degenerate, min_angle: AngleStats }` serialised as RFC 8785 canonical JSON.
- **Algorithm:** build a sorted edge list (vertex-pair keys sorted, no hash maps) to count edge use; boundary loops by walking boundary edges in ascending start-vertex order; vertex fans by union-find over incident triangles sharing an edge; components by union-find over triangles; angles via `fid-num` `acos` on clamped cosines.
- **Errors:** `scan.mesh.invalid_index`, `scan.mesh.non_finite`.
- **Determinism:** D0 (sorted structures, fixed-order reductions, KNP-1).
- **Performance budget:** 10⁸ triangles within 60 s and 16 GiB [unverified].
- **Crate:** `fid-scan-mesh` (kernel tier).

## Test fixtures

- `fixtures/scan/mesh_closed_cube`, `mesh_open_cube`, `mesh_bowtie`, `mesh_fin`: hand-written tiny meshes, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `quality`, implement `mesh_quality(mesh, degenerate_area) -> Result<MeshQualityReport, QualityError>`, a pure D0 kernel function with no registry command of its own. Report triangle, vertex, edge, boundary-edge (used by exactly one triangle), boundary-loop, non-manifold-edge (used by ≥ 3 triangles), non-manifold-vertex (incident triangles form more than one edge-connected fan) and component counts; count degenerate triangles (area < threshold, default 1e-18 m²) and exclude them from minimum-angle statistics (min, nearest-rank p1, p5, median, radians).
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet; use sorted vectors or BTreeMap; `fid-num` for `acos`; NaN is an error), no panic/unwrap/unsafe, canonical JSON (RFC 8785) serialisation of the report. Tests: closed cube, open cube, bowtie vertex, fin edge, zero-area triangle, and thread-count invariance, as in the acceptance criteria.

## Open questions

- None.

## Children

_Leaf: none._
