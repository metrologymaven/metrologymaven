---
id: SCAN.MESHOP.DECIM
title: Thin and refine meshes
level: 3
parent: SCAN.MESHOP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP-001, REQ-SCAN.MESHOP-002, REQ-SCAN.MESHOP-010]
covers: [GOM.MSH.02, GOM.MSH.04]
depends_on: [SCAN.MESHOP.PROV, GEO.INDEX, GEO.DIST]
side_effect: W2
determinism: D0
cli: [fid mesh thin, fid mesh refine]
mcp: [mesh_thin, mesh_refine]
journal: [mesh.thin@1, mesh.refine@1]
updated: 2026-11-16
---

# SCAN.MESHOP.DECIM: Thin and refine meshes

## Purpose

Changes mesh density without changing its shape beyond a stated bound: thinning (decimation) to a target triangle count or a maximum chordal deviation, and refining (subdivision) to a target edge length. Thinning keeps large scans manageable; refining gives enough vertices for later operations and for comparison stations.

## Scope

**In:**
- Thin by target triangle count, by maximum chordal deviation in metres, or both (whichever stops first)
- Refine by target maximum edge length in metres
- Optional restriction to a GEO selection; boundary edges preserved
- Report: triangle counts before and after, largest vertex-to-input-surface distance

**Out** (and which node owns it instead):
- Smoothing (owned by `SCAN.MESHOP.SMOOTH`)
- Repair of defects found during thinning (owned by `SCAN.MESHOP.REPAIR`)
- Point cloud subsampling (owned by `SCAN.CLOUD.SUBS`)

## Requirements

- **REQ-SCAN.MESHOP.DECIM-001** When `mesh.thin@1` or `mesh.refine@1` runs, the system shall create a new mesh version through the `MeshOp` contract and shall leave the input mesh bit-identical.
- **REQ-SCAN.MESHOP.DECIM-002** When thinning runs with a maximum chordal deviation d in metres, no output vertex shall lie further than d from the input mesh surface, and the result shall report the largest such distance.
- **REQ-SCAN.MESHOP.DECIM-003** When thinning runs with a target triangle count N, the output shall have at most N triangles unless the deviation bound, boundary preservation or manifoldness prevents further collapse, in which case the result shall report the reason and the achieved count.
- **REQ-SCAN.MESHOP.DECIM-004** Thinning shall not create non-manifold edges or flip any triangle normal by more than 90° relative to its pre-collapse orientation.
- **REQ-SCAN.MESHOP.DECIM-005** When refining runs with a target edge length L in metres, no output edge shall be longer than L, and every new vertex shall lie on the input surface (planar subdivision of input triangles).
- **REQ-SCAN.MESHOP.DECIM-006** Thinning and refining shall be determinism class D0.
- **REQ-SCAN.MESHOP.DECIM-007** Thinning a 10⁸-triangle mesh to 10⁷ triangles shall complete within a published time and memory budget on reference hardware, initially 300 s and 24 GiB [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh_sphere_fine` (icosphere of radius 50 mm with 1.3·10⁶ triangles), when `fid mesh thin --max-deviation 0.00001 --json` runs, then the reported largest vertex distance is ≤ 10 µm, verified independently by GEO.DIST on every output vertex, and the triangle count drops by at least 80 %.
- **AC-2** Given the same fixture, when `fid mesh thin --target 10000` runs, then the output has ≤ 10 000 triangles, zero non-manifold edges and no flipped triangles.
- **AC-3** Given fixture `fixtures/scan/mesh_open_cube`, when thinning runs, then the boundary loop is preserved vertex for vertex.
- **AC-4** Given `fixtures/scan/mesh_closed_cube`, when `fid mesh refine --max-edge 0.01` runs on a 100 mm cube, then every edge is ≤ 10 mm and every vertex lies on a cube face within 1e-12 m.
- **AC-5** (error path) Given `--max-deviation -1` or `--target 0`, the command fails with `scan.mesh.invalid_parameter` and journals nothing.
- **AC-6** (replay) Given the journal from AC-1, exact-mode replay on a different thread count yields `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `mesh.thin@1` (`mesh`, `target_triangles?` u64, `max_deviation?` m, `selection?`; at least one of the first two); `mesh.refine@1` (`mesh`, `max_edge` m, `selection?`).
- **Thin algorithm:** quadric-error-metric edge collapse (Garland and Heckbert, 1997) with the collapse position restricted to the optimal quadric point or an endpoint; candidates in a binary heap keyed by (cost, lower vertex index, higher vertex index); each candidate checked against the input surface with GEO.DIST and rejected if any affected vertex would exceed d, if it creates a non-manifold edge, or if it flips a triangle. Boundary vertices are never moved. Sequential to preserve D0.
- **Refine algorithm:** repeated longest-edge bisection in ascending edge-key order until no edge exceeds L; new vertices at edge midpoints, so they stay on the input triangles; `derive_triangle` keeps the synthesized flag.
- **Errors:** `scan.mesh.invalid_parameter`, `scan.mesh.stale_selection`.
- **Determinism:** D0 (sequential collapse order with total-order keys; KNP-1).
- **Performance budget:** REQ-007 [unverified].
- **Crate:** `fid-scan-mesh`.

## Test fixtures

- `fixtures/scan/mesh_sphere_fine`: generated icosphere (subdivision level 8), CC-BY-4.0.
- `fixtures/scan/mesh_open_cube`, `mesh_closed_cube` (shared).
- `fixtures/scan/mesh_1e8_synthetic`: generated freeform patch with 10⁸ triangles, seed 17, for the benchmark.

## Build prompt

> In the Rust crate `fid-scan-mesh`, implement two W2, D0 registry commands through the existing `MeshOp` trait (module `op`): `mesh.thin@1` (target triangle count and/or maximum chordal deviation in metres, optional selection) and `mesh.refine@1` (maximum edge length in metres, optional selection). CLI `fid mesh thin|refine ... [--json]`; MCP `mesh_thin`, `mesh_refine`.
>
> Thin by sequential quadric-error edge collapse with a heap keyed (cost, low vertex, high vertex); reject a collapse that would move any affected vertex further than the deviation bound from the input surface (check with the GEO.DIST closest-point API against the untouched input), create a non-manifold edge, flip a triangle, or move a boundary vertex. Report the achieved count, the reason collapse stopped, and the largest vertex-to-input distance. Refine by longest-edge midpoint bisection in edge-key order until every edge ≤ L. Use `derive_triangle`/`copy_triangle` so the synthesized flag propagates.
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet, fixed order), no panic/unwrap/unsafe, input never modified, typed errors `scan.mesh.invalid_parameter`, `scan.mesh.stale_selection`. Tests: icosphere thinning with 10 µm bound verified by GEO.DIST and ≥ 80 % reduction; target-count thinning manifold and unflipped; boundary preserved on the open cube; cube refinement edge bound and on-surface vertices; error paths; exact replay IDENTICAL; 10⁸-triangle benchmark (300 s, 24 GiB, unverified).

## Open questions

- Curvature-following refinement (new vertices placed on an interpolating surface rather than the planar triangles) is a likely user request; not in v1 because it moves data off the measured triangles.

## Children

_Leaf: none._
