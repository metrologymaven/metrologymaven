---
id: SCAN.CLOUD.FILTER
title: Statistical and radius outlier removal
level: 3
parent: SCAN.CLOUD
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CLOUD-001, REQ-SCAN.CLOUD-002, REQ-SCAN.CLOUD-008]
covers: [PW.PCL.01, PCD.LAS.02]
depends_on: [GEO.INDEX, GEO.OOC, GEO.TYPES]
side_effect: W2
determinism: D0
cli: [fid cloud remove-outliers]
mcp: [cloud_remove_outliers]
journal: [cloud.remove_outliers@1]
updated: 2026-11-16
---

# SCAN.CLOUD.FILTER: Statistical and radius outlier removal

## Purpose

Removes noise and isolated outlier points from a point cloud by a neighbourhood criterion, producing a new cloud version and a count of what was removed. It is the cleanup step in front of normal estimation, meshing and comparison, and it is the cloud-side half of the point-cloud operations incumbents bundle with CAD comparison.

## Scope

**In:**
- Statistical criterion: mean distance to k nearest neighbours against a global mean plus a standard-deviation multiplier
- Radius criterion: minimum neighbour count within a radius in metres
- Optional restriction to a GEO selection (points outside it are kept unchanged)
- In-memory and out-of-core (chunked) clouds with identical results
- Removal report: points in, points removed, points out, criterion and parameters

**Out** (and which node owns it instead):
- Subsampling (owned by `SCAN.CLOUD.SUBS`)
- Normal estimation (owned by `SCAN.CLOUD.NORM`)
- Spike and noise-component removal on meshes (owned by `SCAN.MESHOP`)
- Neighbour queries and chunk iteration (owned by `GEO.INDEX` and `GEO.OOC`)

## Requirements

- **REQ-SCAN.CLOUD.FILTER-001** When `cloud.remove_outliers@1` runs, the system shall create a new cloud element version that references the input version digest and shall leave the input element bit-identical.
- **REQ-SCAN.CLOUD.FILTER-002** When the statistical criterion is chosen with parameters k (integer, 1 to 256) and multiplier m (f64, greater than 0), the system shall remove exactly the points whose mean distance to their k nearest neighbours exceeds μ + m·σ, where μ and σ are the mean and population standard deviation of that per-point distance over the considered points.
- **REQ-SCAN.CLOUD.FILTER-003** When the radius criterion is chosen with radius r in metres (greater than 0) and minimum count c (integer, at least 1), the system shall remove exactly the points that have fewer than c other points within distance r.
- **REQ-SCAN.CLOUD.FILTER-004** The command shall report the number of input points, removed points and output points, and the removed count shall equal input minus output.
- **REQ-SCAN.CLOUD.FILTER-005** The output of the command shall be bit-identical (determinism class D0) for the same input digest and parameters, independent of thread count and of whether the cloud is in memory or chunked.
- **REQ-SCAN.CLOUD.FILTER-006** If a parameter is out of range or the cloud has fewer than k+1 points, then the command shall fail with a typed error and create no element version.
- **REQ-SCAN.CLOUD.FILTER-007** Both criteria shall complete on a 10⁸-point cloud within a published time and memory budget on reference hardware, initially 120 s and 8 GiB peak resident memory [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cloud_plane_outliers` (10⁵ points on a 1 m plane with 0.1 mm Gaussian noise plus 1 000 injected outliers 5–50 mm off the plane, seed 7), when `fid cloud remove-outliers --method statistical --k 16 --multiplier 2.0 --json` runs, then at least 99 % of the injected outliers are removed, at most 0.5 % of plane points are removed, and the report counts satisfy removed = input − output.
- **AC-2** Given the same fixture, when the radius criterion runs with r = 2 mm and c = 4, then the removed set equals the brute-force O(n²) reference on the same points exactly (same indices).
- **AC-3** Given the fixture converted to chunked form by GEO.OOC, when AC-1 is repeated, then the output element digest equals the in-memory output digest.
- **AC-4** (error path) Given `--k 0` or `--radius -1`, when the command runs, then it fails with error `scan.cloud.invalid_parameter`, creates no element and journals nothing.
- **AC-5** (input integrity) After AC-1, the input element digest is unchanged.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode on a different thread count, then the outcome is `IDENTICAL`.
- **AC-7** (performance) Given fixture `fixtures/scan/cloud_1e8_synthetic` (generator, seed 11), the benchmark records wall time and peak memory for both criteria; the result is published, and the budget in REQ-007 is checked [unverified until measured].

## Design

- **Registry command:** `cloud.remove_outliers@1`, class W2. Parameters: `cloud` (element handle), `method` (`statistical` | `radius`), `k` (u32), `multiplier` (f64), `radius` (f64, metres), `min_count` (u32), `selection` (optional GEO selection handle). Result: `{ output: ElementVersionRef, input_points: u64, removed: u64, output_points: u64, method, params }`.
- **Data structures:** `OutlierReport`; output cloud keeps all per-point attributes of kept points in original order.
- **Algorithm (statistical):** for each point in canonical index order, query k nearest neighbours (excluding self) via GEO.INDEX or GEO.OOC; d_i = mean of the k distances, summed in ascending neighbour-distance order. μ and σ computed with a fixed-order pairwise reduction over d_i in index order (KNP-1). Keep the point when d_i ≤ μ + m·σ. This is the widely used statistical outlier removal scheme (Rusu et al., 2008) [unverified citation detail].
- **Algorithm (radius):** count neighbours within r (strictly less than or equal to r, self excluded); keep when count ≥ c.
- **Selection:** when given, only selected points are tested; μ and σ are computed over selected points only.
- **Errors:** `scan.cloud.invalid_parameter`, `scan.cloud.too_few_points`, GEO frame and integrity errors passed through.
- **Determinism:** D0. Neighbour ties follow GEO.INDEX's deterministic tie-break; reductions are fixed order; no FMA (KNP-1).
- **Performance budget:** see REQ-007 [unverified].
- **Crate:** `fid-scan-cloud` (kernel tier: pure, no I/O, KNP-1; command handler registered through the CORE registry manifest).

## Test fixtures

- `fixtures/scan/cloud_plane_outliers`: synthetic generator, seed 7, CC-BY-4.0 (project-generated).
- `fixtures/scan/cloud_1e8_synthetic`: synthetic generator producing a 10⁸-point freeform patch, seed 11, generated at test time and not stored.
- Brute-force reference implementation in the test module (test-only, O(n²) on ≤ 10⁵ points).

## Build prompt

> Implement the registry command `cloud.remove_outliers@1` (side-effect class W2, determinism D0) in the Rust crate `fid-scan-cloud`. CLI: `fid cloud remove-outliers --cloud <handle> --method statistical|radius [--k N --multiplier M | --radius R_METRES --min-count C] [--selection <handle>] [--json]`; MCP tool `cloud_remove_outliers`; journal event `cloud.remove_outliers@1` plus result digest.
>
> Behaviour: statistical criterion removes points whose mean distance to their k nearest neighbours (self excluded) exceeds μ + m·σ (population σ over the tested points); radius criterion removes points with fewer than `min_count` other points within `radius` metres. Output is a new cloud element version referencing the input digest; the input is never modified. Report input, removed and output counts. Optional selection limits which points are tested.
>
> Constraints: SI metres, f64. Kernel rules KNP-1: no FMA, no std HashMap/HashSet, fixed-order reductions, total-order min/max, NaN is an error. Results must be identical across thread counts and between in-memory and chunked clouds (use the GEO.INDEX and GEO.OOC query APIs; do not build your own index). No panic, unwrap, expect or unsafe. Typed errors: `scan.cloud.invalid_parameter`, `scan.cloud.too_few_points`.
>
> Tests: synthetic plane with injected outliers (seed 7, 10⁵ points): ≥ 99 % outliers removed, ≤ 0.5 % inliers removed at k=16, m=2; radius criterion equals a brute-force reference exactly; chunked equals in-memory digest; invalid parameters fail and journal nothing; exact-mode replay is IDENTICAL; a 10⁸-point benchmark records time and memory (budget 120 s, 8 GiB, unverified).

## Open questions

- Whether a per-scan (rather than global) μ and σ is wanted for merged multi-scan clouds; for now the statistics are global over the tested points.

## Children

_Leaf: none._
