---
id: KRN.ENV.TAN.PLANE
title: Tangential plane in three variants
level: 4
parent: KRN.ENV.TAN
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV.TAN-001, REQ-KRN.ENV.TAN-002, REQ-KRN.ENV.TAN-005]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.ENV.TAN.ROCK, KRN.LSQ.LIN, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit tan-plane]
mcp: [fit_tan_plane]
journal: [fit.tan_plane@1]
updated: 2026-11-19
---

# KRN.ENV.TAN.PLANE: Tangential plane in three variants

## Purpose

The plane that touches a surface from its material-free side, as a surface plate touches a datum face. It is the main datum-simulator case. Three objectives are offered and none is a default, because the planar datum default depends on the standard and its edition, which `GDT` decides: external minimax (the tangent plane with the smallest maximum distance to the points), constrained L2 (smallest sum of squared distances with every point on the material side) and constrained L1 (smallest sum of distances with every point on the material side).

## Scope

**In:** algorithm IDs `fit.plane.tan_minimax@1.0.0`, `fit.plane.tan_l2@1.0.0`, `fit.plane.tan_l1@1.0.0`; normal parameterised by two tangent-plane parameters about the least-squares normal; LP (minimax, L1) or QP (L2) steps through `KRN.SOLV.CONIC` and `KRN.SOLV.SEQ`; one-sided constraints with recorded `side_tol`; contact set and constraint residuals in the certificate; multi-start candidates passed to `KRN.ENV.TAN.ROCK`; optional fixed normal (orientation to a higher-precedence datum); the command; spec cards.
**Out:** the variant table, rocking report and tie-break (`KRN.ENV.TAN.ROCK`); choice of variant per dialect and edition (`GDT`).

## Requirements

- **REQ-KRN.ENV.TAN.PLANE-001** The module shall compute the tangential plane for the required variant and material side, implementing the `Fit` trait, and shall refuse a call without a variant with `InvalidParams{field: "variant"}`.
- **REQ-KRN.ENV.TAN.PLANE-002** Every point shall lie on the material side within the recorded `side_tol`, and the certificate shall carry the contact set (points within `side_tol` of the plane) and the maximum constraint violation.
- **REQ-KRN.ENV.TAN.PLANE-003** The result shall include the `RockReport` from `KRN.ENV.TAN.ROCK` built from all multi-start outcomes.
- **REQ-KRN.ENV.TAN.PLANE-004** When a fixed normal is given, the module shall return the plane with that normal through the extreme point on the material-free side (the problem is then exact) and an empty rocking set.
- **REQ-KRN.ENV.TAN.PLANE-005** When the command is committed, the system shall journal one `fit.tan_plane@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/tan-plane-v1` (seeded plates with known convex form, 100 to 100 000 points), when `fid fit tan-plane --variant <v> --side <dir> --seed 1 --json` runs for each variant, then all points are on the material side within 1e-12 m and the objective matches the generator's analytic optimum within 1e-9 m [unverified].
- **AC-2** Given the crowned plate of `fixtures/fit/tan-rock-v1`, then with `constrained_l1` the `RockReport` lists the 4 facet sets and `rocking` is true; with `external_minimax` it lists one candidate {0}, objective 1.5h, `rocking` false.
- **AC-3** (error path) Given a call without `--variant`, then it fails with `InvalidParams{field: "variant"}` and journals nothing.
- **AC-4** Given a fixed normal, then the plane passes through the extreme point and `rocking` is false.
- **AC-5** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.tan_plane@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), `variant` (required), `material_side`, `side_tol` (m), `rock_tol`, seed, optional fixed normal. Result: `FittedFeature::Plane`, certificate, `RockReport`, residual statistics.
- **Algorithm:** start at the least-squares plane shifted to the extreme point; linearise in the two normal parameters and offset; LP or QP step; accept by the `KRN.SOLV.SEQ` merit rule; multi-start over normal perturbations. Background: published work on datum plane fitting by constrained least squares and L1 (Shakarji and Srinivasan, around 2012 to 2015 [unverified citation]).
- **Errors:** `TooFewPoints`, `Collinear`, `InvalidParams`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 100 000 points, 8 starts, any variant < 300 ms [unverified until measured].
- **Crate:** `fid-fit` (module `tan_plane`).

## Test fixtures

`fixtures/fit/tan-plane-v1`: seeded generator with analytic optima per variant, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `tan_plane` implementing the `Fit` trait for `fit.plane.tan_minimax@1.0.0`, `fit.plane.tan_l2@1.0.0`, `fit.plane.tan_l1@1.0.0` with a required `TanVariant` and `MaterialSide` from `tan_rock`. Parameterise the normal by two tangent-plane parameters about the least-squares normal; LP (minimax, L1 with slack variables) or QP (L2) steps via `fid_solv::conic` and `fid_solv::seq`; all points on the material side within `side_tol`; certificate with contacts and maximum violation via `fid_solv::cert`; multi-start via `fid_solv::mstart`, outcomes passed to `tan_rock::rock_report`; fixed-normal shortcut. Add the registry data row `fit.tan_plane@1` (W2, dry-run R1, D0 per seed; CLI `fid fit tan-plane`; MCP `fit_tan_plane`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: `fixtures/fit/tan-plane-v1` per variant, crowned-plate rocking, missing-variant error, fixed-normal case, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.ENV.TAN.PLANE.

## Open questions

- The exact constrained-L1 objective must follow published work, not standards text [unverified]; confirm the definition with a `GDT` maintainer before release.

## Children

_Leaf: none._
