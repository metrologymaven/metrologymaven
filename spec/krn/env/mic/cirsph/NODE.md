---
id: KRN.ENV.MIC.CIRSPH
title: Maximum inscribed circle and sphere
level: 4
parent: KRN.ENV.MIC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV.MIC-001, REQ-KRN.ENV.MIC-004, REQ-KRN.ENV.MIC-005]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.ENV.MIC.TR, KRN.LSQ.RND, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.ENV.XCHK, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mic-circle2d, fid fit mic-sphere]
mcp: [fit_mic_circle2d, fit_mic_sphere]
journal: [fit.mic_circle2d@1, fit.mic_sphere@1]
updated: 2026-11-17
---

# KRN.ENV.MIC.CIRSPH: Maximum inscribed circle and sphere

## Purpose

The largest circle (in a given plane) or sphere that fits inside the points without containing any: the mating envelope of a hole section or a spherical socket. The problem is non-convex, so it is solved by sequential linearisation inside the trust region of `KRN.ENV.MIC.TR`, with multi-start, a local optimality check and, for the 2D circle, a CI agreement job against the exact Voronoi path of `KRN.ENV.XCHK`.

## Scope

**In:** algorithms `fit.circle2d.mic@1.0.0` and `fit.sphere.mic@1.0.0`; linearised step maximise r subject to |p_i - c_k| + g_i . (c - c_k) >= r and the trust-region rows; multi-start; contact set and certificate; `local_optimum_only`; the 2D agreement job with `KRN.ENV.XCHK` (the XCHK region is the convex hull intersected with the same trust disc); the two commands; spec cards.
**Out:** trust region and defined pairs (`KRN.ENV.MIC.TR`); the cylinder (`KRN.ENV.MIC.CYL`); the Voronoi algorithm (`KRN.ENV.XCHK`); Rule #1 and bonus (`GDT`).

## Requirements

- **REQ-KRN.ENV.MIC.CIRSPH-001** The module shall compute the maximum inscribed 2D circle and sphere implementing the `Fit` trait, with the centre constrained to the trust region of `KRN.ENV.MIC.TR`.
- **REQ-KRN.ENV.MIC.CIRSPH-002** Each result shall carry the contact set, the radius recomputed from the contact set, the local optimality check outcome, the multi-start agreement flag and `local_optimum_only` whenever global optimality is not proven.
- **REQ-KRN.ENV.MIC.CIRSPH-003** If the optimum lies on the trust-region boundary, then the fit shall return `unbounded_envelope` from `KRN.ENV.MIC.TR` and no envelope.
- **REQ-KRN.ENV.MIC.CIRSPH-004** For the 2D circle, CI shall compare the result with `KRN.ENV.XCHK` on every frozen reference case and fail if centre or radius differ by more than 1e-9 m [unverified], or if one path reports a boundary optimum and the other does not.
- **REQ-KRN.ENV.MIC.CIRSPH-005** When a command is committed, the system shall journal one `fit.mic_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mic-cirsph-v1` (seeded holes and sockets with lobed form, full coverage, 50 to 100 000 points), when `fid fit mic-circle2d --tr-fraction 0.2 --seed 3 --json` and `fid fit mic-sphere ... --json` run, then radii match the generator's analytic inscribed radius within 1e-8 m [unverified] and no point lies inside the envelope by more than 1e-12 m.
- **AC-2** Given the frozen reference-set MIC circle cases (`UNC.RDL.FREEZE`), when the agreement job runs, then it passes; an injected 1e-8 m shift fails it with the case ID.
- **AC-3** (error path) Given a 60-degree arc of points, then the circle fit returns `unbounded_envelope` and journals nothing, and `KRN.ENV.XCHK` reports `on_region_boundary` for the same case.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.
- **AC-5** Given each algorithm ID, then a spec card exists and the card check passes.

## Design

- **Registry command(s):** `fit.mic_circle2d@1`, `fit.mic_sphere@1`; W2 (dry-run R1). Params: point-set handle (`PointsView`), projection plane (circle2d), `tr_fraction` (required), `boundary_tol` (m), seed, `SeqConfig`. Result: `FittedFeature::{Circle2, Sphere}`, contact set, certificate, multi-start summary, residual statistics.
- **Algorithm:** least-squares start from `KRN.LSQ.RND`; trust region from `KRN.ENV.MIC.TR`; LP steps through `fid_solv::seq::solve`; multi-start over centre perturbations inside the region; certify via `fid_solv::cert::certify`.
- **Errors:** `TooFewPoints`, `Collinear`, `UnboundedEnvelope`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 100 000-point circle with 8 starts < 300 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mic_cirsph`).

## Test fixtures

`fixtures/fit/mic-cirsph-v1`: seeded generator with analytic inscribed radii and partial-arc cases, CC-BY-4.0; reference cases from `UNC.RDL.FREEZE` at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mic_cirsph` implementing the `Fit` trait for `fit.circle2d.mic@1.0.0` and `fit.sphere.mic@1.0.0`: least-squares start, trust region from `mic_tr` (required `tr_fraction`), linearised LP steps maximise r s.t. |p_i - c_k| + g_i.(c - c_k) >= r plus trust rows via `fid_solv::seq::solve`, multi-start via `fid_solv::mstart`, certificate via `fid_solv::cert::certify`, radius recomputed from contacts, `local_optimum_only` when unproven, `UnboundedEnvelope` on the boundary. Add a CI test comparing the 2D circle with `fid_fit::xchk` Voronoi MIC on every frozen reference case (fail > 1e-9 m or boundary-status mismatch). Add registry data rows `fit.mic_circle2d@1`, `fit.mic_sphere@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mic-<primitive>`; MCP `fit_mic_<primitive>`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: `fixtures/fit/mic-cirsph-v1`, agreement job with injected-shift negative test, 60-degree arc error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL, card check. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.ENV.MIC.CIRSPH.

## Open questions

- The polygonal trust region (32 facets) is slightly smaller than the disc used by `KRN.ENV.XCHK`; the agreement job must use the same polygon on both sides, which `KRN.ENV.XCHK` accepts (any convex polygon).

## Children

_Leaf: none._
