---
id: KRN.MZ.EXT.ELL
title: Minimum-zone ellipse
level: 4
parent: KRN.MZ.EXT
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ.EXT-001, REQ-KRN.MZ.EXT-003, REQ-KRN.MZ.EXT-004]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.PLAN2D.ELL, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MZ.EXT.UNDEF, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit mz-ellipse]
mcp: [fit_mz_ellipse]
journal: [fit.mz_ellipse@1]
updated: 2026-11-17
---

# KRN.MZ.EXT.ELL: Minimum-zone ellipse

## Purpose

The Chebyshev ellipse in a caller-given plane: the ellipse that minimises the maximum orthogonal deviation, so that a band of constant width about it contains all points (for example an elliptical bore or an oval cam section). It starts from the least-squares orthogonal-distance ellipse and refines by sequential LP.

## Scope

**In:** algorithm `fit.ellipse.mz@1.0.0`; parameters as `KRN.LSQ.PLAN2D.ELL`; linearised orthogonal distances (foot points as in the least-squares ellipse); LP steps (minimise t subject to -t/2 <= d_i <= t/2); multi-start; certificate and width recomputed from critical points; angular-coverage guard about the centre; near-circle error; the command; a spec card.
**Out:** least-squares ellipse (`KRN.LSQ.PLAN2D.ELL`); minimum-zone circle (`KRN.MZ.RND`).

## Requirements

- **REQ-KRN.MZ.EXT.ELL-001** The module shall compute the minimum-zone ellipse implementing the `Fit` trait, from the least-squares ellipse, with multi-start and `local_optimum_only` when starts disagree.
- **REQ-KRN.MZ.EXT.ELL-002** The reported zone width shall be recomputed from the critical point set, and the result shall carry the certificate.
- **REQ-KRN.MZ.EXT.ELL-003** If angular coverage about the centre is below the configured threshold, or b/a is within `ratio_tol` of 1, then the fit shall return `InsufficientCoverage` or `IllConditioned{measure: "axis_ratio"}`.
- **REQ-KRN.MZ.EXT.ELL-004** When the command is committed, the system shall journal one `fit.mz_ellipse@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mz-ell-v1` (seeded ellipses with lobed deviations, b/a 0.3 to 0.95), when `fid fit mz-ellipse --seed 8 --json` runs, then the zone width is within 1e-9 m of the analytic zone [unverified] and not larger than the least-squares peak-to-valley.
- **AC-2** (error path) Given a 60-degree arc with a 180-degree coverage threshold, then the fit returns `InsufficientCoverage` and journals nothing.
- **AC-3** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mz_ellipse@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), plane frame, seed, coverage threshold, `ratio_tol`, `SeqConfig`. Result: `FittedFeature::Ellipse`, zone width (m), certificate, multi-start summary.
- **Algorithm:** least-squares start; linearisation of orthogonal distances via foot points; LP step via `fid_solv::seq::solve`; certify via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `InsufficientCoverage`, `IllConditioned`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 50 000 points, 4 starts < 400 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mz_ell`).

## Test fixtures

`fixtures/fit/mz-ell-v1`: seeded generator with analytic zones, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mz_ell` implementing the `Fit` trait for `fit.ellipse.mz@1.0.0`: start from `ls_ell`, linearise orthogonal distances via foot points, LP steps minimise t s.t. |d_i| <= t/2 via `fid_solv::seq::solve`, multi-start via `fid_solv::mstart` with `local_optimum_only`, certificate via `fid_solv::cert::certify`, width recomputed from critical points, `InsufficientCoverage` and near-circle `IllConditioned`; register in `mz_table`. Add the registry data row `fit.mz_ellipse@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mz-ellipse`; MCP `fit_mz_ellipse`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/mz-ell-v1`, partial-arc error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-3 of spec node KRN.MZ.EXT.ELL.

## Open questions

- None beyond the parent's reference-basis question.

## Children

_Leaf: none._
