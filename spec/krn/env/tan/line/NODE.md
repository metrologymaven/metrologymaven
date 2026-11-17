---
id: KRN.ENV.TAN.LINE
title: Tangential 2D line in three variants
level: 4
parent: KRN.ENV.TAN
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV.TAN-001, REQ-KRN.ENV.TAN-002, REQ-KRN.ENV.TAN-005]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.ENV.TAN.ROCK, KRN.LSQ.LIN, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit tan-line]
mcp: [fit_tan_line]
journal: [fit.tan_line@1]
updated: 2026-11-17
---

# KRN.ENV.TAN.LINE: Tangential 2D line in three variants

## Purpose

The line, in a given plane, that touches a profile from its material-free side: a secondary or tertiary datum edge, or the simulator of a straight edge in a section. It mirrors `KRN.ENV.TAN.PLANE` in two dimensions with the same three variants and no default.

## Scope

**In:** algorithm IDs `fit.line2d.tan_minimax@1.0.0`, `fit.line2d.tan_l2@1.0.0`, `fit.line2d.tan_l1@1.0.0`; points projected into a caller-given plane; direction by one angle parameter about the least-squares direction; one-sided constraints; certificate; rocking report; optional fixed direction; the command; spec cards. A 3D request returns `method_undefined` (table in `KRN.ENV.TAN.ROCK`).
**Out:** the variant table and rocking rule (`KRN.ENV.TAN.ROCK`); variant choice per dialect (`GDT`).

## Requirements

- **REQ-KRN.ENV.TAN.LINE-001** The module shall compute the tangential 2D line for the required variant and in-plane material side, implementing the `Fit` trait, and shall refuse a call without a variant.
- **REQ-KRN.ENV.TAN.LINE-002** Every projected point shall lie on the material side within `side_tol`, and the certificate shall carry the contact set and the maximum constraint violation.
- **REQ-KRN.ENV.TAN.LINE-003** The result shall include the `RockReport` built from all multi-start outcomes.
- **REQ-KRN.ENV.TAN.LINE-004** When the command is committed, the system shall journal one `fit.tan_line@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/tan-line-v1` (seeded edge profiles with convex and concave form, 20 to 20 000 points), when `fid fit tan-line --variant <v> --plane <id> --side <dir> --seed 1 --json` runs for each variant, then all points are on the material side within 1e-12 m and the objective matches the analytic optimum within 1e-9 m [unverified].
- **AC-2** Given a profile with two equal high points, then the minimax result's rocking report lists one two-point set and `rocking` is false; with a symmetric three-crest profile it lists two sets and `rocking` is true.
- **AC-3** (error path) Given no variant, then the fit fails with `InvalidParams{field: "variant"}` and journals nothing.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.tan_line@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), projection plane, `variant` (required), `material_side`, `side_tol`, `rock_tol`, seed, optional fixed direction. Result: `FittedFeature::Line2`, certificate, `RockReport`, residual statistics.
- **Algorithm:** as `KRN.ENV.TAN.PLANE` with one angle parameter; the minimax variant can use the convex hull edges as an exact candidate list, which also seeds the rocking report.
- **Errors:** `TooFewPoints`, `InvalidParams`, `NotConverged`, `MethodUndefined`, `Solver`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 20 000 points < 30 ms [unverified until measured].
- **Crate:** `fid-fit` (module `tan_line`).

## Test fixtures

`fixtures/fit/tan-line-v1`: seeded profile generator with analytic optima, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `tan_line` implementing the `Fit` trait for `fit.line2d.tan_minimax@1.0.0`, `fit.line2d.tan_l2@1.0.0` and `fit.line2d.tan_l1@1.0.0` on points projected into a given plane, with required `TanVariant` and `MaterialSide` from `tan_rock`; one direction-angle parameter about the least-squares direction; LP/QP steps via `fid_solv`; contacts and maximum violation in the certificate; multi-start outcomes into `tan_rock::rock_report`; 3D requests return `MethodUndefined`. Add the registry data row `fit.tan_line@1` (W2, dry-run R1, D0 per seed; CLI `fid fit tan-line`; MCP `fit_tan_line`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and spec cards. Tests: `fixtures/fit/tan-line-v1`, rocking profiles, missing-variant error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.ENV.TAN.LINE.

## Open questions

- None beyond the parent's constrained-L1 definition question.

## Children

_Leaf: none._
