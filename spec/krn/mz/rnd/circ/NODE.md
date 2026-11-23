---
id: KRN.MZ.RND.CIRC
title: Minimum-zone 2D circle and Voronoi agreement job
level: 4
parent: KRN.MZ.RND
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.RND-001, REQ-KRN.MZ.RND-002, REQ-KRN.MZ.RND-003, REQ-KRN.MZ.RND-004, REQ-KRN.MZ.RND-005, REQ-KRN.MZ.RND-006]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.RND, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MZ.VOR, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mz-circle2d]
mcp: [fit_mz_circle2d]
journal: [fit.mz_circle2d@1]
built_in: [fid-fit]
updated: 2026-11-23
---

# KRN.MZ.RND.CIRC: Minimum-zone 2D circle and Voronoi agreement job

## Purpose

Roundness: the pair of concentric circles with the smallest radial separation containing all points projected into a plane. Solved by sequential LP from the least-squares circle with multi-start; the width is recomputed from the critical points (generically four, alternating). A CI job compares every reference case with the exact Voronoi path of `KRN.MZ.VOR` and fails on disagreement.

## Scope

**In:** algorithm `fit.circle2d.mz@1.0.0`; linearised radial deviations in centre (2) and mean radius; LP steps (minimise t subject to -t/2 <= d_i <= t/2) through `KRN.SOLV.SEQ`; multi-start; angular-coverage guard; certificate and recomputed width; the CI agreement job with `KRN.MZ.VOR`; the command; a spec card.
**Out:** the Voronoi algorithm (`KRN.MZ.VOR`); sphere and cylinder (`KRN.MZ.RND.SPH`, `KRN.MZ.RND.CYL`); roundness evaluation with filters (`GDT`).

## Requirements

- **REQ-KRN.MZ.RND.CIRC-001** The module shall compute the minimum-zone 2D circle implementing the `Fit` trait, from the least-squares circle, through `KRN.SOLV.SEQ`, with a deterministic multi-start and `local_optimum_only` when starts disagree.
- **REQ-KRN.MZ.RND.CIRC-002** If the angular coverage of the projected points is below the configured threshold, then the fit shall return `InsufficientCoverage` and no zone.
- **REQ-KRN.MZ.RND.CIRC-003** The reported width shall be recomputed from the critical point set and carried with the certificate.
- **REQ-KRN.MZ.RND.CIRC-004** On the PTB TraCIM Chebyshev 2D circle sets from the frozen RDL release, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.MZ.RND.CIRC-005** On every reference case, CI shall compare centre and width with `KRN.MZ.VOR` and fail if they differ by more than 1e-9 m [unverified], naming the case.
- **REQ-KRN.MZ.RND.CIRC-006** When the command is committed, the system shall journal one `fit.mz_circle2d@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Chebyshev circle sets (`rdl/tracim-cheb-v0`, from `UNC.RDL.FREEZE` at CI time), when `fid fit mz-circle2d --json` runs on each case, then parameters and width are within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given the same cases, when the agreement job runs, then it passes; an injected 1e-8 m shift fails it with the case ID.
- **AC-3** (error path) Given a 40-degree arc with a 90-degree threshold, then the fit returns `InsufficientCoverage` and journals nothing.
- **AC-4** Given fixture `fixtures/fit/mz-circ-v1` (seeded lobed circles far from origin) on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mz_circle2d@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), projection plane, seed, coverage threshold (rad), `SeqConfig`. Result: `FittedFeature::Circle2` (mean circle), zone width (m), certificate, multi-start summary.
- **Algorithm:** least-squares start; LP steps via `fid_solv::seq::solve`; `fid_solv::mstart`; `ActiveSetProblem` callbacks for the 2-2 alternating critical set; certify via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `Collinear`, `InsufficientCoverage`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 100 000 points, 4 starts < 200 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_circ`).

## Test fixtures

TraCIM Chebyshev sets from `UNC.RDL.FREEZE` (terms [unverified]; fetched at CI time). `fixtures/fit/mz-circ-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_circ` implementing the `Fit` trait for `fit.circle2d.mz@1.0.0`: least-squares circle start, LP steps minimise t s.t. |d_i| <= t/2 via `fid_solv::seq::solve`, multi-start via `fid_solv::mstart` with `local_optimum_only`, `ActiveSetProblem` callbacks, certificate via `fid_solv::cert::certify`, width recomputed from critical points, `InsufficientCoverage` below the explicit angular threshold. Add a CI test comparing every frozen reference case with `fid_fit::vor` (fail > 1e-9 m, naming the case). Add the registry data row `fit.mz_circle2d@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mz-circle2d`; MCP `fit_mz_circle2d`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: TraCIM Chebyshev circle sets at CI time, agreement job with injected-shift negative test, partial-arc error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MZ.RND.CIRC.

## Open questions

- **Enclosure obligation (from KRN.SOLV.CERT.RECOMP, #321):** every `ActiveSetProblem` must implement `value_enclosure`, and a nonlinear one also `residual_enclosure` and `jacobian_enclosure`; otherwise recomputation, and so `certify`, fails with `BoundNotProven`. For a linear problem the proven bound applies to the f64 data as given (the Jacobian and residuals at p = 0 are taken as the exact system).

## Children

_Leaf: none._
