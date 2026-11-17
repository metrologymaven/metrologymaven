---
id: KRN.LSQ.CURVE
title: Least-squares paraboloid, polyline and spline fits
level: 3
parent: KRN.LSQ
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.LSQ-002, REQ-KRN.LSQ-003, REQ-KRN.LSQ-006, REQ-KRN.LSQ-007, REQ-KRN.LSQ-009, REQ-KRN.LSQ-010]
covers: [GOM.CON.18, PW.CON.03, SA.REL.01]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.INIT, KRN.SOLV.NLLS, KRN.SOLV.MSTART, GEO.NURBS.EVAL]
cli: [fid fit ls-paraboloid, fid fit ls-polyline, fid fit ls-spline]
mcp: [fit_ls_paraboloid, fit_ls_polyline, fit_ls_spline]
journal: [fit.ls_paraboloid@1, fit.ls_polyline@1, fit.ls_spline@1]
updated: 2026-11-17
---

# KRN.LSQ.CURVE: Least-squares paraboloid, polyline and spline fits

## Purpose

The remaining least-squares shapes: the paraboloid of revolution (antennas and reflectors, a SpatialAnalyzer staple), and free curves through measured points, as polylines with a fixed vertex count or smoothing splines. None has an NMI reference, and the spline fits need explicit smoothing parameters, so they are grouped and validated against open reference suites.

## Scope

**In:**
- Paraboloid of revolution: vertex, axis, focal length; multi-start for the axis
- Polyline fit with a given vertex count or vertex positions along the curve
- Planar and 3D smoothing-spline curve fits with explicit knot placement and smoothing parameter
- Registry commands, spec cards, typed errors

**Out** (and which node owns it instead):
- Spline and NURBS evaluation primitives (`GEO.NURBS`)
- Section curves and edges extracted from scans (`SCAN`) and constructed from nominals (`KRN.CONS`)
- Profile tolerance evaluation against a curve (`GDT`)

## Requirements

- **REQ-KRN.LSQ.CURVE-001** The module shall fit a paraboloid of revolution by orthogonal distance, with vertex, axis and focal length as parameters and the axis parameterised without singularities.
- **REQ-KRN.LSQ.CURVE-002** The module shall fit a polyline with a caller-given vertex count to ordered points by minimising squared orthogonal distance to the segments.
- **REQ-KRN.LSQ.CURVE-003** The module shall fit a smoothing spline curve with caller-given degree, knot placement rule and smoothing parameter, all recorded in the result, with no default hidden from the caller's record.
- **REQ-KRN.LSQ.CURVE-004** If the points are not ordered along the curve for a polyline or spline fit, or the paraboloid's focal length is ill-determined beyond the condition limit, then the fit shall return a typed error.
- **REQ-KRN.LSQ.CURVE-005** When a command is committed, the system shall journal one `fit.ls_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.ls_paraboloid@1`, `fit.ls_polyline@1`, `fit.ls_spline@1` (W2, dry-run R1).
- **Consumes:** `KRN.LSQ.TRAIT`, `KRN.LSQ.INIT`, `KRN.SOLV.NLLS`, `KRN.SOLV.MSTART`; spline basis evaluation may reuse `GEO.NURBS` math only if it is moved into the kernel graph (open question).

## Decomposition hints

PARAB (paraboloid of revolution), POLY (polyline), SPLINE (smoothing spline curves).

## Open questions

- **Resolved by CONSTITUTION §2.1 (D-034):** `GEO.NURBS.EVAL` is an L0 geometry library in the kernel graph (crate `fid-geo-nurbs`, KNP-1 applies), so `KRN.LSQ.CURVE.SPLINE` uses its B-spline basis directly and no second evaluator is written.
- Phase: P1b, after scan-to-CAD work starts; SpatialAnalyzer users need paraboloids in P3 at the latest [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.LSQ.CURVE.PARAB | Least-squares paraboloid of revolution | 001 004 005 | GOM.CON.18, SA.REL.01 |
| KRN.LSQ.CURVE.POLY | Least-squares polyline with a given vertex count | 002 004 005 | GOM.CON.18, PW.CON.03 |
| KRN.LSQ.CURVE.SPLINE | Least-squares smoothing spline curves | 003 004 005 | PW.CON.03 |
