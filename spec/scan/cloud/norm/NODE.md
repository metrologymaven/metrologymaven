---
id: SCAN.CLOUD.NORM
title: Normal estimation and orientation
level: 3
parent: SCAN.CLOUD
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CLOUD-001, REQ-SCAN.CLOUD-004, REQ-SCAN.CLOUD-005, REQ-SCAN.CLOUD-008]
covers: [PW.PCL.06]
depends_on: [GEO.INDEX, GEO.OOC, GEO.TYPES]
side_effect: W2
determinism: D0
cli: [fid cloud estimate-normals, fid cloud orient-normals]
mcp: [cloud_estimate_normals, cloud_orient_normals]
journal: [cloud.estimate_normals@1, cloud.orient_normals@1]
updated: 2026-11-16
---

# SCAN.CLOUD.NORM: Normal estimation and orientation

## Purpose

Gives every point of a cloud a unit normal estimated from its neighbourhood, with a validity mask, and orients the normals consistently, either toward the sensor origin of each scan or by propagation across the surface. Oriented normals are a precondition for meshing (SCAN.POLY), point-to-plane registration and normal-direction comparison.

## Scope

**In:**
- Per-point normal by principal-component analysis of a k-nearest or fixed-radius neighbourhood
- Valid-normal mask; degenerate neighbourhoods marked invalid
- Orientation toward a sensor origin (given explicitly, or per scan from the scan provenance attribute)
- Orientation by neighbourhood propagation over a minimum spanning tree, per connected component
- Report: valid count, invalid count, components, flipped count

**Out** (and which node owns it instead):
- Curvature-adaptive subsampling that reuses the covariance (owned by `SCAN.CLOUD.SUBS`)
- Mesh vertex normals and normal inversion on meshes (owned by `SCAN.MESHOP`)
- Neighbour queries (owned by `GEO.INDEX`, `GEO.OOC`)

## Requirements

- **REQ-SCAN.CLOUD.NORM-001** When a normal command runs, the system shall create a new cloud element version that references the input version digest and shall leave the input element bit-identical.
- **REQ-SCAN.CLOUD.NORM-002** When `cloud.estimate_normals@1` runs with a neighbourhood of k points (3 to 256) or radius r in metres, the system shall set each point's normal to the unit eigenvector of the smallest eigenvalue of the neighbourhood covariance matrix.
- **REQ-SCAN.CLOUD.NORM-003** If a point has fewer than 3 neighbours, or the ratio of the middle to the largest covariance eigenvalue is below a stated rank threshold (default 1e-6), then the system shall mark that point's normal invalid and shall store no normal vector for it.
- **REQ-SCAN.CLOUD.NORM-004** When orientation toward a sensor origin runs, the system shall flip each valid normal whose dot product with the vector from the point to its scan's sensor origin is negative.
- **REQ-SCAN.CLOUD.NORM-005** When orientation by propagation runs, the system shall orient valid normals along a minimum spanning tree of the k-neighbour graph weighted by 1 − |nᵢ·nⱼ|, seeding each connected component at its point of largest coordinate along a stated axis with the normal pointing along that axis.
- **REQ-SCAN.CLOUD.NORM-006** The command shall report counts of valid normals, invalid normals, flipped normals and connected components.
- **REQ-SCAN.CLOUD.NORM-007** Normal estimation and orientation shall be determinism class D0, independent of thread count and of in-memory versus chunked storage.
- **REQ-SCAN.CLOUD.NORM-008** Normal estimation shall complete on a 10⁸-point cloud within a published time and memory budget on reference hardware, initially 180 s and 12 GiB peak resident memory [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/cloud_sphere_noisy` (10⁵ points on a 50 mm radius sphere, 0.02 mm noise, seed 9), when `fid cloud estimate-normals --k 20 --json` then `fid cloud orient-normals --sensor-origin 0,0,0 --outward` run, then every valid normal is within 2° of the true radial direction and all point outward.
- **AC-2** Given fixture `fixtures/scan/cloud_degenerate` (a line of 50 collinear points plus 3 isolated points), when estimate-normals runs, then all 53 points are marked invalid and no normal vector is stored for them.
- **AC-3** Given fixture `fixtures/scan/cloud_two_boxes` (two disjoint boxes, seed 13), when `fid cloud orient-normals --propagate --axis z` runs, then the report shows 2 components and all normals point outward.
- **AC-4** Given AC-1 on 1 and 16 threads and on the chunked form, then the output digests are equal.
- **AC-5** (error path) Given `--k 2`, the command fails with `scan.cloud.invalid_parameter`; given a cloud without normals, orient-normals fails with `scan.cloud.no_normals`; neither journals anything.
- **AC-6** (replay) Given the journal from AC-1, exact-mode replay yields `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `cloud.estimate_normals@1` (`cloud`, `k` u32 or `radius` m, `rank_threshold` f64 default 1e-6, `selection?`); `cloud.orient_normals@1` (`cloud`, `mode` `sensor_origin` | `propagate`, `origin` [x,y,z] m or `per_scan` from the provenance attribute, `axis` for propagation seeding, `k` u32 default 12).
- **Data:** normals stored as a per-point attribute plus a valid-normal bitmask (GEO.TYPES attributes).
- **Estimation:** covariance accumulated about the neighbourhood centroid in ascending neighbour-distance order; eigen-decomposition by the deterministic symmetric 3×3 solver in `fid-num` (fixed Jacobi sweep count). Sign is canonicalised (largest-magnitude component positive) before orientation so estimation alone is D0.
- **Propagation:** Prim's algorithm over the symmetric k-neighbour graph with priority key (weight, lower index, higher index) in a binary heap; this is the classic Hoppe et al. (1992) scheme [unverified citation detail]. Invalid points are skipped and do not bridge components.
- **Errors:** `scan.cloud.invalid_parameter`, `scan.cloud.no_normals`, `scan.cloud.missing_sensor_origin`.
- **Determinism:** D0 by fixed traversal order and total-order tie-breaks.
- **Performance budget:** REQ-008 [unverified]; propagation on 10⁸ points is measured and published separately.
- **Crate:** `fid-scan-cloud` (kernel tier, KNP-1).

## Test fixtures

- `fixtures/scan/cloud_sphere_noisy` (seed 9), `fixtures/scan/cloud_degenerate`, `fixtures/scan/cloud_two_boxes` (seed 13): synthetic generators, CC-BY-4.0.
- `fixtures/scan/cloud_1e8_synthetic` for the benchmark.

## Build prompt

> In the Rust crate `fid-scan-cloud`, implement two W2, D0 registry commands. `cloud.estimate_normals@1`: per point, PCA over k nearest (3–256) or radius-r neighbours; normal = unit eigenvector of the smallest covariance eigenvalue via the `fid-num` symmetric 3×3 solver; mark invalid (no vector stored, bit in a valid mask) when fewer than 3 neighbours or λ_mid/λ_max < rank_threshold (default 1e-6); canonicalise sign. `cloud.orient_normals@1`: mode `sensor_origin` (flip if n·(origin − p) < 0; origin explicit or per scan from the provenance attribute) or `propagate` (Prim MST over the k-neighbour graph with weight 1 − |nᵢ·nⱼ|, heap key (weight, low index, high index), each component seeded at its maximum point along a given axis, normal pointing along that axis). Report valid, invalid, flipped and component counts.
>
> CLI `fid cloud estimate-normals|orient-normals ... [--json]`; MCP `cloud_estimate_normals`, `cloud_orient_normals`. Output is a new cloud version referencing the input digest; input never modified.
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet, fixed-order reductions, `fid-num` facade), identical results across thread counts and chunked storage (use GEO.INDEX/GEO.OOC queries), no panic/unwrap/unsafe, typed errors `scan.cloud.invalid_parameter`, `scan.cloud.no_normals`, `scan.cloud.missing_sensor_origin`.
>
> Tests: noisy sphere (seed 9) normals within 2° and outward; degenerate fixture all invalid; two boxes give 2 components, outward; digest equality across threads and chunking; error paths journal nothing; exact replay IDENTICAL; 10⁸-point benchmark (180 s, 12 GiB, unverified).

## Open questions

- Whether a weighted PCA (distance-weighted covariance) should be offered as a second method; not in v1.

## Children

_Leaf: none._
