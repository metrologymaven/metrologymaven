---
id: GEO.NURBS.ANALYTIC
title: Closed-form analytic curves and surfaces
level: 3
parent: GEO.NURBS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-nurbs]
satisfies: [REQ-GEO.NURBS-004, REQ-GEO.NURBS-006, REQ-GEO.NURBS-007]
covers: []
depends_on: [GEO.NURBS.EVAL, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.NURBS.ANALYTIC: Closed-form analytic curves and surfaces

## Purpose

The elementary geometry that CAD files carry in exact form: lines, circles and ellipses; planes, cylinders, cones, spheres and tori. Each is evaluated and inverted in closed form through the same `Curve` and `Surface` traits as NURBS, so nominal values stay exact and fast and are never approximated by splines.

## Scope

**In:**
- Placement (origin, axis, reference direction) for every analytic type
- Curves: line, circle, ellipse; surfaces: plane, cylinder, cone (half-angle), sphere, torus (major and minor radius)
- Parameterisation conventions following ISO 10303-42 placements [unverified clause details], documented in the crate
- Position, derivatives to second order, normal and curvature in closed form
- Closed-form closest point (inversion) on each untrimmed analytic curve and surface, including the degenerate cases (point on axis, sphere centre, cone apex)

**Out:**
- NURBS evaluation and inversion (owned by `GEO.NURBS.EVAL` and `GEO.NURBS.INVERT`)
- Recognising analytic shapes encoded as NURBS (owned by `GEO.CAD.NOMINAL`)

## Requirements

- **REQ-GEO.NURBS.ANALYTIC-001** The library shall evaluate plane, cylinder, cone, sphere and torus surfaces and line, circle and ellipse curves in closed form through the `Surface` and `Curve` traits, without converting them to NURBS.
- **REQ-GEO.NURBS.ANALYTIC-002** If a placement's axis and reference direction are not unit and orthogonal within 1e-12, or a radius or half-angle is out of range, then construction shall fail with a typed error.
- **REQ-GEO.NURBS.ANALYTIC-003** Closed-form inversion shall return the exact closest point up to rounding, and when the closest point is not unique (point on a cylinder axis, sphere centre, cone apex region, torus axis) it shall return the canonical choice at parameter u = 0 and flag `NonUnique`.
- **REQ-GEO.NURBS.ANALYTIC-004** Positions and inversions shall agree with the high-precision reference within 1e-13 m for features up to 1 m in size.
- **REQ-GEO.NURBS.ANALYTIC-005** Trigonometric functions shall come only from `fid-num`, so results are bit-identical on x86-64, ARM64 and wasm32.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/nurbs/analytic_set.json` (seeded placements and sizes for all eight types with evaluation and inversion queries and mpmath references), when evaluated and inverted, then all positions and foot points are within 1e-13 m.
- **AC-2** Given a query point on a cylinder's axis, when inverted, then the foot point is at u = 0 at radius distance and `NonUnique` is set.
- **AC-3** (error path) Given a cone with half-angle 0 or ≥ π/2, or a torus with minor radius ≥ major radius where self-intersection is not allowed, when constructed, then a typed error is returned.
- **AC-4** Given `analytic_set.json`, when run on three targets, then the result digest is identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Placement { origin, axis, ref_dir }`; `AnalyticSurface { Plane, Cylinder { r }, Cone { r0, half_angle }, Sphere { r }, Torus { major, minor } }`; `AnalyticCurve { Line, Circle { r }, Ellipse { a, b } }`.
- **Algorithm:** transform the query into the local placement, solve the closest point geometrically (radial projection for cylinder, sphere and torus tube circle; generator-line projection for cone; Newton-free closed forms); ellipse inversion uses a bracketed root solve of the quartic in a fixed iteration count [unverified convergence bound].
- **Errors:** `BadPlacement`, `BadRadius`, `BadAngle`.
- **Determinism:** D0.
- **Performance budget:** under 100 ns per evaluation and 300 ns per inversion [unverified].
- **Crate:** `fid-geo-nurbs` (kernel graph, KNP-1 applies).

## Test fixtures

- `fixtures/geo/nurbs/analytic_set.json`: seed 42, references computed with mpmath at 50 digits by the GEO.NURBS.REF scripts. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-nurbs` (pure Rust, no I/O, KNP-1), add module `analytic`: `Placement { origin, axis, ref_dir }` (unit and orthogonal within 1e-12) and types `Plane`, `Cylinder`, `Cone`, `Sphere`, `Torus`, `Line`, `Circle`, `Ellipse`, implementing the `Surface`/`Curve` traits from `eval` (position, derivatives to second order, normal, curvature) in closed form, plus `invert(point)` returning parameters, foot point, distance and a `NonUnique` flag with canonical u = 0 choice at axis, centre and apex cases. Document parameterisations following ISO 10303-42 placement conventions (cite clause numbers only). Trig via `fid-num` only. Acceptance: AC-1 all eight types within 1e-13 m of mpmath references; AC-2 on-axis cylinder inversion; AC-3 invalid constructions rejected; AC-4 identical digest on three targets. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1, no GPU. Files: `crates/fid-geo-nurbs/src/analytic.rs`, `tests/analytic.rs`.

## Open questions

- Whether self-intersecting tori (minor ≥ major) from STEP must be supported; this leaf rejects them until IO.CAD reports real files that use them.

## Children

_Leaf: none._
