---
id: SCAN.CLOUD.REG.CTGT
title: Oriented-cloud projection target and overlap fraction
level: 4
parent: SCAN.CLOUD.REG
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-cloud]
built_pr: 677
satisfies: [REQ-SCAN.CLOUD.REG-003, REQ-SCAN.CLOUD.REG-008]
covers: []
depends_on: [GEO.DIST.BATCH, GEO.INDEX.KDTREE, GEO.TYPES.POINTS, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.CLOUD.REG.CTGT: Oriented-cloud projection target and overlap fraction

## Purpose

ALN's best-fit loop projects measured points onto a `ProjectionTarget` (mesh or CAD in GEO). Scan-to-scan registration needs a *cloud* as the reference, so this leaf adapts an oriented point cloud to that trait (point-to-plane against the nearest point with a valid normal) and provides the one definition of the overlap fraction that the pair command, the pair graph and the report all use.

## Scope

**In:**
- `OrientedCloudTarget`: a `ProjectionTarget` over a cloud with a valid-normal mask, built on a k-d tree of the valid points
- `overlap_fraction(moving, target, distance, sample)`: share of moving points within a stated distance of the target
- Deterministic stride sampling of a cloud to at most M points, shared by every REG child

**Out** (and which node owns it instead):
- The iteration loop and correspondence rules (owned by `ALN.BFIT.ICP.LOOP` and `ALN.BFIT.ICP.CORR`)
- The pair graph built from overlap values (owned by `SCAN.CLOUD.REG.GRAPH`)
- Normal estimation (owned by `SCAN.CLOUD.NORM`)

## Requirements

- **REQ-SCAN.CLOUD.REG.CTGT-001** For a query point p, `OrientedCloudTarget` shall return as correspondence the nearest target point q with a valid normal n (ties by lowest index), the foot point p − ((p − q)·n) n and the signed distance (p − q)·n, or `NoHit` when no such q lies within the stated maximum distance in metres.
- **REQ-SCAN.CLOUD.REG.CTGT-002** If the target cloud has no valid normals, then construction shall fail with `scan.cloud.no_normals`.
- **REQ-SCAN.CLOUD.REG.CTGT-003** `overlap_fraction` shall return, for the sampled moving points transformed by a given pose, the count whose Euclidean distance to the nearest target point is at most d metres, the sample size and their ratio.
- **REQ-SCAN.CLOUD.REG.CTGT-004** Stride sampling to at most M points shall select indexes ⌊i·N/M⌋ for i = 0..M−1 when N > M, and all points otherwise.
- **REQ-SCAN.CLOUD.REG.CTGT-005** Projection and overlap results shall be determinism class D0, bit-identical for any thread count.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/reg/plane_grid` (flat 100 × 100 mm grid, 1 mm pitch, normals +z), when a point 0.2 mm above a grid point is projected, then the foot lies on the plane within 1e-15 m and the signed distance is 2e-4 m ± 1e-15.
- **AC-2** Given the same target and max distance 1 mm, a point 5 mm away returns `NoHit`.
- **AC-3** Given fixture `fixtures/scan/reg/two_patches_overlap50` (two patches with exactly 50 % planar overlap), `overlap_fraction` with d = 0.5 mm returns 0.5 ± 0.001.
- **AC-4** (error path) Given a cloud whose normals are all invalid, construction fails with `scan.cloud.no_normals`.
- **AC-5** Given N = 10, M = 4, the sampled indexes are [0, 2, 5, 7].
- **AC-6** Given 1 and 16 threads, projection results and overlap counts are identical.

## Design

- **API** (`fid-scan-cloud::reg::target`): `OrientedCloudTarget::new(cloud: &PointCloud, valid: &Bitmask) -> Result<Self, RegError>`; `impl ProjectionTarget for OrientedCloudTarget` (frame, project, `project_along` = intersection of the line with the tangent plane of the nearest valid point, `NoHit` when parallel); `normal_at(entity_id) -> [f64; 3]` for correspondence normals; `fn overlap_fraction(moving: &PointCloud, pose: &Rigid, target: &OrientedCloudTarget, d: f64, max_samples: u64) -> Overlap { within, sampled, fraction }`; `fn stride_sample(n: u64, m: u64) -> impl Iterator<Item = u64>`.
- **Algorithm:** k-d tree (GEO.INDEX.KDTREE) over valid points only, mapped back to original indexes; nearest-neighbour query with the k-d tree's (dist², index) order; dot products without FMA.
- **Errors:** `scan.cloud.no_normals`, `scan.cloud.invalid_parameter` (d ≤ 0, M = 0), `geo.frame.mismatch`.
- **Determinism:** D0.
- **Performance budget:** build plus 10⁶ queries on a 10⁷-point target within 10 s [unverified].
- **Crate:** `fid-scan-cloud` (kernel tier, KNP-1).

## Test fixtures

Synthetic generators with fixed seeds: `plane_grid`, `two_patches_overlap50`; licence CC0.

## Build prompt

> In the Rust crate `fid-scan-cloud`, module `reg::target`, implement `OrientedCloudTarget` over a `PointCloud` with a valid-normal bitmask. It implements GEO's `ProjectionTarget` trait (`fid-geo-dist::batch`): `project(p, opts)` finds the nearest valid-normal point q (k-d tree from `fid-geo-index` built on valid points only; ties by lowest original index), returns foot p − ((p − q)·n) n, signed distance (p − q)·n and `entity_id` = q's original index, or `NoHit` beyond `opts.max_distance`; `project_along` intersects the line with q's tangent plane. Add `normal_at(entity_id)`. Construction with no valid normals fails `scan.cloud.no_normals`. Implement `stride_sample(n, m)` (indexes ⌊i·n/m⌋) and `overlap_fraction(moving, pose, target, d, max_samples)` returning within-count, sample count and ratio (distance to nearest target point, not to the plane). Pure, D0, thread-count independent.
>
> Constraints: CONSTITUTION §8 (no panic, no `unwrap`/`expect`, no `unsafe`), KNP-1 (no FMA, `fid-num` for sqrt, no std HashMap/HashSet, fixed-order reductions). Tests: AC-1..AC-6 with the synthetic fixtures.

## Open questions

- `ALN.BFIT.ICP.CORR` takes the normal at the foot from the target. As built, `normal_at` is an inherent method of `OrientedCloudTarget` (not a `ProjectionTarget` trait method) and returns `Option<[f64; 3]>`: `None` for an index that is not a valid point of the target. Whether GEO.DIST.BATCH should add a trait accessor is left to ALN.BFIT.ICP.CORR.
- Built choices (fid-scan-cloud): `overlap_fraction` requires the moving cloud and the target to be in the same frame (`geo.frame.mismatch` otherwise) and treats the pose as a rigid motion within that frame; a scan in another frame is first transformed with GEO.FRAME.APPLY. The overlap counts distance to the target's valid-normal points only. A threaded `overlap_fraction_with(..., threads)` serves AC-6. `stride_sample` returns a `Result` so that M = 0 is refused. A `d` below a rounding floor of 64 · 2^-53 times the largest coordinate magnitude involved is refused. Point-to-plane hits carry a new additive GEO.DIST sign variant `HitSign::PointNormal` (record sign code 253); directional hits use `HitSign::Along`. Any `subset` option is refused with `subset_mismatch`, since a cloud has no triangles.

## Children

_Leaf: none._
