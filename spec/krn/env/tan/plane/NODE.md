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
updated: 2026-11-20
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
- **REQ-KRN.ENV.TAN.PLANE-004** When a fixed normal is given, the module shall return the plane with that normal through the extreme point on the material-free side (the problem is then exact) and a rocking report holding only that plane (`rocking` false, no alternative candidate).
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
- **Partial build (`fid-fit` module `tan_plane`, not yet marked built).** Done: external minimax and constrained L1 with a free normal (sequential linearisation through `fid_solv::seq`, every result certified by `fid_solv::cert`, never an iterate), all three variants with a fixed normal (exact, REQ-004), the contact set and maximum violation (REQ-002), the `RockReport` of every certified start (REQ-003), the `Fit` trait, the `fit.tan_plane@1` data row, three spec cards, the canonical result bytes and output digest; tests in `crates/fid-fit/tests/tan_plane.rs` against the exact oracle `fixtures/fit/tan-plane-v1` (`make_tan_plane_fixtures.py`), with a cross-target output pin (AC-5 digest part). Remaining: (1) constrained L2 with a free normal (refused with `not_certifiable`, see below); (2) external minimax on the `tan-rock-v1` crowned plate (AC-2, fails closed, see below); (3) the registry, CLI, MCP and journal layer of `fit.tan_plane@1` (REQ-005, the CLI form of AC-1, "journals nothing" of AC-3, exact replay of AC-5), which needs `CORE.REG` to read the kernel's fit data rows, as for `KRN.LSQ.LIN`; (4) AC-1 at 100 000 points and the performance budget (the fixtures go to 1000 points; debug-mode tests on every target).
- **Applied at build (confirm in review).** *Optimality:* only a certified result is returned. The constraints are linear but the objective carries `1 / sqrt(1 + slope^2)`, so a certificate (a KKT vertex with positive multipliers) proves a **strict local minimum**, not a global one; the result says `strict_local` (`closed_form` for a fixed normal, which is the exact optimum for that normal). Multi-start and the rocking report cover the global question. *Formulation:* tangent plane `w = c + a u + b v` in a frame about the outward least-squares normal; minimax adds a far plane `w = e + a u + b v` and minimises `(c - e) / s`; L1 minimises `(N c + a U + b V - W) / s`, which is `N` times the distance from the centroid to the plane, so its optima are hull facets. *Form scale:* `w` is divided by the power of two nearest below its range, so thin plates (a 5 um form over 0.8 m) stay well scaled for the fixed conic tolerances; exact, and the certificate is exact for the scaled data. *Starts:* the least-squares plane shifted to the extreme point, a ring of four tilts, then seeded random tilts (`mstart`, stream 0); a seed is required only when random starts are used. *Results:* residuals are the depths below the plane (into the material, never below `-side_tol`); the full result keeps the outward normal (its sign is the side), the `Fit` summary canonicalises it. *Data scale:* the largest local coordinate magnitude, in the units of the constraints. *Canonical bytes:* `F64Bits` floats (`0x` + 16 hex), the certificate by its `sha256:` digest; no iterate or iteration count enters them. Which starts certified, and to which vertex, does enter, through the rocking report (candidates, `merged`, `excluded`) and the certificate's `local_optimum_only` flag; that is deterministic for a given seed but not a property of the data alone. *Contacts (REQ-002 deviation, review of #344):* the contact set is the tangent-side rows of the certified active set (fixed normal: the points whose computed `t_i` equals the maximum exactly), not the points within `side_tol` of the float plane; a point exactly tied with the active set but outside it is not listed. `side_tol` only bounds the accepted violation, and 0 is allowed. *REQ-004:* the "empty rocking set" is a report holding only the fixed-normal plane (`rocking` false), because `tan_rock::rock_report` refuses an empty list; the requirement text now says so. *Fixed-normal rounding:* the computed depths are non-negative and the extreme point's is exactly 0, so `max_violation` is 0; against the exact depths of the input points each depth carries the centring and dot-product rounding, to first order at most `8 u (||x_i - c||_1 + ||x_m - c||_1)` (`u = 2^-53`, `m` the extreme point) [unverified constant], which also bounds the true violation. *`Fit` summary:* residuals signed as the contract has them (positive on the canonical normal's side: `-depth` when the canonical normal is the outward one, `+depth` when the sign rule flipped it); `certificate.gap` is the proven value bound times the form scale and the scale. *Errors:* three variants added to the contract's `FitError` (`SeqFailed`, code `solver`; `NotCertifiable`, code `not_certifiable`; `SideViolation`, code `side_violation`); `KRN.LSQ.TRAIT` to confirm.
- **For routing to other leaves (not changed here):**
  - `KRN.SOLV.CERT`: the certificate is for vertices only (`expected_active >= n_params`). A constrained-L2 optimum generally touches fewer points than the plane has parameters (it is a smooth stationary point on the material side), so it cannot be certified and is refused. Needs a certificate for a KKT point with fewer active rows than parameters (reduced-gradient and second-order proof), or a ruling that the free-normal L2 variant is out of scope.
  - `KRN.SOLV.CERT.AS`: `MAX_CANDIDATES = 256` caps the candidate bases. On the `tan-rock-v1` crowned plate, external minimax converges to the horizontal plane through the apex (1.5 h, the AC-2 answer), but its 20 coplanar far-side points plus the apex put 21 rows in the small-slack pool, so `C(21, 4) = 5985` bases exceed the cap (`OverCap`) and it fails closed. Measured faces with exact ties (quantised coordinates) will hit the same cap. The #344 reviewer recommends the fix in `KRN.SOLV.CERT.AS`, not here: for `is_linear()` problems, a lexicographic Carathéodory reduction of the tied pool to one basis (routed separately by the coordinator).
  - `KRN.SOLV.CONIC.WRAP` / `KRN.SOLV.CERT.AS`: a facet whose centroid foot point lies near one of its vertices has nearly zero multipliers on its other contacts, and the fixed `1e-10` conic tolerances may leave their slacks unresolved (`ActiveSetUndetermined`). The form scale made the crowned plate's L1 facets certify (multiplier ratio about 1e-7); more extreme cases still fail closed.
  - `CORE.BUS.PIPE.ERR` (with the deferred REQ-005 command layer): the bus error catalogue v3 has no fit-specific reasons. Proposed mapping of the `FitError` codes this leaf returns: caller input (`invalid_params`, `non_finite`, `subnormal`, `non_finite_normal`, `non_unit_normal`, `bad_length`, `length_mismatch`, `invalid_direction`) to `validation/schema`, or `validation/literal_out_of_range` for an out-of-range value; `too_large` to `validation/input_limit`; data that cannot carry the fit (`too_few_points`, `collinear`, `coplanar`, `ill_conditioned`, `method_undefined`, `not_certifiable`) and fits that end without a certified value (`solver`, `not_converged`, `side_violation`, `ambiguous_candidate`) to `precondition/false`; `numeric` to `internal/handler_failed`. `precondition/false` loses the distinction between bad data and an uncertifiable result, so finer reasons (for example `precondition/not_certified`) would need catalogue v4. The fit code and its fields go in the error detail.
  - `CORE.REG` / `KRN.LSQ.TRAIT`: one command `fit.tan_plane@1` serves three algorithm IDs, but the contract's data row holds one algorithm, so the row names the family `fit.plane.tan@1.0.0` and the result's `algorithm` field names the variant. The L2 objective is in square metres, but the row's quantity catalogue has no area kind, so `objective` is declared as a length.

## Children

_Leaf: none._
