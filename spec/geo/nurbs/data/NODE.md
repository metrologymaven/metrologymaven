---
id: GEO.NURBS.DATA
title: B-spline and NURBS data types and validation
level: 3
parent: GEO.NURBS
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-geo-nurbs]
satisfies: [REQ-GEO.NURBS-001, REQ-GEO.NURBS-002]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.NURBS.DATA: B-spline and NURBS data types and validation

## Purpose

The validated value types for B-spline and NURBS curves and surfaces: degree, knot vectors, control points, optional weights, closure and periodicity flags, and parameter domains. Every evaluator, the CAD layer and the importers build on these, so invalid geometry is stopped once, at construction, with a typed error.

## Scope

**In:**
- `KnotVector` (non-decreasing f64 knots with multiplicities)
- `NurbsCurve { degree, knots, ctrl: Vec<[f64; 3]>, weights: Option<Vec<f64>>, periodic }`
- `NurbsSurface { degree_u, degree_v, knots_u, knots_v, ctrl (row-major n_u × n_v), weights, periodic_u, periodic_v }`
- Validation rules and the parameter domain of each type
- Degree range 1 to 25 [unverified as sufficient for imported STEP data]

**Out:**
- Evaluation (owned by `GEO.NURBS.EVAL`)
- Analytic types (owned by `GEO.NURBS.ANALYTIC`)
- Trimming loops (owned by `GEO.CAD.TOPO`)

## Requirements

- **REQ-GEO.NURBS.DATA-001** If a knot vector is not non-decreasing, contains a non-finite value, or has an interior multiplicity above the degree, then construction shall fail with `BadKnots { index }`.
- **REQ-GEO.NURBS.DATA-002** If the number of knots differs from control points + degree + 1 (per direction), then construction shall fail with `CountMismatch { knots, ctrl, degree }`.
- **REQ-GEO.NURBS.DATA-003** If any weight is non-finite or not strictly positive, then construction shall fail with `BadWeight { index }`.
- **REQ-GEO.NURBS.DATA-004** If a degree is 0 or above 25, or any control point is non-finite, then construction shall fail with a typed error.
- **REQ-GEO.NURBS.DATA-005** Each valid curve and surface shall report its parameter domain as `[knots[p], knots[n]]` per direction and whether it is rational (has weights not all equal).
- **REQ-GEO.NURBS.DATA-006** Construction shall never modify the input data (no knot normalisation, no weight rescaling).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/nurbs/valid_set.json` (40 curves and surfaces, degrees 1 to 25, rational and non-rational), when constructed, then all succeed and the reported domains and rational flags match the fixture.
- **AC-2** Given fixture `fixtures/geo/nurbs/invalid_set.json` (one case per error variant), when constructed, then each fails with exactly the expected error and index.
- **AC-3** Given any valid case, when constructed and serialised, then knots, control points and weights are bit-identical to the input.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** as in Scope; `NurbsError { BadKnots, CountMismatch, BadWeight, BadDegree, NonFiniteCtrl }`.
- **Algorithm:** linear validation passes in index order; first failure reported.
- **Errors:** as above.
- **Determinism:** D0.
- **Performance budget:** validation is O(n) and negligible [unverified].
- **Crate:** `fid-geo-nurbs` (kernel graph, KNP-1 applies).

## Test fixtures

- `valid_set.json` and `invalid_set.json`: hand-built and seeded random cases (seed 41), part of the NURBS reference set produced by GEO.NURBS.REF. CC-BY-4.0.

## Build prompt

> Create crate `fid-geo-nurbs` (pure Rust, f64, no I/O, kernel graph, KNP-1) with module `data`: `KnotVector`, `NurbsCurve { degree, knots, ctrl: Vec<[f64;3]>, weights: Option<Vec<f64>>, periodic }` and `NurbsSurface` (two degrees, two knot vectors, row-major control net, optional weights, two periodic flags). Constructors validate in index order and return the first failure: non-decreasing finite knots with interior multiplicity ≤ degree (`BadKnots{index}`), knots = ctrl + degree + 1 per direction (`CountMismatch`), weights finite and > 0 (`BadWeight{index}`), degree in 1..=25 (`BadDegree`), finite control points (`NonFiniteCtrl`). Never modify inputs. Expose `domain()` and `is_rational()`. Acceptance: AC-1 the 40 valid cases with expected domains; AC-2 one invalid case per error with the right index; AC-3 bit-identical round trip. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1. Files: `crates/fid-geo-nurbs/src/data.rs`, `src/lib.rs`, `tests/data.rs`.

## Open questions

- None.

## Children

_Leaf: none._
