---
id: GEO.NURBS
title: NURBS curve and surface evaluation
level: 2
parent: GEO
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO-001, REQ-GEO-003]
covers: []
depends_on: [GEO.TYPES]
cli: [fid geo evaluate-*, fid geo invert-*]
mcp: [geo_evaluate_*, geo_invert_*]
journal: [geo.evaluate_*@*, geo.invert_*@*]
updated: 2026-11-16
---

# GEO.NURBS: NURBS curve and surface evaluation

## Purpose

The exact-geometry evaluator: B-spline and NURBS curves and surfaces, plus the analytic surfaces that STEP carries (plane, cylinder, cone, sphere, torus). It evaluates positions, derivatives, normals and curvature, and inverts a point onto a single curve or surface. It is the numeric core that CAD tessellation and point-to-CAD projection rely on, kept separate so it can be tested against reference data on its own.

## Scope

**In:**
- Curve and surface data types: knot vectors, control points, weights, degree; analytic curves and surfaces
- Validation of knot vectors, weights and degrees
- Evaluation of position and derivatives up to second order
- Normals, principal curvatures and directions
- Point inversion on a single untrimmed curve or surface (parameter-space closest point)
- Parameter-domain bounds and periodicity

**Out** (and which node owns it instead):
- Trimming loops, faces, shells and bodies (owned by `GEO.CAD`)
- Tessellation (owned by `GEO.CAD`)
- Closest point on a trimmed face or whole body, and signed distance (owned by `GEO.DIST`)
- Surface fitting to measured points (owned by `KRN`)
- Surface modelling and editing (out of scope for the product)

## Requirements

- **REQ-GEO.NURBS-001** The evaluator shall evaluate rational and non-rational B-spline curves and surfaces of degree 1 to 25 [unverified] at any parameter in their domain.
- **REQ-GEO.NURBS-002** If a curve or surface has a non-monotone knot vector, a non-positive weight or inconsistent control-point counts, then the system shall reject it with a typed error.
- **REQ-GEO.NURBS-003** Evaluated positions shall agree with an independent reference evaluator within 1e-12 m on the NURBS reference fixture [unverified].
- **REQ-GEO.NURBS-004** The evaluator shall return first and second partial derivatives, unit normal and principal curvatures at any regular point, and shall flag degenerate points (zero-length normal) explicitly.
- **REQ-GEO.NURBS-005** When asked to invert a point onto a single surface, the system shall return the parameter pair, foot point and distance of the closest point on the untrimmed surface, or a typed non-convergence error.
- **REQ-GEO.NURBS-006** Evaluation and inversion shall be deterministic class D0 or D1, follow KNP-1, and use no GPU computation.
- **REQ-GEO.NURBS-007** The analytic surfaces plane, cylinder, cone, sphere and torus shall be evaluated in closed form, not converted to NURBS.

## Interfaces

- **Offers:** `NurbsCurve`, `NurbsSurface`, `AnalyticSurface`, `Surface` trait (`eval`, `derivs`, `normal`, `curvature`, `invert`); commands `geo.evaluate_surface@1`, `geo.invert_point@1` (R1).
- **Consumes:** GEO.TYPES; fid-num (KRN) for transcendentals and linear algebra.

## Decomposition hints

DATA (types and validation), EVAL (de Boor style evaluation and derivatives), ANALYTIC (closed-form surfaces), INVERT (point inversion), REF (reference fixtures and comparison).

## Split rationale

The split follows the **evaluation pipeline**: validated data (DATA), spline evaluation and the shared `Curve`/`Surface` traits (EVAL), closed-form analytic geometry behind the same traits (ANALYTIC), global point inversion (INVERT), the reference oracle and fixtures (REF), and the registry surface (CMD, in `fid-geo-cmd`). All library children live in `fid-geo-nurbs` in the kernel graph.

The parent's open question on the reference evaluator is answered by GEO.NURBS.REF: the project writes its own exact-rational and high-precision oracle in Python, so no third-party evaluator and no licence question is involved.

Alternatives considered:
- **Analytic surfaces converted to NURBS.** Rejected by REQ-GEO.NURBS-007.
- **Inversion inside DIST.** Rejected: single-surface inversion is testable on its own and GEO.CAD.NOMINAL and SEL may use it without trimming.
- **Using an existing CAD kernel as the oracle.** Rejected: licence coupling and a shared-bug risk; independent code is a stronger check.

## Open questions

- Phase set to P1b to match CONSTITUTION §7 (CAD + PMI); confirm in review.
- The reference evaluator for REQ-GEO.NURBS-003 is not chosen; it must be licence-compatible (D-001) or used only to generate fixtures offline.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GEO.NURBS.DATA | B-spline and NURBS data types and validation (leaf) | 001 002 | none |
| GEO.NURBS.EVAL | NURBS evaluation, derivatives, normals and curvature (leaf) | 001 004 006 | none |
| GEO.NURBS.ANALYTIC | Closed-form analytic curves and surfaces (leaf) | 004 006 007 | none |
| GEO.NURBS.INVERT | Point inversion on NURBS curves and surfaces (leaf) | 005 006 | none |
| GEO.NURBS.REF | NURBS reference fixtures and independent oracle (leaf) | 003 | none |
| GEO.NURBS.CMD | Curve and surface evaluation and inversion commands (leaf) | 001 005 | none |
