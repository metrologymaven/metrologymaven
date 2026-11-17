---
id: ALN.BFIT.FEAT
title: Feature-to-nominal best-fit alignment
level: 3
parent: ALN.BFIT
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN.BFIT-001, REQ-ALN.BFIT-008]
covers: [PCD.ALN.02]
depends_on: [ALN.BFIT.CONSTR, ALN.BFIT.STOP, ALN.MODEL.ELEM, ALN.QA.REPORT, KRN.LSQ, KRN.SOLV]
cli: [fid align run-bestfit-features]
mcp: [align_run_bestfit_features]
journal: [align.run_bestfit_features@1]
updated: 2026-11-16
---

# ALN.BFIT.FEAT: Feature-to-nominal best-fit alignment

## Purpose

The CMM-style best-fit alignment: a set of measured features (points, circles, spheres, cylinders, planes, lines, slots) is fitted to their nominals in one simultaneous weighted least-squares objective over centres, axes and normals, in 2D (in a plane) or 3D, with DoF locks and weights. It differs from `ALN.DATUM.ELEM`, which applies feature pairs in priority order with DoF accounting.

## Scope

**In:**
- Per-feature-type deviation definitions (centre distance, axis direction, normal distance and angle, with documented length scaling for angular terms)
- 2D best-fit in a named working plane and full 3D best-fit
- Optional feature-size use (for example, ignore diameter) and per-feature weights
- `align.run_bestfit_features@1` and its R1 entry point

**Out** (and which node owns it instead):
- Priority-ordered element alignment with DoF accounting (`ALN.DATUM.ELEM`)
- Fitting the features themselves (`KRN.LSQ`)
- Surface best-fit (`ALN.BFIT.ICP`)

## Requirements

- **REQ-ALN.BFIT.FEAT-001** When `align.run_bestfit_features` runs on N measured-to-nominal feature pairs, the system shall minimise the weighted sum of squared per-feature deviations over the free pose parameters and create an alignment.
- **REQ-ALN.BFIT.FEAT-002** Each supported feature type shall have a documented deviation vector, and angular terms shall be scaled by a recorded length (default: the feature's extent) to make them commensurable with lengths.
- **REQ-ALN.BFIT.FEAT-003** In 2D mode, the fit shall vary only the two in-plane translations and the rotation about the plane normal, and record the working plane.
- **REQ-ALN.BFIT.FEAT-004** If the pairs cannot determine the free DoF (for example two points in 3D with all DoF free), then the command shall fail with `underdetermined_feature_set` naming the undetermined DoF, and journal nothing.
- **REQ-ALN.BFIT.FEAT-005** Given noise-free synthetic feature sets with a known transform, the fit shall recover translation within 1e-9 m and rotation within 1e-9 rad [unverified].
- **REQ-ALN.BFIT.FEAT-006** The fit shall run in the CPU kernel under KNP-1 and declare class D0.

## Interfaces

- **Offers:** `align.run_bestfit_features@1` (W2; params `pairs`, `mode: 2d | 3d`, `working_plane`, `dof_locks`, `weights`, `use_size`, `stop_criteria`); R1 entry for CMM.
- **Consumes:** `ALN.BFIT.CONSTR`, `ALN.BFIT.STOP`, KRN fitted-feature types and KRN.SOLV; `ALN.MODEL.ELEM`; `ALN.QA.REPORT`.

## Decomposition hints

DEV (per-type deviation vectors and scaling), MODE2D (planar best-fit), SOLVE (weighted solve and determinacy check), CMD (command).

## Open questions

- PC-DMIS offers several best-fit variants (with and without scale, 2D and 3D) [unverified]; scale is excluded here and available through `ALN.XFM.SCALE` on feature centres.

## Children

_None yet: status `open`._
