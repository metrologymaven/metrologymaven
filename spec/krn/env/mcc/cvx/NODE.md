---
id: KRN.ENV.MCC.CVX
title: Minimum circumscribed circle and sphere with duality-gap certificate
level: 4
parent: KRN.ENV.MCC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV.MCC-001, REQ-KRN.ENV.MCC-002, REQ-KRN.ENV.MCC-004, REQ-KRN.ENV.MCC-005]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.RND, KRN.SOLV.CONIC, KRN.SOLV.CERT, KRN.ENV.XCHK, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mcc-circle2d, fid fit mcc-sphere]
mcp: [fit_mcc_circle2d, fit_mcc_sphere]
journal: [fit.mcc_circle2d@1, fit.mcc_sphere@1]
updated: 2026-11-17
---

# KRN.ENV.MCC.CVX: Minimum circumscribed circle and sphere with duality-gap certificate

## Purpose

The smallest circle (in a given plane) or sphere containing all points: the mating envelope of a shaft section or a ball. The problem is a convex second-order cone program, so the global optimum can be certified by a duality gap and checked against the independent Welzl algorithm in `KRN.ENV.XCHK`. This leaf also owns the table of primitives for which a minimum circumscribed fit is defined.

## Scope

**In:** algorithms `fit.circle2d.mcc@1.0.0` and `fit.sphere.mcc@1.0.0` as SOCP (minimise r subject to |p_i - c| <= r) through `KRN.SOLV.CONIC`; contact set and certificate through `KRN.SOLV.CERT`; radius recomputed from the contact set; the MCC defined-pairs table (circle2d, sphere, cylinder defined; every other primitive `method_undefined`); the CI agreement job with `KRN.ENV.XCHK`; the two commands; spec cards.
**Out:** the cylinder (`KRN.ENV.MCC.CYL`); the Welzl algorithm (`KRN.ENV.XCHK`); Rule #1 and MMC bonus (`GDT`).

## Requirements

- **REQ-KRN.ENV.MCC.CVX-001** The module shall compute the minimum circumscribed 2D circle (points projected into a caller-given plane) and sphere through the conic solver, implementing the `Fit` trait.
- **REQ-KRN.ENV.MCC.CVX-002** Each result shall carry the contact set (2 to 3 points for a circle, 2 to 4 for a sphere), the duality gap, and the radius recomputed from the contact set; the result shall be rejected with `NotConverged` if the duality gap exceeds 1e-12 m [unverified].
- **REQ-KRN.ENV.MCC.CVX-003** On every case of the frozen reference set for these primitives, CI shall compare centre and radius with `KRN.ENV.XCHK` and fail if they differ by more than 1e-9 m [unverified].
- **REQ-KRN.ENV.MCC.CVX-004** If a minimum circumscribed fit is requested for a primitive not in the defined-pairs table (for example line, plane, slot, rectangle), then the system shall return `method_undefined` naming the primitive and method.
- **REQ-KRN.ENV.MCC.CVX-005** When a command is committed, the system shall journal one `fit.mcc_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mcc-cvx-v1` (seeded circles and spheres, full and partial, 4 to 100 000 points, far from origin), when `fid fit mcc-circle2d --json` and `fid fit mcc-sphere --json` run, then each duality gap is at most 1e-12 m [unverified] and centre and radius agree with `KRN.ENV.XCHK` within 1e-9 m [unverified].
- **AC-2** Given the frozen reference-set cases for MCC circle and sphere from `UNC.RDL.FREEZE`, when the CI agreement job runs, then it passes; an injected 1e-8 m perturbation of the conic result makes it fail with the case ID.
- **AC-3** (error path) Given a request for MCC of a plane, then the command returns `method_undefined{primitive: plane, method: mcc}` and journals nothing.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32, then output digests are identical; given a committed run, replay in exact mode is `IDENTICAL`.
- **AC-5** Given each algorithm ID, then a spec card exists and the card check passes.

## Design

- **Registry command(s):** `fit.mcc_circle2d@1`, `fit.mcc_sphere@1`; W2 (dry-run R1). Params: point-set handle (resolved into `PointsView`), projection plane (circle2d), solver tolerances. Result: `FittedFeature::{Circle2, Sphere}`, contact set (indices), certificate, residual statistics.
- **Algorithm:** start from the `KRN.LSQ.RND` least-squares centre for scaling; SOCP via `fid_solv::conic::solve_socp`; certify via `fid_solv::cert::certify` with the active set as contacts; recompute the radius as the circumradius of the contact set.
- **Errors:** `TooFewPoints`, `Collinear`, `NotConverged`, `MethodUndefined`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 100 000-point circle < 50 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mcc_cvx`).

## Test fixtures

`fixtures/fit/mcc-cvx-v1`: seeded synthetic generator, CC-BY-4.0. Frozen reference-set cases come from the RDL release (`UNC.RDL.FREEZE`) at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mcc_cvx` implementing the `Fit` trait for `fit.circle2d.mcc@1.0.0` (points projected into a given plane) and `fit.sphere.mcc@1.0.0`: SOCP minimise r s.t. |p_i - c| <= r via `fid_solv::conic::solve_socp`, certificate via `fid_solv::cert::certify`, contact set (2-3 or 2-4 points), duality gap <= 1e-12 m else `NotConverged`, radius recomputed from the contact set. Add the MCC defined-pairs table (circle2d, sphere, cylinder; others `MethodUndefined`). Add a CI test comparing with `fid_fit::xchk` Welzl on every frozen reference case (fail > 1e-9 m). Add registry data rows `fit.mcc_circle2d@1`, `fit.mcc_sphere@1` (W2, dry-run R1, D0; CLI `fid fit mcc-<primitive>`; MCP `fit_mcc_<primitive>`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: `fixtures/fit/mcc-cvx-v1`, reference agreement job with an injected-perturbation negative test, `method_undefined` case, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL, card check. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; no reference-dataset constants in source; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.ENV.MCC.CVX.

## Open questions

- The 1e-12 m duality-gap and 1e-9 m agreement limits are [unverified] until the conic solver's tolerances in `KRN.SOLV.CONIC` are fixed.

## Children

_Leaf: none._
