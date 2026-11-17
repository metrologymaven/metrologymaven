---
id: ALN.RPS.VECTOR.SURF
title: Measured-surface deviation provider for vector points
level: 4
parent: ALN.RPS.VECTOR
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS.VECTOR-001, REQ-ALN.RPS.VECTOR-002]
covers: []
depends_on: [ALN.RPS.VECTOR.LOOP, GEO.DIST, GEO.TYPES.MESH, GEO.FRAME.XFORM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.RPS.VECTOR.SURF: Measured-surface deviation provider for vector points

## Purpose

The `DeviationProvider` for scan data: under the current pose, each vector point's nominal line (nominal position plus s times nominal vector) is carried into the measured frame and intersected with the measured mesh; the intersection is the point's measured counterpart for this iteration.

## Scope

**In:** line-mesh intersection along the nominal vector through GEO.DIST.BATCH's directional projection; the search length; exclusion of points with no intersection; the pose-dependent re-evaluation each iteration.
**Out:** the loop and solve (`ALN.RPS.VECTOR.LOOP`); ray and projection arithmetic (`GEO.DIST`); meshing of raw clouds (`SCAN`).

## Requirements

- **REQ-ALN.RPS.VECTOR.SURF-001** For each point i and pose T, the provider shall intersect the line through T^-1 n_i with direction T^-1 d_i with the measured mesh, taking the intersection nearest along the line (either sign) within the search length (default 5 mm [unverified]), and return it as the counterpart.
- **REQ-ALN.RPS.VECTOR.SURF-002** If no intersection lies within the search length, then the provider shall return `Excluded(no_intersection)` for that point.
- **REQ-ALN.RPS.VECTOR.SURF-003** If the measured source is a point cloud rather than a mesh, then the provider shall fail at construction with `surface_source_requires_mesh`.
- **REQ-ALN.RPS.VECTOR.SURF-004** When two intersections are equally near, the provider shall use the one GEO.DIST reports (lowest triangle index).
- **REQ-ALN.RPS.VECTOR.SURF-005** The provider shall be D0 and byte-identical for any thread count.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/rps/vector/panel_mesh.json` (analytic panel mesh, 10 vector points, known transform), when `solve_vector_points` runs with this provider, then locked deviations converge to at most 1e-9 m against the analytic surface tessellated finely enough [unverified: tessellation error budget].
- **AC-2** Given a point whose line misses the mesh by 6 mm, then it is excluded with `no_intersection`.
- **AC-3** (error path) Given a point-cloud source, then construction fails with `surface_source_requires_mesh`.
- **AC-4** Given 1 and 8 threads, then the outputs are byte-identical.

## Design

- **Registry command(s):** none; `SurfaceProvider::new(mesh: &Mesh, points: &[VectorPoint], search_length: f64) -> Result<SurfaceProvider, SurfError>` implementing `DeviationProvider`.
- **Algorithm:** batch directional projection (GEO.DIST.BATCH, `direction` option, `max_distance` = search length) once per outer iteration for all points.
- **Errors:** `surface_source_requires_mesh`, `invalid_search_length`, `frame_mismatch`.
- **Determinism:** D0.
- **Performance budget:** 500 points against a 1,000,000-triangle mesh in under 20 ms per iteration [unverified until measured].
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`panel_mesh.json` (analytic panel tessellated in-repo, CC0).

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `SurfaceProvider` for the `DeviationProvider` trait (ALN.RPS.VECTOR.LOOP). For pose T, carry each nominal line (n_i, d_i) into the measured frame with T^-1 and intersect it with the measured mesh by GEO's batch directional projection (`fid-geo-dist`, `direction` and `max_distance` options), taking the nearest intersection within the search length (default 5 mm); none → `Excluded(no_intersection)`. Reject point-cloud sources with `surface_source_requires_mesh`. Byte-identical for any thread count. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1.

## Open questions

- REQ-ALN.RPS.VECTOR-002 says "first intersection"; this leaf reads it as nearest along the line (either sign), which is what GEO.DIST.BATCH offers. Phase P1b follows GEO.DIST.BATCH and scan-to-CAD (P1b).

## Children

_Leaf: none._
