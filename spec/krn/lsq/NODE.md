---
id: KRN.LSQ
title: Fit contract and least-squares (Gaussian) fits
level: 2
parent: KRN
kind: subsystem
status: open
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

## Children

_None yet: status `open`._
