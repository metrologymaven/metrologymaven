---
id: KRN.MZ.RND.CYL
title: Minimum-zone cylinder
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
depends_on: [KRN.LSQ.AXIS.CYL, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mz-cylinder]
mcp: [fit_mz_cylinder]
journal: [fit.mz_cylinder@1]
updated: 2026-11-17
---

# KRN.MZ.RND.CYL: Minimum-zone cylinder

## Purpose

Cylindricity: the pair of coaxial cylinders with the smallest radial separation containing all points. It is the hardest TraCIM Chebyshev case: non-convex in the axis direction, so it linearises in two tangent-plane axis parameters, axis point, and mean radius about the least-squares cylinder, runs a deterministic multi-start and reports the width recomputed from the critical points.

## Scope

**In:** algorithm `fit.cylinder.mz@1.0.0`; axis by two tangent-plane parameters, re-parameterised after each accepted step; LP steps via `KRN.SOLV.SEQ`; multi-start with the `KRN.LSQ.INIT`-style start spec from the least-squares cylinder; coverage guards (angular coverage about the axis and axial extent relative to radius); certificate and recomputed width; the command; a spec card.
**Out:** circle and sphere (`KRN.MZ.RND.CIRC`, `KRN.MZ.RND.SPH`); the least-squares cylinder (`KRN.LSQ.AXIS.CYL`); cylindricity evaluation (`GDT`).

## Requirements

- **REQ-KRN.MZ.RND.CYL-001** The module shall compute the minimum-zone cylinder implementing the `Fit` trait, from the least-squares cylinder, through `KRN.SOLV.SEQ`, with a deterministic multi-start and `local_optimum_only` when starts disagree.
- **REQ-KRN.MZ.RND.CYL-002** If angular coverage about the axis or axial extent relative to the radius is below its configured threshold, then the fit shall return `InsufficientCoverage` or `IllConditioned{measure: "aspect"}`.
- **REQ-KRN.MZ.RND.CYL-003** The reported width shall be recomputed from the critical point set at the final axis and carried with the certificate.
- **REQ-KRN.MZ.RND.CYL-004** On the PTB TraCIM Chebyshev cylinder sets from the frozen RDL release, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.MZ.RND.CYL-005** When the command is committed, the system shall journal one `fit.mz_cylinder@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Chebyshev cylinder sets (`rdl/tracim-cheb-v0`, from `UNC.RDL.FREEZE` at CI time), when `fid fit mz-cylinder --json` runs on each case, then parameters and width are within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given fixture `fixtures/fit/mz-cyl-v1` (seeded cylinders with lobing, taper and bending, full and half circumference), then widths equal the analytic zones within 1e-9 m [unverified], and on half-circumference cases with two near-equal optima `local_optimum_only` is set.
- **AC-3** (error path) Given a 60-degree sector with a 120-degree threshold, then the fit returns `InsufficientCoverage` and journals nothing.
- **AC-4** Given AC-2 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mz_cylinder@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, coverage and aspect thresholds, `SeqConfig`, `StartSpec` overrides. Result: `FittedFeature::Cylinder` (mean cylinder), zone width (m), certificate, multi-start summary.
- **Algorithm:** linearised radial deviations; LP steps via `fid_solv::seq::solve`; `fid_solv::mstart`; `ActiveSetProblem` callbacks; certify via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `InsufficientCoverage`, `IllConditioned`, `NotConverged`, `Solver`, `MultiStart`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 100 000 points, 8 starts < 1.5 s [unverified until measured].
- **Crate:** `fid-fit` (module `mz_cyl`).

## Test fixtures

TraCIM Chebyshev sets from `UNC.RDL.FREEZE` (terms [unverified]). `fixtures/fit/mz-cyl-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_cyl` implementing the `Fit` trait for `fit.cylinder.mz@1.0.0`: least-squares cylinder start, axis by two tangent-plane parameters re-parameterised per accepted step, LP steps minimise t s.t. |d_i| <= t/2 via `fid_solv::seq::solve`, multi-start via `fid_solv::mstart` with `local_optimum_only`, certificate via `fid_solv::cert::certify`, width recomputed from critical points, `InsufficientCoverage` and `IllConditioned{measure: "aspect"}` guards. Add the registry data row `fit.mz_cylinder@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mz-cylinder`; MCP `fit_mz_cylinder`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: TraCIM Chebyshev cylinder sets at CI time, `fixtures/fit/mz-cyl-v1` incl. local-optimum flag, sector error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MZ.RND.CYL.

## Open questions

- **Enclosure obligation (from KRN.SOLV.CERT.RECOMP, #321):** every `ActiveSetProblem` must implement `value_enclosure`, and a nonlinear one also `residual_enclosure` and `jacobian_enclosure`; otherwise recomputation, and so `certify`, fails with `BoundNotProven`. For a linear problem the proven bound applies to the f64 data as given (the Jacobian and residuals at p = 0 are taken as the exact system).

## Children

_Leaf: none._
