---
id: KRN.OUT.RANSAC
title: Seeded RANSAC consensus policy
level: 3
parent: KRN.OUT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.OUT-001, REQ-KRN.OUT-005]
covers: [GOM.CON.21, SA.REL.04]
depends_on: [KRN.OUT.POL, KRN.NUM.RNG, KRN.LSQ.LIN, KRN.LSQ.RND]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.OUT.RANSAC: Seeded RANSAC consensus policy

## Purpose

Random sample consensus for heavily contaminated data: repeatedly fit a minimal sample, count points within a distance threshold, and keep the largest consensus set. Every random choice comes from the `fid-num` counter-based RNG under a journaled seed, so the same seed always gives the same excluded set on every host.

## Scope

**In:** `filter.outlier.ransac@1.0.0` implementing `PolicyMethod` for line, plane, circle and sphere targets; minimal-sample solvers for those primitives; fixed iteration count; deterministic consensus tie-break.
**Out:** the final least-squares refit on the consensus set (`KRN.OUT.RUN`); RANSAC for cylinder, cone and torus (later leaf; needs minimal solvers with normals).

## Requirements

- **REQ-KRN.OUT.RANSAC-001** The policy shall run exactly `iterations` rounds; round i shall draw its minimal sample from RNG stream `i` under the given seed, so rounds are independent of each other's outcome.
- **REQ-KRN.OUT.RANSAC-002** Each round shall solve the minimal sample exactly (2 points for a line, 3 for a plane or circle, 4 for a sphere) and count points whose orthogonal distance is at most `threshold_m`.
- **REQ-KRN.OUT.RANSAC-003** The policy shall keep the round with the largest consensus, breaking ties by the smaller sum of squared in-threshold distances and then by the lower round index, and shall exclude every point outside that consensus.
- **REQ-KRN.OUT.RANSAC-004** If no round reaches `min_inliers`, or every minimal sample is degenerate, then the policy shall return a typed `PolicyError::NoConsensus` and no mask.
- **REQ-KRN.OUT.RANSAC-005** For the same seed and input, the excluded set shall be identical on every target.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/out/ransac-contam-v1` (seeded planes, lines, circles and spheres with 40 % uniform outliers), when the policy runs with seed 7 and 500 iterations, then at least 99 % of planted inliers are kept and at least 99 % of planted outliers are excluded [unverified targets].
- **AC-2** Given the same inputs and seed on x86-64, ARM64 and wasm32 under wasmtime, then the excluded sets and their digests are identical.
- **AC-3** Given seed 8 instead of 7, then the run completes and the mask may differ; given seed 7 again, the mask is identical to AC-2.
- **AC-4** (error path) Given 100 collinear points and a plane target, then the policy returns `NoConsensus` (every sample degenerate).

## Design

- **Registry command(s):** none; selected through `fit.reject_outliers@1` (`KRN.OUT.RUN`).
- **Data structures:** implementation of `PolicyMethod` for `Ransac{threshold_m, iterations, min_inliers, seed}`.
- **Algorithm:** Fischler and Bolles 1981, "Random sample consensus"; distinct-index sampling by partial Fisher-Yates over `fid_num::rng` cursor (seed, stream = round index); minimal solvers by direct formulas (cross product for planes, circumcircle and circumsphere by small `fid_num::la` solves).
- **Errors:** `PolicyError::{NoConsensus, InvalidParameter}` and guard errors.
- **Determinism:** D2 (seeded; bit-exact for a given seed).
- **Performance budget:** 1 000 rounds on 100 000 points < 400 ms [unverified until measured].
- **Crate:** `fid-filter` (module `ransac`).

## Test fixtures

`fixtures/out/ransac-contam-v1`: seeded synthetic generator, CC-BY-4.0.

## Build prompt

> In crate `fid-filter`, create module `ransac` implementing `PolicyMethod` for `Ransac{threshold_m, iterations, min_inliers, seed}` (`filter.outlier.ransac@1.0.0`) for line, plane, circle and sphere targets. Round i draws distinct indices by partial Fisher-Yates from `fid_num::rng::Cursor{seed, stream: i}`; solve the minimal sample exactly; count orthogonal distances <= threshold; keep the largest consensus (ties: smaller in-threshold sum of squares, then lower round). Exclude the rest; apply `policy` guards. Typed `NoConsensus` if no round reaches `min_inliers` or all samples are degenerate. Tests: `fixtures/out/ransac-contam-v1` (inlier and outlier recovery), cross-target identity of masks on x86-64/ARM64/wasm32, seed reproducibility, collinear-plane error. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, single-threaded, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.OUT.RANSAC.

## Open questions

- None.

## Children

_Leaf: none._
