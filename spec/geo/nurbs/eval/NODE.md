---
id: GEO.NURBS.EVAL
title: NURBS evaluation, derivatives, normals and curvature
level: 3
parent: GEO.NURBS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-nurbs]
satisfies: [REQ-GEO.NURBS-001, REQ-GEO.NURBS-004, REQ-GEO.NURBS-006]
covers: []
depends_on: [GEO.NURBS.DATA, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.NURBS.EVAL: NURBS evaluation, derivatives, normals and curvature

## Purpose

Evaluates B-spline and NURBS curves and surfaces at a parameter: position, first and second derivatives, unit normal, principal curvatures and directions, with explicit flags at degenerate points. It defines the `Surface` and `Curve` traits that ANALYTIC also implements, so downstream code is generic over both.

## Scope

**In:**
- Knot span search and basis function derivatives
- Curve: position, first and second derivatives, unit tangent, curvature
- Surface: position, S_u, S_v, S_uu, S_uv, S_vv, unit normal, Gaussian and mean curvature, principal curvatures and directions
- Rational evaluation by the quotient rule on homogeneous coordinates
- `Regularity` flag: regular, degenerate (normal length below threshold), outside domain
- The `Curve` and `Surface` traits

**Out:**
- Validation of the data (owned by `GEO.NURBS.DATA`)
- Point inversion (owned by `GEO.NURBS.INVERT`)
- Closed-form analytic surfaces (owned by `GEO.NURBS.ANALYTIC`)

## Requirements

- **REQ-GEO.NURBS.EVAL-001** The evaluator shall return position and all partial derivatives up to second order for any parameter inside the domain, for degrees 1 to 25.
- **REQ-GEO.NURBS.EVAL-002** If a parameter lies outside the domain by more than 1e-14 of the domain width, then evaluation shall fail with `OutsideDomain`; within that margin it shall clamp to the domain.
- **REQ-GEO.NURBS.EVAL-003** When ‖S_u × S_v‖ is below 1e-14 times ‖S_u‖·‖S_v‖ or either is zero, the evaluator shall return the derivatives with `Regularity::Degenerate` and no normal, never a NaN normal.
- **REQ-GEO.NURBS.EVAL-004** At regular points the evaluator shall return the unit normal and principal curvatures computed from the first and second fundamental forms.
- **REQ-GEO.NURBS.EVAL-005** Every sum shall use a fixed order and every square root `fid-num`, with no fused multiply-add, so results are bit-identical on x86-64, ARM64 and wasm32.
- **REQ-GEO.NURBS.EVAL-006** At a knot with multiplicity, the evaluator shall use the span to the right of the knot except at the domain end, where it shall use the last non-empty span.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/nurbs/ref_set_v1` (from GEO.NURBS.REF), when positions are evaluated at the listed parameters, then every position is within 1e-12 m of the reference and derivatives are within 1e-9 relative.
- **AC-2** Given the NURBS sphere in `ref_set_v1` at its poles, when evaluated, then `Regularity::Degenerate` is returned with finite derivatives and no normal.
- **AC-3** Given a cylinder of radius 10 mm encoded as NURBS, when principal curvatures are evaluated at 100 parameters, then they are 100 m⁻¹ and 0 within 1e-9 relative.
- **AC-4** Given `ref_set_v1`, when evaluated on x86-64, ARM64 and wasm32, then the result digest is identical.
- **AC-5** (error path) Given u = domain end + 1e-6 × width, when evaluated, then `OutsideDomain` is returned.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `trait Curve { fn eval(&self, t, order) -> CurveDerivs; fn domain(&self) }`, `trait Surface { fn eval(&self, u, v, order) -> SurfaceDerivs; fn normal(...); fn curvature(...); fn domain(&self) }`; `SurfaceDerivs { p, su, sv, suu, suv, svv, regularity }`; `Curvature { k1, k2, d1, d2, gaussian, mean }`.
- **Algorithm:** span search by binary search with the rule of REQ-006; basis derivatives and surface derivatives per Piegl and Tiller, *The NURBS Book*, 2nd ed., algorithms A2.1, A2.3, A3.6 and A4.4 (cited, not copied); principal curvatures from the shape operator eigenproblem in closed form.
- **Errors:** `OutsideDomain`, `OrderTooHigh`; also `NotANumber` (NaN parameter, KNP-1), `NonFinite` (an intermediate overflowed; no NaN or infinity is ever returned) and `Inconsistent` (unreachable for validated data; returned instead of indexing out of bounds). Invalid knot vectors, degrees, control nets and weights are refused earlier, by the `GEO.NURBS.DATA` constructors (`NurbsError`), so they cannot reach the evaluator.
- **Outside the domain** is the error `OutsideDomain`, not a `Regularity` value: `Regularity` is `Regular` or `Degenerate`.
- **Pinned order of operations** (built in `fid-geo-nurbs` `basis.rs` and `eval.rs`; mirrored bit for bit by `fixtures/geo/nurbs/eval/make_eval_fixtures.py`). IEEE 754 binary64, round to nearest even, every `+ - * /` a separate rounded operation (no FMA), `sqrt` = `fid_num::f::sqrt` (correctly rounded); `x op y op z` is left to right unless bracketed.
  1. *Domain:* NaN is `NotANumber`; ±inf is `OutsideDomain`; `margin = 1e-14 * (hi - lo)`; `t < lo`: `lo - t <= margin` gives `lo`, else `OutsideDomain`; `t > hi` likewise with `t - hi`. The parameter actually used is returned with the result.
  2. *Span* (n control points, degree p): if `t >= U[n]`, `s = n - 1`, then `s -= 1` while `s > p` and `U[s] == U[s+1]`; otherwise binary search `lo = p`, `hi = n`, while `hi - lo > 1`: `mid = lo + (hi - lo) / 2`, `t < U[mid]` sets `hi = mid`, else `lo = mid`; `s = lo`.
  3. *Basis (Cox–de Boor):* `N[0][0] = 1`; for `k = 1..=p`: `saved = 0`; for `r = 0..k`: `temp = N[k-1][r] / (U[s+r+1] - U[s+r+1-k])`, `N[k][r] = saved + (U[s+r+1] - t) * temp`, `saved = (t - U[s+r+1-k]) * temp`; then `N[k][k] = saved`.
  4. *Basis derivatives* (degree-lowering recurrence): `D(prev, k)[j] = k * (a - b)`, `a = prev[j-1] / (U[s+j] - U[s+j-k])` for `j >= 1` else `0.0`, `b = prev[j] / (U[s+j+1] - U[s+j+1-k])` for `j < k` else `0.0`. First derivative `D(N[p-1], p)`; second `D(D(N[p-2], p-1), p)`, with `D(N[p-2], p-1)` replaced by `[0.0]` when `p = 1`.
  5. *Homogeneous points:* `[x*w, y*w, z*w, w]` with weights, `[x, y, z, 1]` without. *Curve:* `A_d = sum over j = 0..=p ascending of N^(d)[j] * Pw[s-p+j]`, each component `acc = acc + b * pw`, from `0.0`. *Surface:* `T_l[i] = sum over j = 0..=q ascending of Nv^(l)[j] * Pw[(su-p+i) * n_v + (sv-q+j)]`, then `A_kl = sum over i = 0..=p ascending of Nu^(k)[i] * T_l[i]`, for `k + l <= order`.
  6. *Quotient rule* (only when weights are present, even if all equal; without weights the result is `A` itself): `C0 = A0 / w0`; `C1 = (A1 - w1*C0) / w0`; `C2 = ((A2 - (2*w1)*C1) - w2*C0) / w0`. Surfaces: `S10 = (A10 - w10*S00) / w00` (and `S01`), `S20 = ((A20 - (2*w10)*S10) - w20*S00) / w00` (and `S02`), `S11 = (((A11 - w10*S01) - w01*S10) - w11*S00) / w00`.
  7. *Vectors:* `dot(a, b) = (a0*b0 + a1*b1) + a2*b2`; `cross(a, b) = [a1*b2 - a2*b1, a2*b0 - a0*b2, a0*b1 - a1*b0]`.
  8. *Curve regularity and geometry:* `cl = sqrt(dot(C', C'))`, `den = (cl*cl)*cl`; degenerate iff `den == 0`; tangent `C' / cl` (per component); curvature `sqrt(dot(x, x)) / den`, `x = cross(C', C'')`.
  9. *Surface regularity* (REQ-003, squared form): `a2 = dot(Su, Su)`, `b2 = dot(Sv, Sv)`, `c2 = dot(Su x Sv, Su x Sv)`; degenerate iff `a2 == 0` or `b2 == 0` or `c2 == 0` or `c2 < 1e-28 * (a2 * b2)`.
  10. *Normal and curvatures* (shape operator in the orthonormal tangent frame, so the eigenvalues have no cancellation at umbilics): `n = cr / sqrt(c2)`; `a = sqrt(a2)`; `e1 = Su / a`; `e2 = cross(n, e1)`; `b = dot(Sv, e1)`, `c = dot(Sv, e2)`; `L, M, N = dot(Suu, n), dot(Suv, n), dot(Svv, n)`; `t = b / a`; `s11 = (L / a) / a`; `s12 = ((M - t*L) / a) / c`; `s22 = ((N - (2*t)*M) + (t*t)*L) / (c*c)`; `mean = 0.5 * (s11 + s22)`; `h = 0.5 * (s11 - s22)`; `r = sqrt(h*h + s12*s12)`; `k1 = mean + r`, `k2 = mean - r`; `gaussian = k1 * k2`. Curvatures are signed against `n` (a sphere with outward `n` has `k1 = k2 = -1/r`), as in `ref_set_v1`.
  11. *Principal directions:* none (umbilic) when `r <= 1e-8 * |mean|`. Otherwise, with `A = s11 - k1`, `C = s22 - k1`: `(x, y) = (-s12, A)` if `|A| >= |C|`, else `(C, -s12)`; `d = x*e1 + y*e2` per component; `d1 = d / sqrt(dot(d, d))` (none if that length is 0); `d2 = cross(n, d1)`.
- **Determinism:** D0.
- **Allocation and bounds:** no heap allocation; fixed-size stack tables of `MAX_DEGREE + 1 = 26` entries per order; every loop is bounded by the degree.
- **Performance budget:** bicubic surface position plus second derivatives under 300 ns per point [unverified].
- **Crate:** `fid-geo-nurbs` (kernel graph, KNP-1 applies).

## Test fixtures

- `fixtures/geo/nurbs/ref_set_v1` (owned by GEO.NURBS.REF); the NURBS sphere and cylinder are part of it. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-nurbs` (pure Rust, f64, no I/O, KNP-1), add module `eval` with traits `Curve` and `Surface` and implementations for `NurbsCurve` and `NurbsSurface`: span search (right-continuous, last non-empty span at the domain end), basis-function derivatives, surface derivatives to second order, rational derivatives via the quotient rule (algorithms after Piegl and Tiller A2.1, A2.3, A3.6, A4.4; write your own code), unit normal and principal curvatures from the fundamental forms. Clamp parameters within 1e-14 of the domain width, else `OutsideDomain`. Degenerate points (‖S_u×S_v‖ < 1e-14·‖S_u‖‖S_v‖) return `Regularity::Degenerate` with no normal, never NaN. Fixed-order sums, sqrt via `fid-num`, no `mul_add`. Acceptance: AC-1 positions within 1e-12 m and derivatives within 1e-9 relative of `ref_set_v1`; AC-2 sphere poles flagged degenerate; AC-3 cylinder curvatures 100 m⁻¹ and 0; AC-4 identical result digest on three targets; AC-5 outside-domain error. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1, no GPU. Files: `crates/fid-geo-nurbs/src/eval.rs`, `src/basis.rs`, `tests/eval.rs`.

## Open questions

- None.

## Children

_Leaf: none._
