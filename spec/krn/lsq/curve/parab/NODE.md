---
id: KRN.LSQ.CURVE.PARAB
title: Least-squares paraboloid of revolution
level: 4
parent: KRN.LSQ.CURVE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.CURVE-001, REQ-KRN.LSQ.CURVE-004, REQ-KRN.LSQ.CURVE-005]
covers: [GOM.CON.18, SA.REL.01]
depends_on: [KRN.LSQ.TRAIT, KRN.SOLV.NLLS, KRN.SOLV.MSTART, KRN.NUM.LA, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit ls-paraboloid]
mcp: [fit_ls_paraboloid]
journal: [fit.ls_paraboloid@1]
updated: 2026-11-17
---

# KRN.LSQ.CURVE.PARAB: Least-squares paraboloid of revolution

## Purpose

The orthogonal-distance least-squares paraboloid of revolution, used for antenna reflectors and optical mirrors (a SpatialAnalyzer staple). Its parameters are the vertex, the axis and the focal length. The focal length is poorly determined on shallow or small-aperture data, so the fit reports its conditioning and refuses ill-determined cases.

## Scope

**In:** algorithm `fit.paraboloid.ls@1.0.0`; parameters vertex (3), axis (two tangent-plane parameters), focal length f; algebraic start (quadric fit constrained to a paraboloid of revolution) plus a deterministic multi-start over axis tilts; orthogonal distance computed by solving the foot-point cubic in the meridian plane with a fixed-iteration safeguarded Newton; fixed focal length and fixed axis direction constraints; the command; a spec card.
**Out:** off-axis paraboloid segments with an explicit off-axis offset (a later version); general quadrics (not in scope).

## Requirements

- **REQ-KRN.LSQ.CURVE.PARAB-001** The module shall fit a paraboloid of revolution by minimising the sum of squared orthogonal distances, with vertex, axis (two tangent-plane parameters) and focal length as parameters.
- **REQ-KRN.LSQ.CURVE.PARAB-002** The orthogonal distance of each point shall be computed from the real root of the meridian-plane foot-point equation that minimises the distance, with a bounded iteration count, and the per-point foot residual shall be at most 1e-12 m [unverified].
- **REQ-KRN.LSQ.CURVE.PARAB-003** If the focal length's relative standard error from the Jacobian exceeds the configured `focal_cond_limit`, then the fit shall return `IllConditioned{measure: "focal_length"}` with the value.
- **REQ-KRN.LSQ.CURVE.PARAB-004** The fit shall run a deterministic multi-start through `KRN.SOLV.MSTART` and set `local_optimum_only` when starts disagree.
- **REQ-KRN.LSQ.CURVE.PARAB-005** When the command is committed, the system shall journal one `fit.ls_paraboloid@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/parab-v1` (seeded reflectors, f/D from 0.25 to 1.5, aperture 0.2 to 5 m, 10 micrometre noise, far from origin), when `fid fit ls-paraboloid --seed 9 --json` runs, then vertex, axis and focal length are within 3 sigma of the generator's propagated noise [unverified bound].
- **AC-2** Given open reference-suite paraboloid cases from the frozen RDL release (`UNC.RDL.FREEZE`), then parameters agree within the suite's tolerance [unverified].
- **AC-3** (error path) Given a 1 percent aperture patch at the vertex with f/D 5, then the fit returns `IllConditioned{measure: "focal_length"}` and journals nothing.
- **AC-4** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.ls_paraboloid@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, optional fixed focal length (m) or axis direction, `focal_cond_limit`, `NllsConfig`. Result: `FittedFeature::Paraboloid { vertex, axis, focal }`, residual statistics, diagnostics, multi-start summary.
- **Algorithm:** algebraic start by a linear least-squares fit of the constrained quadric (`fid_num::la`); Gauss-Newton or LM through `fid_solv::nlls` with the implicit-function Jacobian of the orthogonal distance; foot points by safeguarded Newton capped at 50 iterations.
- **Errors:** `TooFewPoints`, `IllConditioned`, `NotConverged`, `Solver`, `MultiStart`.
- **Determinism:** D0 for a given seed.
- **Performance budget:** 1 M points, 4 starts < 3 s [unverified until measured].
- **Crate:** `fid-fit` (module `ls_parab`).

## Test fixtures

`fixtures/fit/parab-v1`: seeded generator, CC-BY-4.0; open reference cases from `UNC.RDL.FREEZE` at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_parab` implementing the `Fit` trait for `fit.paraboloid.ls@1.0.0`: parameters vertex, axis (two tangent-plane parameters), focal length; algebraic constrained-quadric start; orthogonal distances by solving the meridian-plane foot-point equation with safeguarded Newton (max 50 iterations); implicit-function Jacobian; `fid_solv::nlls`; multi-start via `fid_solv::mstart` with `local_optimum_only`; `IllConditioned{measure: "focal_length"}` above `focal_cond_limit`; optional fixed focal length or axis direction. Add the registry data row `fit.ls_paraboloid@1` (W2, dry-run R1, D0 per seed; CLI `fid fit ls-paraboloid`; MCP `fit_ls_paraboloid`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/parab-v1`, open reference cases from the frozen RDL release, small-aperture error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.CURVE.PARAB.

## Open questions

- Phase P1b; SpatialAnalyzer users need paraboloids in P3 at the latest [unverified].

## Children

_Leaf: none._
