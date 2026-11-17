---
id: KRN.MZ.RND
title: Minimum-zone circle, sphere and cylinder
level: 3
parent: KRN.MZ
kind: module
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ-001, REQ-KRN.MZ-003, REQ-KRN.MZ-004, REQ-KRN.MZ-005, REQ-KRN.MZ-007, REQ-KRN.MZ-008, REQ-KRN.MZ-009]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.RND, KRN.LSQ.AXIS, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART]
uses_artifacts: [UNC.RDL.FREEZE]
cli: [fid fit mz-circle2d, fid fit mz-sphere, fid fit mz-cylinder]
mcp: [fit_mz_circle2d, fit_mz_sphere, fit_mz_cylinder]
journal: [fit.mz_circle2d@1, fit.mz_sphere@1, fit.mz_cylinder@1]
updated: 2026-11-17
---

# KRN.MZ.RND: Minimum-zone circle, sphere and cylinder

## Purpose

Chebyshev fits for roundness (2D circle), sphericity and cylindricity: the pair of concentric (or coaxial) boundaries with minimum radial separation containing all points. These are non-convex, so they use sequential linearisation from the least-squares solution, deterministic multi-start and the `local_optimum_only` flag, and partial features need coverage checks.

## Scope

**In:**
- Formulations for `fit.circle2d.mz`, `fit.sphere.mz`, `fit.cylinder.mz` as sequential SOCP or LP about the least-squares solution
- Multi-start with agreement flag; partial-coverage guard
- Active-set callbacks for certificates; recomputed zone width
- The CI agreement job between the 2D circle conic path and `KRN.MZ.VOR`
- Registry commands and spec cards

**Out** (and which node owns it instead):
- The exact Voronoi algorithm itself (`KRN.MZ.VOR`)
- Least-squares starts (`KRN.LSQ.RND`, `KRN.LSQ.AXIS`); solvers (`KRN.SOLV`)
- Roundness and cylindricity tolerance evaluation and filtering (`GDT`)

## Requirements

- **REQ-KRN.MZ.RND-001** The module shall compute minimum-zone 2D circle, sphere and cylinder fits, each implementing the `Fit` trait, from the least-squares start through `KRN.SOLV.SEQ`.
- **REQ-KRN.MZ.RND-002** Each fit shall run a deterministic multi-start and shall set `local_optimum_only` when starts disagree.
- **REQ-KRN.MZ.RND-003** If angular coverage is below the configured threshold, then the fit shall return `InsufficientCoverage` and no zone.
- **REQ-KRN.MZ.RND-004** The reported zone width shall be recomputed from the critical point set and carried with its certificate.
- **REQ-KRN.MZ.RND-005** On the PTB TraCIM Chebyshev 2D circle, sphere and cylinder sets, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.MZ.RND-006** When a command is committed, the system shall journal one `fit.mz_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.mz_circle2d@1`, `fit.mz_sphere@1`, `fit.mz_cylinder@1` (W2, dry-run R1); certificates for `GDT` form evaluation.
- **Consumes:** `KRN.LSQ.RND`, `KRN.LSQ.AXIS`, `KRN.SOLV.SEQ`, `KRN.SOLV.CERT`, `KRN.SOLV.MSTART`; `KRN.MZ.VOR` in CI only.

## Decomposition hints

CIRC (2D circle and its VOR agreement job), SPH (sphere), CYL (cylinder, axis linearisation, the hardest case).

## Open questions

- The concept doc notes MZ may slip to early P1a; the phase stays P0 here.

- `KRN.MZ.VOR` is used by `KRN.MZ.RND.CIRC` in CI only (agreement job), declared as a `depends_on` of that leaf because the test links it; it never enters the fit's call path.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.MZ.RND.CIRC | Minimum-zone 2D circle and Voronoi agreement job | 001 002 003 004 005 006 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.RND.SPH | Minimum-zone sphere | 001 002 003 004 005 006 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.RND.CYL | Minimum-zone cylinder | 001 002 003 004 005 006 | GOM.CON.19, PW.CON.05 |
