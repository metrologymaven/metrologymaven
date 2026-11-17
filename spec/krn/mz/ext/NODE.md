---
id: KRN.MZ.EXT
title: Minimum-zone cone, slot, rectangle, ellipse and planar curves
level: 3
parent: KRN.MZ
kind: module
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN.MZ-002, REQ-KRN.MZ-004, REQ-KRN.MZ-005, REQ-KRN.MZ-008, REQ-KRN.MZ-009]
covers: [GOM.CON.19, PW.CON.05]
depends_on: [KRN.LSQ.AXIS, KRN.LSQ.PLAN2D, KRN.SOLV.NLLS, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART]
cli: [fid fit mz-cone, fid fit mz-slot, fid fit mz-rectangle, fid fit mz-ellipse, fid fit mz-curve2d]
mcp: [fit_mz_cone, fit_mz_slot, fit_mz_rectangle, fit_mz_ellipse, fit_mz_curve2d]
journal: [fit.mz_cone@1, fit.mz_slot@1, fit.mz_rectangle@1, fit.mz_ellipse@1, fit.mz_curve2d@1]
updated: 2026-11-17
---

# KRN.MZ.EXT: Minimum-zone cone, slot, rectangle, ellipse and planar curves

## Purpose

Minimum-zone fits beyond the TraCIM set: cone, slot, rectangle, ellipse and planar curves, where a zone is defined. For every other primitive-method pair, it returns a typed `method_undefined` error so callers never get a silently substituted method. None has an NMI reference, so results carry the reference basis from `UNC`.

## Scope

**In:**
- Minimum-zone formulations for cone, slot, rectangle, ellipse and planar curves (zone of constant width about a nominal-shaped curve), with certificates
- The `method_undefined` error and the table of defined primitive-method pairs for minimum zone
- Partial-coverage guards; registry commands; spec cards

**Out** (and which node owns it instead):
- TraCIM primitives (`KRN.MZ.LIN`, `KRN.MZ.RND`)
- Profile tolerance zones with datums (`GDT`)
- Reference cases for these primitives (`UNC`)

## Requirements

- **REQ-KRN.MZ.EXT-001** The module shall compute minimum-zone fits for cone, slot, rectangle, ellipse and planar curves, each implementing the `Fit` trait and carrying a certificate with a recomputed zone width.
- **REQ-KRN.MZ.EXT-002** If a minimum-zone fit is requested for a primitive not in the defined-pairs table, then the system shall return `method_undefined` naming the primitive and method.
- **REQ-KRN.MZ.EXT-003** If angular or edge coverage is below the configured threshold, then the fit shall return a typed error rather than a zone.
- **REQ-KRN.MZ.EXT-004** When a command is committed, the system shall journal one `fit.mz_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.mz_cone@1`, `fit.mz_slot@1`, `fit.mz_rectangle@1`, `fit.mz_ellipse@1`, `fit.mz_curve2d@1` (W2, dry-run R1); the defined-pairs table as data.
- **Consumes:** `KRN.LSQ.AXIS`, `KRN.LSQ.PLAN2D`, `KRN.SOLV` (NLLS for the curve placement start, SEQ, CERT, MSTART).

## Decomposition hints

CONE, PLAN (slot and rectangle), ELL (ellipse), CURVE (planar curves), UNDEF (defined-pairs table and `method_undefined`).

## Open questions

- Which of these zones have a published definition in ISO 1101 or ISO 12180/12181 style specifications versus being project definitions; results without one need `reference_basis: none` (owned by `UNC`) [unverified].

- The planar-curve zone is defined here as the minimum-width band of constant width centred on a caller-given nominal curve under free in-plane rigid placement (form only, no datums). It needs a least-squares placement start, not a free-curve fit, so the dependency on `KRN.LSQ.CURVE` was replaced by `KRN.SOLV.NLLS`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.MZ.EXT.UNDEF | Minimum-zone defined-pairs table and method_undefined | 002 | none |
| KRN.MZ.EXT.CONE | Minimum-zone cone | 001 003 004 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.EXT.PLAN | Minimum-zone slot and rectangle | 001 003 004 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.EXT.ELL | Minimum-zone ellipse | 001 003 004 | GOM.CON.19, PW.CON.05 |
| KRN.MZ.EXT.CURVE | Minimum-zone planar curve about a nominal | 001 003 004 | GOM.CON.19, PW.CON.05 |
