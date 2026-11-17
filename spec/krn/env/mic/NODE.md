---
id: KRN.ENV.MIC
title: Maximum inscribed fits with trust region
level: 3
parent: KRN.ENV
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV-001, REQ-KRN.ENV-003, REQ-KRN.ENV-005, REQ-KRN.ENV-006, REQ-KRN.ENV-008, REQ-KRN.ENV-009]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.LSQ.RND, KRN.LSQ.AXIS, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.ENV.XCHK]
cli: [fid fit mic-circle2d, fid fit mic-sphere, fid fit mic-cylinder]
mcp: [fit_mic_circle2d, fit_mic_sphere, fit_mic_cylinder]
journal: [fit.mic_circle2d@1, fit.mic_sphere@1, fit.mic_cylinder@1]
updated: 2026-11-17
---

# KRN.ENV.MIC: Maximum inscribed fits with trust region

## Purpose

The largest circle, sphere or cylinder that fits inside the points: the mating envelope of a hole. It is a largest-empty-ball problem and always non-convex. On sparse or partial data an unconstrained centre can escape to infinity, so the centre or axis is held in a trust region around the least-squares solution and an optimum on the region's boundary is a typed error, not a result.

## Scope

**In:**
- `fit.circle2d.mic`, `fit.sphere.mic`, `fit.cylinder.mic` by sequential linearisation and multi-start
- The trust region (radius as a recorded fraction of the least-squares radius) and the `unbounded_envelope` error
- Slot and rectangle inscribed envelopes where defined
- The CI agreement job between the 2D conic path and the Voronoi path in `KRN.ENV.XCHK`
- Registry commands and spec cards

**Out** (and which node owns it instead):
- The Voronoi MIC algorithm (`KRN.ENV.XCHK`)
- Actual mating envelope logic under a dialect (`GDT`)

## Requirements

- **REQ-KRN.ENV.MIC-001** The module shall compute maximum inscribed 2D circle, sphere and cylinder fits implementing the `Fit` trait, with contact set, local optimality check and `local_optimum_only` whenever global optimality is not proven.
- **REQ-KRN.ENV.MIC-002** The centre or axis shall be constrained to a trust region around the least-squares solution whose size is an explicit recorded parameter.
- **REQ-KRN.ENV.MIC-003** If the optimum lies on the trust-region boundary, then the fit shall return `unbounded_envelope` with the boundary distance and no envelope.
- **REQ-KRN.ENV.MIC-004** For the 2D circle, CI shall compare the conic path with `KRN.ENV.XCHK` on every reference case and shall fail if they differ by more than 1e-9 m [unverified].
- **REQ-KRN.ENV.MIC-005** When a command is committed, the system shall journal one `fit.mic_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.mic_circle2d@1`, `fit.mic_sphere@1`, `fit.mic_cylinder@1` (W2, dry-run R1); envelopes with flags for `GDT` (Rule #1, MMC bonus).
- **Consumes:** `KRN.LSQ`, `KRN.SOLV`, `KRN.ENV.XCHK` (CI agreement).

## Decomposition hints

TR (trust region and `unbounded_envelope`), CIRC (2D circle and agreement job), SPH (sphere), CYL (cylinder), PLAN (slot and rectangle).

## Open questions

- Default trust-region size must be tuned on the ORS-SIZE partial-arc cases from `UNC` [unverified]. `KRN.ENV.MIC.TR` makes the size a required, recorded parameter so tuning never changes a past result silently.
- Slot and rectangle inscribed envelopes: no accepted definition is cited yet [unverified]; `KRN.ENV.MIC.TR` lists them as `method_undefined` in the MIC defined-pairs table until one is.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.ENV.MIC.TR | Trust region, unbounded-envelope error and MIC defined pairs | 002 003 | none |
| KRN.ENV.MIC.CIRSPH | Maximum inscribed circle and sphere | 001 004 005 | GOM.CON.20, PW.CON.05 |
| KRN.ENV.MIC.CYL | Maximum inscribed cylinder | 001 005 | GOM.CON.20, PW.CON.05 |
