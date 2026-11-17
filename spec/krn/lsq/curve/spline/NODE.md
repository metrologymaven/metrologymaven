---
id: KRN.LSQ.CURVE.SPLINE
title: Least-squares smoothing spline curves
level: 4
parent: KRN.LSQ.CURVE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.CURVE-003, REQ-KRN.LSQ.CURVE-004, REQ-KRN.LSQ.CURVE-005]
covers: [PW.CON.03]
depends_on: [KRN.LSQ.TRAIT, KRN.SOLV.NLLS, KRN.NUM.LA, GEO.NURBS.EVAL, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit ls-spline]
mcp: [fit_ls_spline]
journal: [fit.ls_spline@1]
updated: 2026-11-17
---

# KRN.LSQ.CURVE.SPLINE: Least-squares smoothing spline curves

## Purpose

Fits a planar or 3D B-spline curve through ordered measured points with an explicit degree, knot placement rule and smoothing parameter, all recorded in the result, so that freeform contours (blade sections, seals, styling lines) become a smooth curve element. Nothing is hidden: there is no implicit default smoothing.

## Scope

**In:** algorithm `fit.spline.ls@1.0.0`; required parameters degree (1 to 5), knot rule (`uniform{count}` or `averaging{count}` over chord-length parameters, or `explicit{knots}`), smoothing weight lambda >= 0 on the integrated squared second derivative; chord-length parameterisation followed by a fixed number of parameter-correction passes (foot points via `GEO.NURBS.EVAL` derivatives); the linear penalised least-squares solve; ordering check; optional projection plane; the command; a spec card.
**Out:** B-spline basis and derivative evaluation (`GEO.NURBS.EVAL`); NURBS surfaces and reverse engineering (out of scope); polylines (`KRN.LSQ.CURVE.POLY`).

## Requirements

- **REQ-KRN.LSQ.CURVE.SPLINE-001** The module shall fit a B-spline curve of the given degree with knots from the given rule, minimising the sum of squared distances to the parameterised points plus lambda times the integrated squared second derivative, and shall record degree, knot rule, knots, lambda and correction-pass count in the result.
- **REQ-KRN.LSQ.CURVE.SPLINE-002** The fit shall refuse a call that omits degree, knot rule or lambda with `InvalidParams{field}`.
- **REQ-KRN.LSQ.CURVE.SPLINE-003** After the last parameter-correction pass, the reported residuals shall be orthogonal distances to the fitted curve computed from foot points with tangent residual at most 1e-12 m [unverified].
- **REQ-KRN.LSQ.CURVE.SPLINE-004** If the points are not ordered along the curve, or a knot span contains fewer points than degree + 1 (Schoenberg-Whitney condition not met), then the fit shall return `NotOrdered{index}` or `UnderSampled{span}`.
- **REQ-KRN.LSQ.CURVE.SPLINE-005** When the command is committed, the system shall journal one `fit.ls_spline@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/spline-v1` (points sampled from known cubic B-splines with 1 micrometre noise), when `fid fit ls-spline --degree 3 --knots averaging:12 --lambda 0 --json` runs with the generating knot count, then the maximum orthogonal residual is below 5 micrometres and the control points are within 3 sigma of the propagated noise [unverified bound].
- **AC-2** Given lambda increased from 0 to 1e-6, then the integrated squared second derivative of the result decreases monotonically over the fixture sweep.
- **AC-3** (error path) Given no `--lambda`, the fit returns `InvalidParams{field: "lambda"}`; given a knot span with 2 points for degree 3, it returns `UnderSampled{span}`; neither journals anything.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.ls_spline@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`, order as stored), `degree`, `knot_rule`, `lambda` (m^-2 scaled; recorded with unit), `correction_passes` (default none: required), optional projection plane. Result: `FittedFeature::Spline { degree, knots, control_points }`, per-point residuals, diagnostics.
- **Algorithm:** chord-length parameters; knot vector by rule; banded normal equations with the second-derivative penalty (Gram matrix by fixed Gauss-Legendre quadrature); solve by banded Cholesky in `fid_num::la`; parameter correction by Newton foot points using `GEO.NURBS.EVAL` derivatives. Reference: Dierckx 1993, "Curve and Surface Fitting with Splines" [unverified edition].
- **Errors:** `InvalidParams`, `TooFewPoints`, `NotOrdered`, `UnderSampled`, `IllConditioned`, `Num`.
- **Determinism:** D0.
- **Performance budget:** 100 000 points, 200 control points, 3 passes < 400 ms [unverified until measured].
- **Crate:** `fid-fit` (module `ls_spline`), using `fid-geo-nurbs` (kernel graph, KNP-1).

## Test fixtures

`fixtures/fit/spline-v1`: seeded generator from known B-splines, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num`, `fid-solv` and `fid-geo-nurbs` for basis and derivative evaluation), add module `ls_spline` implementing the `Fit` trait for `fit.spline.ls@1.0.0`: required degree (1..=5), knot rule (`uniform`, `averaging`, `explicit`), lambda >= 0 and correction-pass count (missing -> `InvalidParams{field}`); chord-length parameters; penalised least squares with a second-derivative Gram matrix by fixed Gauss-Legendre quadrature; banded Cholesky via `fid_num::la`; parameter correction by Newton foot points; `NotOrdered{index}` and `UnderSampled{span}` (Schoenberg-Whitney) errors; record all parameters in the result. Add the registry data row `fit.ls_spline@1` (W2, dry-run R1, D0; CLI `fid fit ls-spline`; MCP `fit_ls_spline`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/spline-v1`, lambda monotonicity, missing-parameter and under-sampled errors, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.CURVE.SPLINE.

## Open questions

- `fid-geo-nurbs` must stay inside the KNP-1 lint and the kernel dependency allowlist (`KRN.NUM.LINT`) for this dependency to be legal; confirm with GEO.

## Children

_Leaf: none._
