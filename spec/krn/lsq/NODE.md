---
id: KRN.LSQ
title: Fit contract and least-squares (Gaussian) fits
level: 2
parent: KRN
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN-002, REQ-KRN-003, REQ-KRN-005, REQ-KRN-007]
covers: [GOM.CON.18, PW.CON.01, PW.CON.02, PW.CON.03, PW.CON.07, SA.REL.01]
depends_on: [KRN.NUM, KRN.SOLV, KRN.MAN, GEO]
cli: [fid fit ls-*]
mcp: [fit_ls_*]
journal: [fit.ls_*]
updated: 2026-11-16
---

# KRN.LSQ: Fit contract and least-squares (Gaussian) fits

## Purpose

Defines the `Fit` trait that every fitting method implements (inputs, parameters, result, diagnostics, typed errors), and implements orthogonal-distance least-squares fits for every primitive in the parent's scope. Least squares is the default method in every incumbent and the starting point for minimum-zone and envelope fits, so the shared contract lives here and the other fit subsystems depend on it.

## Scope

**In:**
- The `Fit` trait, the fitted-feature result types (parameters, residuals, statistics, certificate) and the fit error taxonomy
- Primitive parameterisations without singularities (for example, axis direction by local tangent-plane parameters)
- Input centring and scaling, and degenerate-input detection (too few points, collinear or coplanar sets, partial arcs below a conditioning threshold)
- Deterministic initial estimates and multi-start for cylinder, cone and torus
- Orthogonal-distance least-squares fits for point (centroid), 2D and 3D line, plane, 2D and 3D circle, slot, rectangle, cylinder, cone, sphere, torus, paraboloid, ellipse, and polyline and spline curves
- Fixed constraints that keep a least-squares fit well-posed (fixed radius, fixed axis direction, projection onto a plane) as fit parameters
- Residual statistics (RMS, max, min, standard deviation, per-point signed distances)
- Spec cards and algorithm IDs for every least-squares algorithm (format from `KRN.MAN`)

**Out** (and which node owns it instead):
- Minimum-zone fits (`KRN.MZ`)
- Circumscribed, inscribed, tangential and datum-simulator fits (`KRN.ENV`)
- Outlier rejection and point selection before or during a fit (`KRN.OUT`)
- Building features from other features (`KRN.CONS`)
- Validation-run publication and the manifest (`KRN.MAN`)
- Uncertainty of fitted parameters (`UNC`)

## Requirements

- **REQ-KRN.LSQ-001** The `Fit` trait shall take a point set (GEO types, SI metres), typed parameters and an optional seed, and shall return either a fitted feature with residuals, statistics and a certificate, or a typed error.
- **REQ-KRN.LSQ-002** The subsystem shall provide orthogonal-distance least-squares fits for point, 2D and 3D line, plane, 2D and 3D circle, slot, rectangle, cylinder, cone, sphere, torus, paraboloid, ellipse and polyline or spline curves.
- **REQ-KRN.LSQ-003** Each least-squares fit shall minimise the sum of squared orthogonal distances, using parameterisations without singularities and centred, scaled input.
- **REQ-KRN.LSQ-004** On the PTB TraCIM Gaussian test sets for 3D line, plane, 3D circle, cylinder, cone and sphere, each fit shall stay within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.LSQ-005** On the NIST algorithm-testing reference datasets for line, plane, circle, sphere, cylinder and cone, each fit shall reproduce the reference parameters within the same internal limits as REQ-KRN.LSQ-004 [unverified].
- **REQ-KRN.LSQ-006** If the input has fewer points than the primitive needs, is degenerate (collinear, coplanar or a partial arc below the conditioning threshold), or yields a Jacobian condition number above the configured limit, then the fit shall return a typed error with the condition number, rank and point count.
- **REQ-KRN.LSQ-007** Each least-squares fit result shall report the gradient norm, Jacobian rank, condition number, iteration count and RMS and maximum residuals.
- **REQ-KRN.LSQ-008** For cylinder, cone and torus, the fit shall use a deterministic multi-start and shall flag the result when the starts disagree.
- **REQ-KRN.LSQ-009** Each least-squares algorithm shall have an algorithm ID of the form `fit.<primitive>.ls@<semver>` and a spec card before it is registered.
- **REQ-KRN.LSQ-010** When a least-squares fit command is committed, the system shall journal one `fit.ls_<primitive>` event carrying the algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** the `Fit` trait and result types; commands `fit.ls_<primitive>@1` (side-effect class R1 for preview, W2 when the fitted feature is committed to the document); CLI `fid fit ls-<primitive>`; MCP `fit_ls_<primitive>`; journal `fit.ls_*`.
- **Consumes:** `KRN.NUM`, `KRN.SOLV` (NLLS driver, multi-start), `KRN.MAN` (spec-card format), `GEO` point types.

## Decomposition hints

TRAIT (fit contract, result and error types), LIN (point, line, plane), RND (circle, sphere, cylinder, cone, torus), PLAN2D (slot, rectangle, ellipse), CURVE (paraboloid, polyline, spline), INIT (initial estimates and multi-start).

## Open questions

- Whether the R1/W2 split for fit commands is one command with a commit flag or two commands is a CORE registry convention; follow CORE when it is decided.
- Exact TraCIM default MPE values must be taken from PTB's published test conditions, not recalled [unverified].
- **R1/W2 convention (assumed at this split):** each fit is one registry command of class W2 whose dry-run returns the R1 scratch result (CONSTITUTION §3: dry-run where meaningful). Revisit if `CORE` decides on two commands.
- **Kernel layering (cross-domain):** `GEO.TYPES` depends on `CORE`, but the kernel may depend on nothing above it. The children therefore take a borrowed, plain-data `PointsView` that `GEO.TYPES` can produce, with no crate dependency from the kernel to `GEO.TYPES` or `CORE`; and fit commands are declared as data rows read by `CORE.REG` rather than through `CORE` macros. Confirm with GEO and CORE.

## Split rationale

The split separates the **shared contract** (TRAIT) and **start-value machinery** (INIT) from the fits, and groups the fits by **mathematical family**, because primitives in one family share parameterisation, start strategy and validation data:
- LIN: point, lines and plane (closed form through an eigen or singular-value problem);
- RND: circles and sphere (iterative, algebraic start, convex enough for one start);
- AXIS: cylinder, cone and torus (iterative, axis parameterisation, multi-start);
- PLAN2D: slot, rectangle and ellipse (compound or conic planar shapes);
- CURVE: paraboloid, polyline and spline curves (no TraCIM reference).

The TraCIM Gaussian set falls in LIN, RND and AXIS, so those three carry REQ-KRN.LSQ-004 and -005.

Alternatives considered:
- **One child per primitive.** Rejected: 15 children, with the same start and validation logic repeated.
- **Split by dimension (2D versus 3D).** Rejected: 2D and 3D circles share the method and the test data; only the projection differs.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.LSQ.TRAIT | Fit contract, result types and degeneracy checks | 001 003 006 007 009 010 | PW.CON.01 |
| KRN.LSQ.INIT | Initial estimates for axial primitives | 003 008 | none |
| KRN.LSQ.LIN | Least-squares point, line and plane fits | 002 003 004 005 006 007 009 010 | GOM.CON.18, PW.CON.01, PW.CON.02, SA.REL.01 |
| KRN.LSQ.RND | Least-squares circle and sphere fits | 002 003 004 005 006 007 009 010 | GOM.CON.18, PW.CON.01, PW.CON.02, SA.REL.01 |
| KRN.LSQ.AXIS | Least-squares cylinder, cone and torus fits | 002 003 004 005 006 007 008 009 010 | GOM.CON.18, PW.CON.02, PW.CON.07, SA.REL.01 |
| KRN.LSQ.PLAN2D | Least-squares slot, rectangle and ellipse fits | 002 003 006 007 009 010 | GOM.CON.18, PW.CON.03 |
| KRN.LSQ.CURVE | Least-squares paraboloid, polyline and spline fits | 002 003 006 007 009 010 | GOM.CON.18, PW.CON.03, SA.REL.01 |
