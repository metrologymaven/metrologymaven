---
id: KRN.ENV.TAN.CYL
title: Tangential cylinder, inner and outer
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
depends_on: [KRN.ENV.TAN.ROCK, KRN.LSQ.AXIS.CYL, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit tan-cylinder]
mcp: [fit_tan_cylinder]
journal: [fit.tan_cylinder@1]
updated: 2026-11-17
---

# KRN.ENV.TAN.CYL: Tangential cylinder, inner and outer

## Purpose

The cylinder that touches a cylindrical surface from its material-free side: inside a bore (inner, all points outside the cylinder) or around a shaft (outer, all points inside), in the three variants. Combined with an optional fixed axis direction it simulates a datum cylinder that must stay perpendicular to a primary datum plane. It is non-convex in the axis, so it uses sequential linearisation and multi-start, and reports rocking candidates.

## Scope

**In:** algorithm IDs `fit.cylinder.tan_minimax@1.0.0`, `fit.cylinder.tan_l2@1.0.0`, `fit.cylinder.tan_l1@1.0.0`; `side: inner | outer` (required); axis by two tangent-plane parameters; free radius, or fixed radius (a fixed-size simulator); optional fixed axis direction; one-sided constraints; certificate; rocking report; the command; spec cards.
**Out:** maximum inscribed and minimum circumscribed cylinders without a variant objective (`KRN.ENV.MIC.CYL`, `KRN.ENV.MCC.CYL`); variant table and rocking rule (`KRN.ENV.TAN.ROCK`); datum mobility and virtual condition (`GDT`).

## Requirements

- **REQ-KRN.ENV.TAN.CYL-001** The module shall compute the tangential cylinder for the required variant and side, implementing the `Fit` trait, and shall refuse a call without a variant or side.
- **REQ-KRN.ENV.TAN.CYL-002** Every point shall lie on the material side within `side_tol`, and the certificate shall carry the contact set and the maximum constraint violation.
- **REQ-KRN.ENV.TAN.CYL-003** The result shall carry the multi-start agreement flag, `local_optimum_only` when global optimality is not proven, and the `RockReport`.
- **REQ-KRN.ENV.TAN.CYL-004** When a fixed radius is given and no tangential cylinder of that radius exists on the material side (for example a simulator larger than the bore), then the fit shall return `Undefined{reason: no_feasible_tangent}`.
- **REQ-KRN.ENV.TAN.CYL-005** When the command is committed, the system shall journal one `fit.tan_cylinder@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/tan-cyl-v1` (seeded bores and shafts with lobing, 1 000 to 100 000 points), when `fid fit tan-cylinder --variant <v> --side <s> --seed 2 --json` runs for each combination, then all points are on the material side within 1e-12 m and the objective matches the generator's analytic optimum within 1e-8 m [unverified].
- **AC-2** Given a bore with a fixed axis direction perpendicular to a datum plane, then the axis equals that direction bit for bit and the result agrees with the 2D tangential circle of the projected points.
- **AC-3** (error path) Given a fixed radius 0.1 mm larger than the bore's inscribed radius with `side: inner`, then the fit returns `Undefined{reason: no_feasible_tangent}` and journals nothing.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.tan_cylinder@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), `variant` and `side` (required), optional fixed radius (m) and fixed axis direction, `side_tol`, `rock_tol`, seed, `SeqConfig`. Result: `FittedFeature::Cylinder`, certificate, multi-start summary, `RockReport`, residual statistics.
- **Algorithm:** start at the least-squares cylinder, moved to the material-free side; linearise radial distances in the axis parameters, axis point and radius; LP or QP steps via `fid_solv`; multi-start over axis perturbations; certify.
- **Errors:** `TooFewPoints`, `IllConditioned`, `InvalidParams`, `Undefined`, `NotConverged`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 50 000 points, 8 starts < 600 ms [unverified until measured].
- **Crate:** `fid-fit` (module `tan_cyl`).

## Test fixtures

`fixtures/fit/tan-cyl-v1`: seeded generator with analytic optima per variant and side, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `tan_cyl` implementing the `Fit` trait for `fit.cylinder.tan_minimax@1.0.0`, `fit.cylinder.tan_l2@1.0.0` and `fit.cylinder.tan_l1@1.0.0` with required `TanVariant` and `side: Inner | Outer`; start from the least-squares cylinder on the material-free side; axis by two tangent-plane parameters; optional fixed radius and fixed axis direction; LP/QP steps via `fid_solv::seq`; multi-start via `fid_solv::mstart` with outcomes into `tan_rock::rock_report`; certificate via `fid_solv::cert`; `Undefined{reason: no_feasible_tangent}` when a fixed radius cannot touch on the material side. Add the registry data row `fit.tan_cylinder@1` (W2, dry-run R1, D0 per seed; CLI `fid fit tan-cylinder`; MCP `fit_tan_cylinder`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: `fixtures/fit/tan-cyl-v1` all six variant-side combinations, fixed-direction case, infeasible fixed radius, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.ENV.TAN.CYL.

## Open questions

- None beyond the parent's constrained-L1 definition question.

## Children

_Leaf: none._
