---
id: ALN.PRE.GLOBAL.SCORE
title: Prealignment candidate scoring and short refinement
level: 4
parent: ALN.PRE.GLOBAL
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.PRE.GLOBAL-002, REQ-ALN.PRE.GLOBAL-003, REQ-ALN.PRE.GLOBAL-004, REQ-ALN.PRE.GLOBAL-005]
covers: []
depends_on: [ALN.PRE.GLOBAL.HYPO, ALN.BFIT.ICP.LOOP, GEO.DIST, KRN.NUM]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ALN.PRE.GLOBAL.SCORE: Prealignment candidate scoring and short refinement

## Purpose

Turns raw hypotheses into the top K scored candidates that `ALN.PRE.AMBIG` receives: each hypothesis is refined by a few point-to-plane best-fit iterations on the sampled points (through `ALN.BFIT.ICP.LOOP`'s R1 entry), scored by inlier fraction and trimmed distance against the full reference, and near-duplicates are suppressed.

## Scope

**In:** short refinement per hypothesis; inlier fraction and trimmed RMS against the reference surface; duplicate suppression; ranking and truncation to K.
**Out:** hypotheses (`ALN.PRE.GLOBAL.HYPO`); hint filtering, symmetry merging and ambiguity (`ALN.PRE.AMBIG`); final refinement after prealignment (`ALN.BFIT`).

## Requirements

- **REQ-ALN.PRE.GLOBAL.SCORE-001** Each hypothesis shall be refined by `bestfit` with metric `point_to_plane`, scope global over the measured samples, at most 10 iterations [unverified], and no locks.
- **REQ-ALN.PRE.GLOBAL.SCORE-002** After refinement, the inlier fraction shall be the fraction of measured samples within 2 voxel sizes of the reference surface (GEO.DIST), and the trimmed RMS the RMS of the lowest 80 % of distances [unverified].
- **REQ-ALN.PRE.GLOBAL.SCORE-003** The score shall be the inlier fraction, with ties broken by lower trimmed RMS, then by canonical pose bytes.
- **REQ-ALN.PRE.GLOBAL.SCORE-004** Two refined candidates within 1 voxel size in translation (at the reference bounding-box centre) and 1 degree in rotation shall be merged into the better-scored one.
- **REQ-ALN.PRE.GLOBAL.SCORE-005** The function shall return the top K candidates (K default 8, user-settable 1 to 64) with pose, score and inlier fraction, in score order.
- **REQ-ALN.PRE.GLOBAL.SCORE-006** Given identical inputs and seed, the candidate list shall be bit-identical on every supported platform (class D2).
- **REQ-ALN.PRE.GLOBAL.SCORE-007** Scoring 64 hypotheses with 5,000 samples against a 1,000,000-triangle reference shall take under 2 s on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given an `ALN.PRE.CORPUS` noise-free part with the true pose among the hypotheses, when `score` runs, then the top candidate is within 0.1 mm and 0.1 degree of the true pose and its inlier fraction is at least 0.95.
- **AC-2** Given two hypotheses 0.2 voxel apart, then one candidate remains.
- **AC-3** Given K = 3, then exactly three candidates are returned in score order.
- **AC-4** Given seed 1 on two platforms, then outputs are bit-identical.

## Design

- **Registry command(s):** none; R1 library function `score(hyps: &[Hypothesis], measured: &SampledCloud, reference: &dyn ProjectionTarget, params: &ScoreParams) -> Result<Vec<Candidate>, ScoreError>`.
- **Data structures:** `Candidate { pose, score, inlier_fraction, trimmed_rms, method }` (the `Candidate` type that ALN.PRE.AMBIG consumes, extended with `trimmed_rms` and `method`).
- **Algorithm:** refine, score, sort, suppress duplicates in score order, truncate.
- **Errors:** `invalid_k`, refinement errors are recorded per hypothesis (the hypothesis is dropped with reason) rather than failing the call.
- **Determinism:** D2 (inherits the seeded hypotheses); refinement is D0.
- **Performance budget:** REQ-007.
- **Crate:** `fid-align-solve` (kernel layer; provisional name).

## Test fixtures

`ALN.PRE.CORPUS` parts (CC0).

## Build prompt

> In crate `fid-align-solve` (pure Rust, f64, kernel layer, KNP-1), implement `score(hyps, measured, reference, params) -> Result<Vec<Candidate>, ScoreError>`. Refine each hypothesis with `bestfit` (ALN.BFIT.ICP.LOOP) using point-to-plane, global scope over the samples, max 10 iterations, no locks; drop hypotheses whose refinement errors, recording the reason. Score: inlier fraction (samples within 2 voxel sizes of the reference via GEO.DIST), tie-break lower trimmed RMS (lowest 80 %), then canonical pose bytes. Merge candidates within 1 voxel and 1 degree into the better one. Return top K (default 8, 1..=64, else `invalid_k`). Tests: AC-1..AC-4. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- ALN.PRE.AMBIG's `Candidate` lacks `trimmed_rms` and `method`; AMBIG ignores extra fields, so no change there is required.

## Children

_Leaf: none._
