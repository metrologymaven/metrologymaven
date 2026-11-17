---
id: KRN.MZ.RND.SPH
title: Minimum-zone sphere
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
depends_on: [KRN.LSQ.RND, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mz-sphere]
mcp: [fit_mz_sphere]
journal: [fit.mz_sphere@1]
updated: 2026-11-17
---

# KRN.MZ.RND.SPH: Minimum-zone sphere

## Purpose

Sphericity: the pair of concentric spheres with the smallest radial separation containing all points. Solved by sequential LP from the least-squares sphere with multi-start; the width is recomputed from the critical points (generically five or six). Reference spheres and partial caps (a probed hemisphere) are the common cases, so a solid-angle coverage guard applies.

## Scope

**In:** algorithm `fit.sphere.mz@1.0.0`; linearised radial deviations in centre (3) and mean radius; LP steps via `KRN.SOLV.SEQ`; multi-start; coverage guard (largest empty cap angle); certificate and recomputed width; the command; a spec card.
**Out:** circle and cylinder (`KRN.MZ.RND.CIRC`, `KRN.MZ.RND.CYL`); sphericity evaluation (`GDT`).

## Requirements

- **REQ-KRN.MZ.RND.SPH-001** The module shall compute the minimum-zone sphere implementing the `Fit` trait, from the least-squares sphere, through `KRN.SOLV.SEQ`, with a deterministic multi-start and `local_optimum_only` when starts disagree.
- **REQ-KRN.MZ.RND.SPH-002** If the points' angular coverage (measured as the half-angle of the largest empty spherical cap about the least-squares centre, subtracted from 180 degrees) is below the configured threshold, then the fit shall return `InsufficientCoverage`.
- **REQ-KRN.MZ.RND.SPH-003** The reported width shall be recomputed from the critical point set and carried with the certificate.
- **REQ-KRN.MZ.RND.SPH-004** On the PTB TraCIM Chebyshev sphere sets from the frozen RDL release, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.MZ.RND.SPH-005** When the command is committed, the system shall journal one `fit.mz_sphere@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given the TraCIM Chebyshev sphere sets (`rdl/tracim-cheb-v0`, from `UNC.RDL.FREEZE` at CI time), when `fid fit mz-sphere --json` runs on each case, then parameters and width are within 1/10 of the TraCIM default MPE [unverified].
- **AC-2** Given fixture `fixtures/fit/mz-sph-v1` (seeded spheres with known zones, full and hemispherical), then widths equal the analytic zones within 1e-10 m [unverified].
- **AC-3** (error path) Given a 20-degree cap with a 60-degree threshold, then the fit returns `InsufficientCoverage` and journals nothing.
- **AC-4** Given AC-2 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mz_sphere@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, coverage threshold (rad), `SeqConfig`. Result: `FittedFeature::Sphere` (mean sphere), zone width (m), certificate, multi-start summary.
- **Algorithm:** as `KRN.MZ.RND.CIRC` in three dimensions; coverage from the least-squares centre.
- **Errors:** `TooFewPoints`, `Coplanar`, `InsufficientCoverage`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 100 000 points, 4 starts < 300 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_sph`).

## Test fixtures

TraCIM Chebyshev sets from `UNC.RDL.FREEZE` (terms [unverified]). `fixtures/fit/mz-sph-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_sph` implementing the `Fit` trait for `fit.sphere.mz@1.0.0`: least-squares sphere start, LP steps minimise t s.t. |d_i| <= t/2 via `fid_solv::seq::solve`, multi-start via `fid_solv::mstart` with `local_optimum_only`, certificate via `fid_solv::cert::certify`, width recomputed from critical points, `InsufficientCoverage` from the largest-empty-cap measure. Add the registry data row `fit.mz_sphere@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mz-sphere`; MCP `fit_mz_sphere`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: TraCIM Chebyshev sphere sets at CI time, `fixtures/fit/mz-sph-v1`, small-cap error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.MZ.RND.SPH.

## Open questions

- None beyond the parent's.

## Children

_Leaf: none._
