---
id: KRN.MZ.EXT.CONE
title: Minimum-zone cone
level: 4
parent: KRN.MZ.EXT
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.EXT-001, REQ-KRN.MZ.EXT-003, REQ-KRN.MZ.EXT-004]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.AXIS.CONE, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MZ.EXT.UNDEF, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit mz-cone]
mcp: [fit_mz_cone]
journal: [fit.mz_cone@1]
updated: 2026-11-17
---

# KRN.MZ.EXT.CONE: Minimum-zone cone

## Purpose

The Chebyshev cone: the cone whose surface minimises the maximum orthogonal deviation of the points, so that two coaxial cones of equal half-angle, offset normally by the zone width, contain all points. It is non-convex in the axis and half-angle, so it runs sequential linearisation from the least-squares cone with multi-start, and reports a recomputed zone width with a certificate.

## Scope

**In:** algorithm `fit.cone.mz@1.0.0`; the same apex-free parameterisation as `KRN.LSQ.AXIS.CONE`; linearised LP steps (minimise t subject to -t/2 <= d_i <= t/2) through `KRN.SOLV.SEQ`; multi-start; certificate and width recomputed from the critical points; partial-coverage guard (angular coverage about the axis); optional fixed half-angle; the command; a spec card.
**Out:** least-squares cone (`KRN.LSQ.AXIS.CONE`); conicity tolerance evaluation (`GDT`); reference cases (`UNC`).

## Requirements

- **REQ-KRN.MZ.EXT.CONE-001** The module shall compute the minimum-zone cone implementing the `Fit` trait, from the least-squares cone, with multi-start and `local_optimum_only` when starts disagree.
- **REQ-KRN.MZ.EXT.CONE-002** The reported zone width shall be recomputed from the critical point set at the final parameters, and the result shall carry the certificate from `KRN.SOLV.CERT`.
- **REQ-KRN.MZ.EXT.CONE-003** If the angular coverage about the axis is below the configured threshold, then the fit shall return `InsufficientCoverage` and no zone; ill-posed half-angles shall return `IllConditioned` as in the least-squares cone.
- **REQ-KRN.MZ.EXT.CONE-004** When the command is committed, the system shall journal one `fit.mz_cone@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mz-cone-v1` (seeded cones with known lobed form error, half-angles 5 to 60 degrees), when `fid fit mz-cone --seed 6 --json` runs, then the zone width is within 1e-8 m of the generator's analytic zone [unverified] and not larger than the least-squares cone's peak-to-valley.
- **AC-2** (error path) Given a cone sampled over 45 degrees about its axis with a 90-degree threshold, then the fit returns `InsufficientCoverage` and journals nothing.
- **AC-3** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.
- **AC-4** Given the algorithm ID, then a spec card exists with `reference_basis` stated, and the card check passes.

## Design

- **Registry command(s):** `fit.mz_cone@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, coverage threshold (rad), optional fixed half-angle, `SeqConfig`. Result: `FittedFeature::Cone`, zone width (m), certificate, multi-start summary.
- **Algorithm:** linearise orthogonal distances to the cone in the parameters; LP step via `fid_solv::seq::solve`; certify via `fid_solv::cert::certify` with the equal-deviation active set.
- **Errors:** `TooFewPoints`, `InsufficientCoverage`, `IllConditioned`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 50 000 points, 8 starts < 800 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_cone`).

## Test fixtures

`fixtures/fit/mz-cone-v1`: seeded generator with analytic zones, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_cone` implementing the `Fit` trait for `fit.cone.mz@1.0.0`: start from the least-squares cone (apex-free parameterisation), LP steps minimise t s.t. |d_i| <= t/2 via `fid_solv::seq::solve`, multi-start via `fid_solv::mstart` with `local_optimum_only`, certificate via `fid_solv::cert::certify`, width recomputed from critical points, `InsufficientCoverage` below the explicit angular threshold, optional fixed half-angle; register in `mz_table`. Add the registry data row `fit.mz_cone@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mz-cone`; MCP `fit_mz_cone`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/mz-cone-v1`, partial-coverage error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL, card check. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MZ.EXT.CONE.

## Open questions

- No published reference for minimum-zone cones is known [unverified]; results carry `reference_basis: none` or `fiducial-ORS` as `UNC` decides.

## Children

_Leaf: none._
