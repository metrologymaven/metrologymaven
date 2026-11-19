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
built_in: [fid-scan-mesh]
satisfies: [REQ-SCAN.POLY-005]
covers: []
depends_on: [GEO.TYPES]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **Metric definitions (pinned at build, PR for `fid-scan-mesh`):**
  - *Index-degenerate:* a triangle with a repeated vertex index counts in `triangles` and `degenerate` and takes no other part (no edges, component, fan or angle), as in GEO.TYPES.MESH.
  - *Area and degenerate:* area = 0.5 · sqrt((cx·cx + cy·cy) + cz·cz) with (cx, cy, cz) = (b − a) × (c − a), evaluated in that order without FMA; degenerate when area < threshold (strict). The threshold must be finite and > 0, so a zero-area triangle is always degenerate. A degenerate-by-area triangle leaves only the angle statistics; it keeps its edges, component and fans.
  - *Edges:* undirected vertex pairs of the non-index-degenerate triangles; each triangle having the edge is one use (duplicate triangles count separately).
  - *Vertices:* the mesh's vertex count, unreferenced vertices included. A vertex with no triangle has no fan and is never non-manifold; the endpoints of a fin edge form one fan each.
  - *Boundary loops:* boundary edges are walked undirected (so a Möbius strip closes). A walk starts at the smallest vertex with an unused boundary edge, always takes the unused boundary edge whose other end is smallest, and ends back at its start vertex (closed) or at a vertex with no unused boundary edge (open chain, only at odd boundary degree). Each walk is one loop. At a non-manifold vertex this rule decides the split (a bow tie of two fans gives two loops).
  - *Angles:* at each corner in stored order, u and v point to the next and previous corner; cos = dot(u, v) / (sqrt(dot(u, u)) · sqrt(dot(v, v))), dot = (x·x′ + y·y′) + z·z′, clamped to [−1, 1] by comparison; angle = `fid-num` `acos` (not atan2); the triangle's value is the smallest of the three. Statistics: count, min, nearest-rank p1, p5 and median (rank ceil(p·n/100), 1-based, ascending IEEE total order); all four null when no triangle qualifies. No histogram.
  - *Canonical JSON:* kernel convention: every number is a string (integers in decimal, angles in the ECMAScript number form of RFC 8785 §3.2.2.3), `null` for an absent statistic, keys sorted, no whitespace.
- **Errors:** `scan.mesh.invalid_index`, `scan.mesh.non_finite` (a non-finite coordinate, area or cosine, e.g. overflowing squares), `scan.mesh.invalid_threshold` (threshold not finite or not > 0) and `scan.mesh.allocation_failed` (a failed working allocation); the first failure in triangle order is returned.
- **Determinism:** D0 (sorted structures, fixed-order reductions, KNP-1).
- **Performance budget:** 10⁸ triangles within 60 s and 16 GiB [unverified].
- **Crate:** `fid-scan-mesh` (kernel tier).

## Test fixtures

- `fixtures/scan/mesh_closed_cube`, `mesh_open_cube`, `mesh_bowtie`, `mesh_fin`: hand-written tiny meshes, CC-BY-4.0. Built as `fixtures/scan/<name>.txt` with expected reports `<name>.quality.txt` from the independent reference `fixtures/scan/make_mesh_quality.py`, which also writes `mesh_tetra`, `mesh_open_grid`, `mesh_jitter_grid`, `mesh_mobius`, `mesh_slivers` (two thresholds), `mesh_zero_area`, `mesh_duplicate` and `mesh_empty`.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `quality`, implement `mesh_quality(mesh, degenerate_area) -> Result<MeshQualityReport, QualityError>`, a pure D0 kernel function with no registry command of its own. Report triangle, vertex, edge, boundary-edge (used by exactly one triangle), boundary-loop, non-manifold-edge (used by ≥ 3 triangles), non-manifold-vertex (incident triangles form more than one edge-connected fan) and component counts; count degenerate triangles (area < threshold, default 1e-18 m²) and exclude them from minimum-angle statistics (min, nearest-rank p1, p5, median, radians).
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet; use sorted vectors or BTreeMap; `fid-num` for `acos`; NaN is an error), no panic/unwrap/unsafe, canonical JSON (RFC 8785) serialisation of the report. Tests: closed cube, open cube, bowtie vertex, fin edge, zero-area triangle, and thread-count invariance, as in the acceptance criteria.

## Open questions

- None.

## Children

_Leaf: none._
