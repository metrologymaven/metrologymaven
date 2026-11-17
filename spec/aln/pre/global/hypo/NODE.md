---
id: ALN.PRE.GLOBAL.HYPO
title: Pose hypothesis generation by descriptor consensus and principal axes
level: 4
parent: ALN.PRE.GLOBAL
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE.GLOBAL-001, REQ-ALN.PRE.GLOBAL-003, REQ-ALN.PRE.GLOBAL-004, REQ-ALN.PRE.GLOBAL-005]
covers: []
depends_on: [ALN.PRE.GLOBAL.SAMPLE, ALN.XFM.PAIRS, GEO.INDEX.KDTREE, KRN.NUM]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.PRE.GLOBAL.HYPO: Pose hypothesis generation by descriptor consensus and principal axes

## Purpose

Generates candidate poses over the whole pose space from sampled clouds, with no prior pose and no picks. The default method matches descriptors between measured and reference samples and finds poses by seeded random-sample consensus over three-point correspondences. A principal-axes method enumerates the proper rotations that align the two clouds' principal frames; it serves featureless or highly symmetric parts and is always added as a fallback set.

## Scope

**In:** descriptor matching (mutual nearest neighbours in descriptor space); seeded consensus sampling with geometric pre-checks; rigid pose from three correspondences via `ALN.XFM.PAIRS`; inlier counting against the reference samples; principal-axes enumeration (24 proper rotations); merging both sets into a ranked hypothesis list; method algorithm IDs.
**Out:** sampling and descriptors (`ALN.PRE.GLOBAL.SAMPLE`); scoring and refinement (`ALN.PRE.GLOBAL.SCORE`); hint filtering and ambiguity (`ALN.PRE.AMBIG`).

## Requirements

- **REQ-ALN.PRE.GLOBAL.HYPO-001** The `consensus` method shall pair each measured sample with its nearest reference descriptor and keep only mutual nearest pairs, with ties broken by lowest index.
- **REQ-ALN.PRE.GLOBAL.HYPO-002** Each consensus iteration i shall draw three pairs using the counter-based RNG at (seed, stream `prealign.hypo`, counter i), reject the draw if the triangle side lengths differ between measured and reference by more than 10 % [unverified], and otherwise compute the rigid pose through `ALN.XFM.PAIRS`.
- **REQ-ALN.PRE.GLOBAL.HYPO-003** Each pose shall be scored by its inlier count (measured samples within 2 voxel sizes of a reference sample), and the method shall keep the M best (default 64) with ties broken by the canonical byte order of the pose.
- **REQ-ALN.PRE.GLOBAL.HYPO-004** The iteration count shall be fixed (default 20,000 [unverified]) and the kept set shall not depend on how iterations are split across threads.
- **REQ-ALN.PRE.GLOBAL.HYPO-005** The `principal_axes` method shall add the 24 proper rotations mapping the measured principal frame onto the reference principal frame, with translations aligning centroids.
- **REQ-ALN.PRE.GLOBAL.HYPO-006** The output shall name the method and version as algorithm ID (`align.prealign.consensus@1.0.0` or `align.prealign.principal_axes@1.0.0`) on each hypothesis.
- **REQ-ALN.PRE.GLOBAL.HYPO-007** Given identical inputs and seed, the hypothesis list shall be bit-identical on every supported platform (class D2).
- **REQ-ALN.PRE.GLOBAL.HYPO-008** Hypothesis generation for 5,000 samples per side shall take under 4 s on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given an `ALN.PRE.CORPUS` noise-free part and its true pose, when `generate` runs with seed 1, then one of the top 8 hypotheses is within 2 voxel sizes and 2 degrees of the true pose.
- **AC-2** Given seed 1 on two platforms and 1 and 8 threads, then the hypothesis lists are bit-identical.
- **AC-3** Given a sphere-like featureless part, then the `principal_axes` set is present and holds 24 poses.
- **AC-4** Given fewer than three mutual pairs, then the consensus set is empty and only principal-axes hypotheses are returned, with warning `no_descriptor_matches`.

## Design

- **Registry command(s):** none; R1 library function `generate(measured: &SampledCloud, reference: &SampledCloud, params: &HypoParams, seed: u64) -> Result<Vec<Hypothesis>, HypoError>`.
- **Data structures:** `HypoParams { iterations, keep_m, inlier_factor, side_tolerance, methods: [Consensus, PrincipalAxes] }`; `Hypothesis { pose: Rigid, inliers: u32, method: AlgorithmId }`.
- **Algorithm:** descriptor nearest neighbours by brute force in fixed order (dimension 33); consensus loop in blocks of 256 iterations, each block's top-M merged in block order; Kabsch-style solve through ALN.XFM.PAIRS; principal axes from 3x3 covariance eigen-decomposition (KRN.NUM).
- **Errors:** `empty_input`, `nan_input`.
- **Determinism:** D2.
- **Performance budget:** REQ-008.
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`ALN.PRE.CORPUS` parts and seeds (CC0).

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `generate(measured, reference, params, seed) -> Result<Vec<Hypothesis>, HypoError>`. Consensus: mutual-nearest descriptor pairs (brute force, fixed order, ties by lowest index); for i in 0..iterations draw three pairs with the counter-based RNG at (seed, stream "prealign.hypo", i); reject if triangle side lengths differ by more than 10 %; solve the rigid pose with the point-pair solver (ALN.XFM.PAIRS); count inliers within 2 voxel sizes via the reference k-d tree; keep the best 64 with ties by canonical pose bytes; process in blocks of 256 merged in block order so threads never change results. Principal axes: add the 24 proper rotations aligning principal frames plus centroid translation. Tag each hypothesis with `align.prealign.consensus@1.0.0` or `align.prealign.principal_axes@1.0.0`. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- The method choice (descriptor consensus plus principal-axes fallback) is this node's proposal; a branch-and-bound global search may replace it if the corpus rate stays below 95 % [unverified]. Incumbents' methods are not public [unverified].

## Children

_Leaf: none._
