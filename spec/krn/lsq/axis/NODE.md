---
id: KRN.LSQ.AXIS
title: Least-squares cylinder, cone and torus fits
level: 3
parent: KRN.LSQ
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ-002, REQ-KRN.LSQ-003, REQ-KRN.LSQ-004, REQ-KRN.LSQ-005, REQ-KRN.LSQ-006, REQ-KRN.LSQ-007, REQ-KRN.LSQ-008, REQ-KRN.LSQ-009, REQ-KRN.LSQ-010]
covers: [GOM.CON.18, PW.CON.02, PW.CON.07, SA.REL.01]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.INIT, KRN.SOLV.NLLS, KRN.SOLV.MSTART]
uses_artifacts: [UNC.RDL.FREEZE]
cli: [fid fit ls-cylinder, fid fit ls-cone, fid fit ls-torus]
mcp: [fit_ls_cylinder, fit_ls_cone, fit_ls_torus]
journal: [fit.ls_cylinder@1, fit.ls_cone@1, fit.ls_torus@1]
updated: 2026-11-17
---

# KRN.LSQ.AXIS: Least-squares cylinder, cone and torus fits

## Purpose

Orthogonal-distance least-squares fits for the axial primitives. They are non-convex in the axis direction, so each runs a deterministic multi-start from `KRN.LSQ.INIT` and flags disagreement. Cylinder and cone are in the TraCIM Gaussian set; torus has no NMI reference and relies on open reference suites.

## Scope

**In:**
- Algorithms `fit.cylinder.ls`, `fit.cone.ls`, `fit.torus.ls` with tangent-plane axis parameterisation
- Fixed-radius and fixed-axis-direction constraints (cylinder), fixed half-angle (cone)
- Multi-start through `KRN.SOLV.MSTART` and the `local_optimum_only` flag
- Wide-angle and near-cylindrical cone handling (typed errors where the cone is ill-posed)
- The three registry commands and spec cards

**Out** (and which node owns it instead):
- Start heuristics (`KRN.LSQ.INIT`)
- Minimum-zone and envelope cylinders (`KRN.MZ`, `KRN.ENV`)
- Constructing cylinders from other features (`KRN.CONS`)

## Requirements

- **REQ-KRN.LSQ.AXIS-001** The module shall fit cylinder, cone and torus by minimising the sum of squared orthogonal distances, with the axis parameterised by two tangent-plane parameters about the start axis.
- **REQ-KRN.LSQ.AXIS-002** Each fit shall run every start of the `StartSpec` from `KRN.LSQ.INIT` through `KRN.SOLV.MSTART` and shall set `local_optimum_only` when the starts disagree.
- **REQ-KRN.LSQ.AXIS-003** On the PTB TraCIM Gaussian cylinder and cone sets and the NIST algorithm-testing cylinder and cone datasets, each fit shall be within 1/10 of the TraCIM default maximum permissible errors [unverified].
- **REQ-KRN.LSQ.AXIS-004** If a cone's half-angle is within the configured threshold of 0 (near-cylinder) or of 90 degrees (near-plane), then the cone fit shall return `IllConditioned` with the half-angle and condition number.
- **REQ-KRN.LSQ.AXIS-005** Each result shall report diagnostics, residual statistics, the start list and per-start outcomes.
- **REQ-KRN.LSQ.AXIS-006** When a command is committed, the system shall journal one `fit.ls_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.ls_cylinder@1`, `fit.ls_cone@1`, `fit.ls_torus@1` (W2, dry-run R1); `FittedFeature::{Cylinder, Cone, Torus}` results used as starts by `KRN.MZ` and `KRN.ENV`.
- **Consumes:** `KRN.LSQ.TRAIT`, `KRN.LSQ.INIT`, `KRN.SOLV.NLLS`, `KRN.SOLV.MSTART`.

## Decomposition hints

CYL (cylinder with constraints; TraCIM), CONE (cone, apex handling and ill-posed cases; TraCIM), TORUS (torus; open reference suites only).

## Open questions

- Which cone parameterisation (apex versus a point on the axis plus radius at that point) is better conditioned for shallow cones [unverified]; the ISO 10360-6 exclusion of wide-angle cones is a reference for the error thresholds. `KRN.LSQ.AXIS.CONE` decides it at the leaf (apex-free parameterisation by a point on the axis and the radius there) and cites ISO 10360-6 by designator and clause only.
- Reference datasets are read at test time from the frozen RDL release (`uses_artifacts: UNC.RDL.FREEZE`), never built in.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.LSQ.AXIS.CYL | Least-squares cylinder | 001 002 003 005 006 | GOM.CON.18, PW.CON.02, SA.REL.01 |
| KRN.LSQ.AXIS.CONE | Least-squares cone | 001 002 003 004 005 006 | GOM.CON.18, PW.CON.02, SA.REL.01 |
| KRN.LSQ.AXIS.TORUS | Least-squares torus | 001 002 005 006 | PW.CON.07 |
