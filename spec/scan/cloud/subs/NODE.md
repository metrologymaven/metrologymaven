---
id: SCAN.CLOUD.SUBS
title: Point cloud subsampling
level: 3
parent: SCAN.CLOUD
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CLOUD-001, REQ-SCAN.CLOUD-003, REQ-SCAN.CLOUD-008]
covers: [PW.PCL.02]
depends_on: [GEO.INDEX, GEO.OOC, GEO.TYPES]
side_effect: W2
determinism: D2
cli: [fid cloud subsample-grid, fid cloud subsample-count, fid cloud subsample-random, fid cloud subsample-curvature]
mcp: [cloud_subsample_grid, cloud_subsample_count, cloud_subsample_random, cloud_subsample_curvature]
journal: [cloud.subsample_grid@1, cloud.subsample_count@1, cloud.subsample_random@1, cloud.subsample_curvature@1]
updated: 2026-11-16
---

# SCAN.CLOUD.SUBS: Point cloud subsampling

## Purpose

Reduces the number of points in a cloud while keeping its shape: by spatial grid spacing, to a target count, by seeded random selection, or with density that follows local curvature. Subsampling keeps very large scans tractable for meshing, registration and comparison.

## Scope

**In:**
- Uniform spatial grid subsampling with spacing in metres
- Subsampling to an exact target point count
- Seeded random subsampling to a target count or fraction
- Curvature-adaptive subsampling between a minimum and a maximum spacing
- Optional restriction to a GEO selection; unselected points are kept

**Out** (and which node owns it instead):
- Outlier removal (owned by `SCAN.CLOUD.FILTER`)
- Normals and their use for orientation (owned by `SCAN.CLOUD.NORM`)
- Display LOD summaries (owned by `GEO.OOC`; never a value of record)
- Mesh thinning (owned by `SCAN.MESHOP`)

## Requirements

- **REQ-SCAN.CLOUD.SUBS-001** When any subsampling command runs, the system shall create a new cloud element version that references the input version digest and shall leave the input element bit-identical.
- **REQ-SCAN.CLOUD.SUBS-002** When grid subsampling runs with spacing s in metres, the system shall keep exactly one point per occupied cubic cell of edge s in the cloud's frame, namely the point nearest the cell centre, with ties broken by lowest point index.
- **REQ-SCAN.CLOUD.SUBS-003** When count subsampling runs with target N, the system shall output exactly min(N, n) points, where n is the input point count.
- **REQ-SCAN.CLOUD.SUBS-004** When random subsampling runs with a seed and a target count or fraction, the system shall select points by the `fid-num` counter-based RNG so that the same seed and input produce the same output, and shall record the seed in the journal.
- **REQ-SCAN.CLOUD.SUBS-005** When curvature-adaptive subsampling runs with minimum spacing s_min and maximum spacing s_max in metres, no two kept points shall be closer than the local spacing interpolated between s_min (highest surface variation) and s_max (lowest surface variation).
- **REQ-SCAN.CLOUD.SUBS-006** Grid, count and curvature-adaptive subsampling shall be determinism class D0, and random subsampling shall be D2.
- **REQ-SCAN.CLOUD.SUBS-007** Kept points shall keep every per-point attribute of the input unchanged, in input index order.
- **REQ-SCAN.CLOUD.SUBS-008** Grid subsampling shall complete on a 10⁸-point cloud within a published time and memory budget on reference hardware, initially 60 s and 8 GiB peak resident memory [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cloud_cube_dense` (10⁶ points on the surface of a 100 mm cube, seed 3), when `fid cloud subsample-grid --spacing 0.002 --json` runs, then no two output points share a 2 mm cell, every occupied input cell has exactly one output point, and the output digest is stable across 1, 4 and 16 threads.
- **AC-2** Given the same fixture, when `fid cloud subsample-count --target 50000` runs, then exactly 50 000 points are output; with `--target 2000000` exactly 10⁶ points are output.
- **AC-3** Given the same fixture, when `fid cloud subsample-random --target 10000 --seed 42` runs twice, then both outputs have the same digest, and with `--seed 43` the digest differs.
- **AC-4** Given fixture `fixtures/scan/cloud_cylinder_fillet` (flat faces joined by 2 mm fillets, seed 5), when `fid cloud subsample-curvature --min-spacing 0.0005 --max-spacing 0.004` runs, then the minimum pairwise distance among kept points in each region is at least the local spacing, and point density on fillets is at least 4× that on flat faces.
- **AC-5** (error path) Given `--spacing 0` or `--min-spacing` greater than `--max-spacing`, the command fails with `scan.cloud.invalid_parameter` and journals nothing.
- **AC-6** (replay) Given the journals from AC-1 and AC-3, when replayed in exact mode, then the outcomes are `IDENTICAL`.

## Design

- **Registry commands** (all W2): `cloud.subsample_grid@1` (`cloud`, `spacing` m, `selection?`) D0; `cloud.subsample_count@1` (`cloud`, `target` u64, `selection?`) D0; `cloud.subsample_random@1` (`cloud`, `target` u64 or `fraction` f64 in (0,1], `seed` u64, `selection?`) D2; `cloud.subsample_curvature@1` (`cloud`, `min_spacing` m, `max_spacing` m, `k` u32 default 16, `selection?`) D0. Result: `{ output, input_points, output_points, params }`.
- **Grid:** cell index = floor((p − origin)/s) with origin the cloud's bounding-box minimum; cells visited in Morton order of their integer index; nearest-to-centre point per cell with lowest-index tie-break.
- **Count:** bisection on spacing (fixed 48 iterations) for the largest grid spacing whose output count is ≥ N, then keep the first N kept points in Morton cell order.
- **Random:** key_i = RNG(seed, i) from the `fid-num` counter-based generator; keep the N points with the smallest keys (ties by index); output in input index order.
- **Curvature:** surface variation λ₀/(λ₀+λ₁+λ₂) from the k-neighbour covariance (deterministic 3×3 symmetric eigen-solver from `fid-num`); local spacing linearly interpolated between s_max and s_min over the variation range [0, 1/3]; greedy Poisson-disk acceptance in descending variation order, ties by index.
- **Errors:** `scan.cloud.invalid_parameter`, `scan.cloud.empty_cloud`.
- **Determinism:** frontmatter states D2, the weakest class among the four commands; grid, count and curvature are D0 (REQ-006).
- **Performance budget:** REQ-008 [unverified].
- **Crate:** `fid-scan-cloud` (kernel tier, KNP-1).

## Test fixtures

- `fixtures/scan/cloud_cube_dense` and `fixtures/scan/cloud_cylinder_fillet`: synthetic generators with the stated seeds, CC-BY-4.0.
- `fixtures/scan/cloud_1e8_synthetic` (shared with SCAN.CLOUD.FILTER) for the benchmark.

## Build prompt

> In the Rust crate `fid-scan-cloud`, implement four W2 registry commands: `cloud.subsample_grid@1` (spacing in metres, D0), `cloud.subsample_count@1` (exact target count, D0), `cloud.subsample_random@1` (seed plus target or fraction, D2, seed journaled) and `cloud.subsample_curvature@1` (min and max spacing in metres, D0). CLI `fid cloud subsample-grid|subsample-count|subsample-random|subsample-curvature ... [--json]`; MCP tools `cloud_subsample_*`.
>
> Each creates a new cloud version referencing the input digest, keeps per-point attributes, never modifies the input, and may be restricted to a selection (unselected points pass through). Grid keeps the point nearest each occupied cell centre (ties: lowest index) with cells in Morton order. Count outputs exactly min(N, n) points (bisection on grid spacing, 48 fixed iterations, then first N in Morton order). Random keeps the N smallest counter-based RNG keys from `fid-num`. Curvature uses surface variation from k-neighbour PCA to interpolate a local spacing and accepts points greedily (descending variation, ties by index) so no two kept points are closer than their local spacing.
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet, fixed-order reductions, `fid-num` for transcendentals and RNG, NaN is an error); identical output across thread counts and in-memory versus chunked clouds via GEO.INDEX and GEO.OOC; no panic/unwrap/unsafe; typed errors `scan.cloud.invalid_parameter`, `scan.cloud.empty_cloud`.
>
> Tests: the six acceptance criteria above on synthetic cube (seed 3) and fillet (seed 5) fixtures, plus a 10⁸-point grid benchmark (budget 60 s, 8 GiB, unverified).

## Open questions

- Whether count subsampling should prefer the random method's uniformity over grid regularity; grid-based is chosen for D0.

## Children

_Leaf: none._
