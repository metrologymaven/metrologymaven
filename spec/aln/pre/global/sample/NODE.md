---
id: ALN.PRE.GLOBAL.SAMPLE
title: Prealignment down-sampling, normals and local shape descriptors
level: 4
parent: ALN.PRE.GLOBAL
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE.GLOBAL-004, REQ-ALN.PRE.GLOBAL-005, REQ-ALN.PRE.GLOBAL-006]
covers: []
depends_on: [GEO.INDEX.KDTREE, GEO.TYPES.POINTS, GEO.TYPES.MESH, GEO.CAD.TESS, KRN.NUM]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.PRE.GLOBAL.SAMPLE: Prealignment down-sampling, normals and local shape descriptors

## Purpose

Reduces measured and reference data to a few thousand representative points with normals and a rotation-invariant local shape descriptor each, so that hypothesis generation is fast and independent of input density. Reference meshes are sampled by area; CAD references are sampled from GEO's tessellation.

## Scope

**In:** voxel-grid down-sampling with a deterministic representative per voxel; area-weighted seeded sampling of reference surfaces; normals by local principal component analysis; histogram descriptors of local normal geometry in the spirit of fast point feature histograms (Rusu, Blodow and Beetz, ICRA 2009); the minimum-data check.
**Out:** hypothesis generation (`ALN.PRE.GLOBAL.HYPO`); scoring (`ALN.PRE.GLOBAL.SCORE`); spatial indexes (`GEO.INDEX`); CAD tessellation (`GEO.CAD.TESS`).

## Requirements

- **REQ-ALN.PRE.GLOBAL.SAMPLE-001** The voxel size shall default to 1 % of the reference bounding-box diagonal [unverified], be user-settable, and be recorded.
- **REQ-ALN.PRE.GLOBAL.SAMPLE-002** Each occupied voxel shall contribute exactly one point: the input point nearest the voxel's centroid, ties broken by lowest input index; voxels shall be visited in ascending (ix, iy, iz) order.
- **REQ-ALN.PRE.GLOBAL.SAMPLE-003** Reference surfaces shall be sampled by area-weighted sampling using the counter-based RNG with the run seed and a fixed stream ID, then down-sampled like the measured data.
- **REQ-ALN.PRE.GLOBAL.SAMPLE-004** Each sample shall get a unit normal from the smallest-eigenvalue eigenvector of its 16 nearest neighbours' covariance [unverified: k], oriented toward the sensor or reference outward normal when known and otherwise left unoriented and flagged.
- **REQ-ALN.PRE.GLOBAL.SAMPLE-005** Each sample shall get a descriptor of fixed length (33 bins [unverified]) computed from neighbours within 5 voxel sizes, invariant to rigid motion of the data within 1e-9 per bin.
- **REQ-ALN.PRE.GLOBAL.SAMPLE-006** If fewer than 100 measured samples remain [unverified], then the function shall fail with `insufficient_data` stating the count.
- **REQ-ALN.PRE.GLOBAL.SAMPLE-007** Given identical inputs and seed, outputs shall be bit-identical on every supported platform and thread count (class D2).
- **REQ-ALN.PRE.GLOBAL.SAMPLE-008** Sampling and description of 1,000,000 measured points and a 1,000,000-triangle reference shall take under 3 s on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/pre/global/sphere_cloud.json` and voxel 1 mm, when `sample` runs, then each occupied voxel yields one point, chosen per REQ-002, and the order is ascending voxel key.
- **AC-2** Given the same cloud rotated by a known rotation, then descriptors of corresponding samples agree within 1e-9 per bin.
- **AC-3** Given seed 9 on two platforms and 1 and 8 threads, then outputs are bit-identical.
- **AC-4** (error path) Given 50 points, then it fails with `insufficient_data{50}`.

## Design

- **Registry command(s):** none; R1 library function `sample(data: &SampleSource, params: &SampleParams, seed: u64) -> Result<SampledCloud, SampleError>`.
- **Data structures:** `SampleSource = Cloud(PointView) | Mesh(MeshView) | Cad(TessView)`; `SampledCloud { points, normals, oriented: BitVec, descriptors: Vec<[f64; 33]>, source_index: Vec<u64>, voxel_size }`.
- **Algorithm:** voxel keys as integer triples in a BTreeMap; k-nearest via GEO.INDEX.KDTREE; 3x3 symmetric eigen-decomposition via KRN.NUM; descriptor bins from angle triples between normal pairs, accumulated in neighbour-index order.
- **Errors:** `insufficient_data`, `invalid_voxel_size`, `nan_input`.
- **Determinism:** D2 (seeded sampling), KNP-1 arithmetic.
- **Performance budget:** REQ-008.
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`sphere_cloud.json`, a box mesh and a cylinder mesh, generated in-repo with seeds (CC0); `ALN.PRE.CORPUS` parts for timing.

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `sample(data, params, seed)`. Voxel down-sample (default voxel 1 % of the reference bbox diagonal; record it): one point per occupied voxel, the input point nearest the voxel centroid, ties by lowest index, voxels in ascending integer key order (BTreeMap). Sample reference meshes or CAD tessellations by area with the kernel's counter-based RNG (fixed stream), then voxel down-sample. Normals from the smallest eigenvector of the 16-NN covariance (k-d tree from `fid-geo-index`), oriented when a sensor or outward normal is known. Descriptor: 33-bin histogram of angle features between the sample's normal and neighbours' normals within 5 voxels (after Rusu et al. 2009; write your own code), rigid-motion invariant. Fail `insufficient_data` below 100 measured samples. Bit-identical across platforms and threads. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- Descriptor length, neighbourhood size and voxel default are starting values to be tuned against `ALN.PRE.CORPUS` [unverified].

## Children

_Leaf: none._
