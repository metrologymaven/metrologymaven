---
id: SCAN.MESHOP.SMOOTH
title: Smoothing, spike and noise removal
level: 3
parent: SCAN.MESHOP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP-001, REQ-SCAN.MESHOP-003, REQ-SCAN.MESHOP-005, REQ-SCAN.MESHOP-010]
covers: [GOM.MSH.03, GOM.MSH.06]
depends_on: [SCAN.MESHOP.PROV]
side_effect: W2
determinism: D0
cli: [fid mesh smooth, fid mesh remove-spikes, fid mesh remove-noise]
mcp: [mesh_smooth, mesh_remove_spikes, mesh_remove_noise]
journal: [mesh.smooth@1, mesh.remove_spikes@1, mesh.remove_noise@1]
updated: 2026-11-16
---

# SCAN.MESHOP.SMOOTH: Smoothing, spike and noise removal

## Purpose

Reduces measurement noise on a mesh while bounding how far any vertex may move, removes single-vertex spikes, and deletes small disconnected noise components. Every move is bounded and reported so that a smoothed mesh can still be trusted for comparison.

## Scope

**In:**
- Smoothing with a maximum vertex displacement bound in metres, iteration count and strength
- Spike removal: vertices whose offset from their neighbour plane exceeds a threshold, relocated to that plane
- Noise removal: deletion of connected components below a triangle count or area
- Optional restriction to a GEO selection; boundary vertices fixed

**Out** (and which node owns it instead):
- Outlier removal on point clouds (owned by `SCAN.CLOUD.FILTER`)
- Thinning and refinement (owned by `SCAN.MESHOP.DECIM`)
- Hole filling after deletion (owned by `SCAN.MESHOP.HOLES`)

## Requirements

- **REQ-SCAN.MESHOP.SMOOTH-001** When a command in this node runs, the system shall create a new mesh version through the `MeshOp` contract and shall leave the input mesh bit-identical.
- **REQ-SCAN.MESHOP.SMOOTH-002** When `mesh.smooth@1` runs with maximum displacement b in metres, no vertex shall be further than b from its input position, and the result shall report the largest and RMS displacement.
- **REQ-SCAN.MESHOP.SMOOTH-003** Smoothing shall use a volume-preserving (shrink-compensated) scheme so that, on a noisy sphere fixture, the fitted radius changes by less than 10 % of the noise standard deviation [unverified].
- **REQ-SCAN.MESHOP.SMOOTH-004** When `mesh.remove_spikes@1` runs with threshold t in metres, the system shall relocate every non-boundary vertex whose distance to the least-squares plane of its one-ring neighbours exceeds t onto that plane, shall flag the triangles incident to relocated vertices as synthesized, and shall report the count of relocated vertices.
- **REQ-SCAN.MESHOP.SMOOTH-005** When `mesh.remove_noise@1` runs with a minimum triangle count or minimum area in square metres, the system shall delete every connected component below the threshold and shall report the number of components and triangles deleted.
- **REQ-SCAN.MESHOP.SMOOTH-006** All three commands shall be determinism class D0.
- **REQ-SCAN.MESHOP.SMOOTH-007** Smoothing a 10⁸-triangle mesh with 10 iterations shall complete within a published time and memory budget on reference hardware, initially 200 s and 16 GiB [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh_sphere_noisy` (radius 50 mm, 0.02 mm normal noise, seed 19), when `fid mesh smooth --max-displacement 0.00005 --iterations 10 --json` runs, then every vertex moved ≤ 50 µm (checked independently), the residual RMS to the true sphere drops by at least 40 %, and the fitted radius changes by less than 2 µm.
- **AC-2** Given fixture `fixtures/scan/mesh_plane_spikes` (planar grid with 25 vertices raised 1 mm, seed 23), when `fid mesh remove-spikes --threshold 0.0002` runs, then exactly 25 vertices are relocated, all lie within 1e-9 m of the plane, and their incident triangles are flagged synthesized.
- **AC-3** Given fixture `fixtures/scan/mesh_with_debris` (one main body and 40 components under 50 triangles), when `fid mesh remove-noise --min-triangles 50` runs, then 40 components are deleted and the main body is unchanged.
- **AC-4** Given `fixtures/scan/mesh_open_cube`, smoothing leaves every boundary vertex in place.
- **AC-5** (error path) Given `--max-displacement 0`, the command fails with `scan.mesh.invalid_parameter` and journals nothing.
- **AC-6** (replay) Exact-mode replay of AC-1 on a different thread count yields `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `mesh.smooth@1` (`mesh`, `max_displacement` m, `iterations` u32 1–100, `lambda` f64 default 0.5, `mu` f64 default −0.53, `selection?`); `mesh.remove_spikes@1` (`mesh`, `threshold` m, `selection?`); `mesh.remove_noise@1` (`mesh`, `min_triangles?` u64, `min_area?` m², `selection?`).
- **Smoothing:** Taubin λ/μ smoothing (Taubin, 1995) with uniform one-ring weights, Jacobi-style updates (all vertices from the previous iterate) so the result does not depend on processing order; after each step each vertex displacement from its input position is clamped to b.
- **Spikes:** one pass in vertex index order using one-ring plane fits on input positions; relocation by orthogonal projection.
- **Noise:** components by union-find over edge-adjacent triangles; deletion via the builder (remaining triangles copied).
- **Errors:** `scan.mesh.invalid_parameter`, `scan.mesh.stale_selection`.
- **Determinism:** D0 (Jacobi updates, fixed-order sums, KNP-1).
- **Performance budget:** REQ-007 [unverified].
- **Crate:** `fid-scan-mesh`.

## Test fixtures

- `fixtures/scan/mesh_sphere_noisy` (seed 19), `mesh_plane_spikes` (seed 23), `mesh_with_debris` (seed 29): synthetic generators, CC-BY-4.0.
- `fixtures/scan/mesh_1e8_synthetic` (shared) for the benchmark.

## Build prompt

> In the Rust crate `fid-scan-mesh`, implement three W2, D0 registry commands through the `MeshOp` trait: `mesh.smooth@1` (Taubin λ/μ with Jacobi updates, uniform one-ring weights, boundary vertices fixed, per-vertex displacement from the input position clamped to `max_displacement` metres after each iteration; report max and RMS displacement), `mesh.remove_spikes@1` (relocate each non-boundary vertex further than `threshold` metres from its one-ring least-squares plane onto that plane; flag incident triangles synthesized; report count) and `mesh.remove_noise@1` (delete edge-connected components below `min_triangles` or `min_area`; report counts). Optional selection on all. CLI `fid mesh smooth|remove-spikes|remove-noise ... [--json]`; MCP `mesh_smooth`, `mesh_remove_spikes`, `mesh_remove_noise`.
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet, fixed-order reductions, `fid-num` facade), no panic/unwrap/unsafe, inputs never modified, typed errors `scan.mesh.invalid_parameter`, `scan.mesh.stale_selection`. Tests: noisy sphere (bound respected, RMS −40 %, radius drift < 2 µm), 25 spikes relocated and flagged, 40 debris components removed, boundary fixed, error path, exact replay IDENTICAL, 10⁸-triangle benchmark (200 s, 16 GiB, unverified).

## Open questions

- Curvature-aware (feature-preserving) smoothing weights are a likely second method; v1 uses uniform weights with the displacement clamp as the safety bound.

## Children

_Leaf: none._
