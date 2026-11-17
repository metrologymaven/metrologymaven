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
- **Errors:** `OutsideDomain`, `OrderTooHigh`.
- **Determinism:** D0.
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
