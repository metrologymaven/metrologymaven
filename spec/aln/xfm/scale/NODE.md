---
id: ALN.XFM.SCALE
title: Similarity point-pair transform with uniform scale
level: 3
parent: ALN.XFM
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.XFM-002, REQ-ALN.XFM-003, REQ-ALN.XFM-007]
covers: [SA.XFM.05]
depends_on: [ALN.XFM.PAIRS, KRN.NUM]
side_effect: W2
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ALN.XFM.SCALE: Similarity point-pair transform with uniform scale

## Purpose

Adds the optional uniform scale factor to the point-pair solver (similarity transform), off by default. It extends `align.run_point_pairs@1` with a `scale: true` parameter and the R1 entry `point_pairs_similarity`, and reports the scale as a dimensionless value. Used for SpatialAnalyzer-style scaled transforms and for checking thermal or unit errors.

## Scope

**In:**
- The `scale` parameter of `align.run_point_pairs@1` and its validation
- Weighted similarity solution and scale reporting
- Recording the alignment as a similarity transform

**Out** (and which node owns it instead):
- The rigid solver, command and residual report (`ALN.XFM.PAIRS`)
- Non-uniform scale or affine transforms (not offered)
- Thermal compensation (`UNC`, `HAL`)

## Requirements

- **REQ-ALN.XFM.SCALE-001** When `scale` is true, the solver shall return the similarity transform (rotation, translation and one uniform scale factor s > 0) minimising the weighted sum of squared pair distances.
- **REQ-ALN.XFM.SCALE-002** The alignment shall record the scale as a dimensionless value, together with the scale expressed in parts per million relative to 1.
- **REQ-ALN.XFM.SCALE-003** Given noise-free synthetic pairs with a known similarity transform (scale between 0.5 and 2), the recovered transform shall reproduce every target point within 1e-12 m for parts within 10 m of the origin [unverified], and the scale within 1e-14 relative.
- **REQ-ALN.XFM.SCALE-004** If `scale` is true and DoF locks are also set, then the command shall fail with `scale_with_locks_unsupported` and journal nothing.
- **REQ-ALN.XFM.SCALE-005** If `scale` is absent or false, the solver shall return a rigid transform identical to `ALN.XFM.PAIRS`'s result.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/xfm/pairs-similarity` (pair sets under known similarity transforms with s in {0.5, 0.999988, 1, 1.000012, 2}), when `fid align run-point-pairs --scale --json` runs, then every target is reproduced within 1e-12 m and the reported scale matches within 1e-14 relative.
- **AC-2** Given s = 1.000012, then the alignment records scale 1.000012 and 12 ppm.
- **AC-3** (error path) Given `--scale` with locks {rz}, then the command fails with `scale_with_locks_unsupported` and journals nothing.
- **AC-4** Given the same pairs without `--scale`, then the result is bit-identical to the rigid solver's.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Command:** extends `align.run_point_pairs@1` (W2) with `scale: bool` (default false); no new command ID, so no new CLI, MCP or journal names. R1 entry `point_pairs_similarity(measured, reference, weights) -> Result<SimilaritySolution, XfmError>`.
- **Algorithm:** as the rigid solver, then s = trace(S D) / sigma_p^2 with D = diag(1, 1, det(V U^T)) and sigma_p^2 the weighted variance of the centred measured points; t = q_bar - s R p_bar (Umeyama 1991).
- **Errors:** `scale_with_locks_unsupported`, plus the rigid solver's errors.
- **Determinism:** D0.
- **Performance budget:** same as the rigid solver.
- **Crate:** `fid-align-solve` (solver) and `fid-align` (parameter); provisional names.

## Test fixtures

`fixtures/aln/xfm/pairs-similarity`: seeded generator (seed 11), pair sets of 4 to 1,000 points under known similarity transforms. Project-generated, CC-BY-4.0.

## Build prompt

> Extend the point-pair solver with uniform scale. In kernel crate `fid-align-solve` add `point_pairs_similarity(measured, reference, weights) -> Result<SimilaritySolution, XfmError>`; in `fid-align` add the `scale: bool` parameter (default false) to the existing command `align.run_point_pairs@1` (W2; CLI flag `--scale`).
> Requirements: with scale, return rotation, translation and one scale s > 0 minimising the weighted squared pair distances (Umeyama: s = trace(S D) / weighted variance of the centred measured points, D the reflection correction; t = q_bar - s R p_bar). Record the scale as a dimensionless value and in ppm relative to 1. Scale together with DoF locks fails with `scale_with_locks_unsupported` (nothing journaled). Without scale the result is bit-identical to the rigid solver.
> Acceptance: fixture `fixtures/aln/xfm/pairs-similarity` (seed 11); tests: targets within 1e-12 m and scale within 1e-14 relative for s from 0.5 to 2; 12 ppm case recorded; locks plus scale rejected; no-scale bit-identity; exact replay IDENTICAL.
> Constraints: KNP-1 (no FMA, fid-num SVD, fixed-order reductions, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- None blocking.

## Children

_Leaf: none._
