---
id: KRN.MZ
title: Minimum-zone (Chebyshev) fits
level: 2
parent: KRN
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN-002, REQ-KRN-003, REQ-KRN-005, REQ-KRN-007]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ, KRN.SOLV, KRN.MAN]
cli: [fid fit mz-*]
mcp: [fit_mz_*]
journal: [fit.mz_*]
updated: 2026-11-16
---

# KRN.MZ: Minimum-zone (Chebyshev) fits

## Purpose

Minimum-zone fits: the feature that minimises the maximum orthogonal deviation. They are the default association for form evaluation under ISO GPS and the second family TraCIM certifies. They need conic solvers, certificates and multi-start for non-convex cases, which makes them a separate body of work from least squares.

## Scope

**In:**
- Minimum-zone fits for 2D line, 3D line, plane, 2D circle, sphere and cylinder (the TraCIM Chebyshev set), then cone, slot, rectangle, ellipse and curves where a zone is defined
- Formulation as sequential LP or SOCP linearised about the least-squares solution
- An independent exact path for the 2D circle (Voronoi-based) as a cross-check
- Certificates: critical point set, multipliers, local optimality check, multi-start agreement flag
- Spec cards and algorithm IDs `fit.<primitive>.mz@<semver>`

**Out** (and which node owns it instead):
- The `Fit` trait and least-squares starts (`KRN.LSQ`)
- Conic solvers, active-set recomputation and tie-break (`KRN.SOLV`)
- Minimum circumscribed and maximum inscribed fits (`KRN.ENV`)
- Form tolerance evaluation and filter defaults (`GDT`)

## Requirements

- **REQ-KRN.MZ-001** The subsystem shall provide minimum-zone fits for 2D line, 3D line, plane, 2D circle, sphere and cylinder, each implementing the `Fit` trait.
- **REQ-KRN.MZ-002** The subsystem shall provide minimum-zone fits for cone, slot, rectangle, ellipse and planar curves, or a typed `method_undefined` error for any primitive where a zone is not defined.
- **REQ-KRN.MZ-003** On the PTB TraCIM Chebyshev test sets for 2D line, 2D circle, plane, sphere and cylinder, each fit shall stay within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.MZ-004** Each minimum-zone result shall carry a certificate with the critical point set, the multipliers and the outcome of a local optimality check.
- **REQ-KRN.MZ-005** The reported zone width shall be recomputed from the critical point set, never taken from the solver's last iterate.
- **REQ-KRN.MZ-006** For the 2D circle, the exact Voronoi-based path and the conic path shall agree within 1e-9 m [unverified] on every reference case, and any disagreement shall fail CI.
- **REQ-KRN.MZ-007** When a minimum-zone problem is non-convex (circle, sphere, cylinder), the result shall carry the multi-start agreement flag, and a `local_optimum_only` flag when the starts disagree.
- **REQ-KRN.MZ-008** If the input is a partial feature whose angular coverage is below the configured conditioning threshold, then the fit shall return a typed error rather than a zone.
- **REQ-KRN.MZ-009** When a minimum-zone fit command is committed, the system shall journal one `fit.mz_<primitive>` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.mz_<primitive>@1`; CLI `fid fit mz-<primitive>`; MCP `fit_mz_<primitive>`; journal `fit.mz_*`; minimum-zone certificates for `GDT` form evaluation.
- **Consumes:** `KRN.LSQ` (trait, starting solution), `KRN.SOLV` (conic solvers, certificates), `KRN.MAN` (spec cards).

## Decomposition hints

LIN (lines and plane), RND (circle, sphere, cylinder), VOR (exact 2D circle cross-check), EXT (cone, slot, rectangle, ellipse, curves).

## Open questions

- Which primitives beyond the TraCIM set have a published reference for minimum zone; where none exists, results carry `reference_basis: fiducial-ORS` or `none` (owned by `UNC`).

## Split rationale

The split follows **convexity and reference coverage**. Lines and the plane (LIN) linearise to near-convex LP problems and are certified by TraCIM. Circle, sphere and cylinder (RND) are non-convex, need sequential linearisation, multi-start and partial-coverage checks, and are also certified by TraCIM. The exact Voronoi path for the 2D circle (VOR) is an independent algorithm family used as a cross-check and is reused by `KRN.ENV` for the maximum inscribed circle, so it stands alone. The remaining primitives (EXT) have no NMI reference and start later.

Alternatives considered:
- **One child per primitive.** Rejected: the six TraCIM primitives share two formulations.
- **Fold VOR into RND.** Rejected: an independent cross-check should be built and reviewed separately from the path it checks.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.MZ.LIN | Minimum-zone lines and plane | 001 003 004 005 009 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.RND | Minimum-zone circle, sphere and cylinder | 001 003 004 005 007 008 009 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.VOR | Exact Voronoi-based minimum-zone 2D circle | 006 | none |
| KRN.MZ.EXT | Minimum-zone cone, slot, rectangle, ellipse and planar curves | 002 004 005 008 009 | GOM.CON.19, PW.CON.05 |
