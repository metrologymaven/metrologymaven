---
id: KRN.LSQ.PLAN2D.ELL
title: Least-squares orthogonal-distance ellipse
level: 4
parent: KRN.LSQ.PLAN2D
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ.PLAN2D-001, REQ-KRN.LSQ.PLAN2D-003, REQ-KRN.LSQ.PLAN2D-004, REQ-KRN.LSQ.PLAN2D-005, REQ-KRN.LSQ.PLAN2D-006]
covers: [GOM.CON.18, PW.CON.03]
depends_on: [KRN.LSQ.TRAIT, KRN.SOLV.NLLS, KRN.NUM.LA, KRN.NUM.FN, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid fit ls-ellipse]
mcp: [fit_ls_ellipse]
journal: [fit.ls_ellipse@1]
updated: 2026-11-17
---

# KRN.LSQ.PLAN2D.ELL: Least-squares orthogonal-distance ellipse

## Purpose

The ellipse in a caller-given plane that minimises the sum of squared orthogonal (geometric) distances, not the algebraic distance most quick fits use. It starts from an algebraic conic fit constrained to be an ellipse and refines with nonlinear least squares. Near-circles are refused because the orientation is undefined there.

## Scope

**In:** algorithm `fit.ellipse.ls@1.0.0`; parameters centre (2), semi-axes a >= b, orientation in [0, pi); ellipse-constrained algebraic start (direct least-squares conic fit with the ellipse constraint, solved as a small generalised eigenproblem with `fid_num::la`); orthogonal foot points by a bounded Newton on the foot-point equation; LM refinement; axis-ratio check; the command; a spec card.
**Out:** circles (`KRN.LSQ.RND`); slot and rectangle (`KRN.LSQ.PLAN2D.SLOT`, `KRN.LSQ.PLAN2D.RECT`); minimum-zone ellipse (`KRN.MZ.EXT`).

## Requirements

- **REQ-KRN.LSQ.PLAN2D.ELL-001** The module shall fit an ellipse in the given plane by minimising the sum of squared orthogonal in-plane distances, starting from an algebraic conic fit constrained to an ellipse.
- **REQ-KRN.LSQ.PLAN2D.ELL-002** Each orthogonal distance shall come from a foot point whose tangent residual is at most 1e-12 m [unverified], computed with a bounded iteration count.
- **REQ-KRN.LSQ.PLAN2D.ELL-003** If b/a is within the configured `ratio_tol` of 1, then the fit shall return `IllConditioned{measure: "axis_ratio", value}`.
- **REQ-KRN.LSQ.PLAN2D.ELL-004** The result shall report centre, semi-axes, orientation normalised to [0, pi), diagnostics and residual statistics.
- **REQ-KRN.LSQ.PLAN2D.ELL-005** When the command is committed, the system shall journal one `fit.ls_ellipse@1` event with algorithm ID, parameter digest, input digest, seed (null) and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/ell-v1` (seeded ellipses with b/a from 0.2 to 0.98, full and half arcs, 1 micrometre noise), when `fid fit ls-ellipse --plane <id> --json` runs, then all parameters are within 3 sigma of the propagated noise [unverified bound] and the sum of squared orthogonal residuals is not greater than that at the algebraic start.
- **AC-2** (error path) Given a circle (b/a = 1) with `ratio_tol = 1e-4`, then the fit returns `IllConditioned{measure: "axis_ratio"}` and journals nothing.
- **AC-3** Given AC-1 on x86-64, ARM64 and wasm32, then output digests are identical; replay in exact mode is `IDENTICAL`.
- **AC-4** Given the algorithm ID, then a spec card exists and the card check passes.

## Design

- **Registry command(s):** `fit.ls_ellipse@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), plane frame, `ratio_tol`, `NllsConfig`. Result: `FittedFeature::Ellipse { centre, a, b, angle }`, residual statistics, diagnostics.
- **Algorithm:** algebraic start following Fitzgibbon, Pilu and Fisher 1999 [unverified citation]; geometric refinement in the spirit of Ahn, Rauh and Warnecke 2001 [unverified citation]; foot points by Newton capped at 50 iterations; LM via `fid_solv::nlls`.
- **Errors:** `TooFewPoints`, `IllConditioned`, `NotConverged`, `Solver`.
- **Determinism:** D0.
- **Performance budget:** 50 000 points < 80 ms [unverified until measured].
- **Crate:** `fid-fit` (module `ls_ell`).

## Test fixtures

`fixtures/fit/ell-v1`: seeded generator, CC-BY-4.0.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `ls_ell` implementing the `Fit` trait for `fit.ellipse.ls@1.0.0` in a given plane: ellipse-constrained algebraic start via a small generalised eigenproblem with `fid_num::la`; orthogonal foot points by bounded Newton (max 50 iterations, tangent residual <= 1e-12 m); LM refinement via `fid_solv::nlls` over centre, semi-axes and orientation; normalise a >= b and orientation in [0, pi); `IllConditioned{measure: "axis_ratio"}` within `ratio_tol` of 1. Add the registry data row `fit.ls_ellipse@1` (W2, dry-run R1, D0; CLI `fid fit ls-ellipse`; MCP `fit_ls_ellipse`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/ell-v1` (incl. objective not worse than the start), near-circle error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL, card check. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.LSQ.PLAN2D.ELL.

## Open questions

- None.

## Children

_Leaf: none._
