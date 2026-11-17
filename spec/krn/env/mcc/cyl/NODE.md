---
id: KRN.ENV.MCC.CYL
title: Minimum circumscribed cylinder
level: 4
parent: KRN.ENV.MCC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV.MCC-001, REQ-KRN.ENV.MCC-003, REQ-KRN.ENV.MCC-005]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.AXIS.CYL, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mcc-cylinder]
mcp: [fit_mcc_cylinder]
journal: [fit.mcc_cylinder@1]
updated: 2026-11-17
---

# KRN.ENV.MCC.CYL: Minimum circumscribed cylinder

## Purpose

The smallest cylinder containing all points: the mating envelope of a shaft. Unlike the circle and sphere, the problem is non-convex in the axis direction, so it is solved by sequential linearisation about the least-squares axis with a deterministic multi-start, and the result says plainly when global optimality is not proven.

## Scope

**In:** algorithm `fit.cylinder.mcc@1.0.0`; axis parameterised by two tangent-plane parameters about the current axis; each linearised step an SOCP through `KRN.SOLV.SEQ`; multi-start from `KRN.LSQ.AXIS.CYL` through `KRN.SOLV.MSTART`; contact set, local optimality check and `local_optimum_only`; optional fixed axis direction (for an envelope constrained to a datum direction); the command; a spec card.
**Out:** circle and sphere (`KRN.ENV.MCC.CVX`); least-squares cylinder (`KRN.LSQ.AXIS.CYL`); actual mating envelope rules under a dialect (`GDT`).

## Requirements

- **REQ-KRN.ENV.MCC.CYL-001** The module shall compute the minimum circumscribed cylinder implementing the `Fit` trait, starting from the least-squares cylinder axis and running every start of the deterministic multi-start.
- **REQ-KRN.ENV.MCC.CYL-002** The result shall carry the contact set, the radius recomputed from the contact set and final axis, the multi-start agreement flag, the local optimality check outcome, and `local_optimum_only: true` whenever starts disagree beyond tolerance or the local check fails.
- **REQ-KRN.ENV.MCC.CYL-003** When a fixed axis direction is given, the module shall solve the convex 2D problem in the plane perpendicular to that direction and shall report `local_optimum_only: false`.
- **REQ-KRN.ENV.MCC.CYL-004** If the points' axial extent is below the conditioning threshold relative to the radius (a disc rather than a cylinder), then the fit shall return `IllConditioned` with the ratio.
- **REQ-KRN.ENV.MCC.CYL-005** When the command is committed, the system shall journal one `fit.mcc_cylinder@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mcc-cyl-v1` (seeded cylinders with known form error, full and half circumference, 1 000 to 200 000 points), when `fid fit mcc-cylinder --seed 7 --json` runs, then the radius is within 1e-8 m of the generator's analytic envelope [unverified] and every point lies inside within 1e-12 m.
- **AC-2** Given a case built with two near-equal envelope axes, then the result reports `agreement: false` and `local_optimum_only: true`.
- **AC-3** Given a fixed axis direction, then the result equals `fit.circle2d.mcc` on the projected points within 1e-12 m and `local_optimum_only` is false.
- **AC-4** (error path) Given a 0.1 mm-thick disc of points of radius 20 mm, then the fit returns `IllConditioned` and journals nothing.
- **AC-5** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mcc_cylinder@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, optional fixed axis direction, `SeqConfig`, `StartSpec` overrides. Result: `FittedFeature::Cylinder`, contact set, certificate, multi-start summary, residual statistics.
- **Algorithm:** at each iterate, linearise distances to the axis in the two tangent-plane parameters plus centre offsets and radius; solve the SOCP step; accept by the `KRN.SOLV.SEQ` merit rule; certify at the end; multi-start through `fid_solv::mstart` with the `KRN.LSQ.INIT`-style start spec from the least-squares fit.
- **Errors:** `TooFewPoints`, `IllConditioned`, `NotConverged`, `Solver`, `MultiStart(AllFailed)`.
- **Determinism:** D0 for a given seed (the seed is journaled).
- **Performance budget:** 50 000 points with 8 starts < 500 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mcc_cyl`).

## Test fixtures

`fixtures/fit/mcc-cyl-v1`: seeded synthetic generator with analytic envelopes, CC-BY-4.0; open reference cases from the RDL release (`UNC.RDL.FREEZE`) at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mcc_cyl` implementing the `Fit` trait for `fit.cylinder.mcc@1.0.0`: start from the least-squares cylinder, parameterise the axis by two tangent-plane parameters, solve linearised SOCP steps via `fid_solv::seq::solve`, run all starts via `fid_solv::mstart`, certify via `fid_solv::cert::certify`, recompute the radius from the contact set, set `local_optimum_only` when starts disagree or the local check fails; with a fixed axis direction solve the convex 2D problem instead; return `IllConditioned` for disc-like data. Add the registry data row `fit.mcc_cylinder@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mcc-cylinder`; MCP `fit_mcc_cylinder`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/mcc-cyl-v1` analytic envelopes, ambiguous-axis case, fixed-direction equivalence to the 2D circle, disc error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.ENV.MCC.CYL.

## Open questions

- No NMI reference exists for the MCC cylinder [unverified]; evidence is synthetic and open-suite only.

## Children

_Leaf: none._
