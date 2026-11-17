---
id: ALN.RPS.CLASSIC
title: Classic 3-2-1 alignment and frame from geometry
level: 3
parent: ALN.RPS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.RPS-001, REQ-ALN.RPS-002, REQ-ALN.RPS-007]
covers: [GOM.ALN.04, PW.ALN.04, SA.XFM.03]
depends_on: [ALN.MODEL.ELEM, ALN.QA.REPORT, GEO.FRAME, KRN.LSQ, KRN.CONS]
side_effect: W2
determinism: D0
cli: [fid align run-321]
mcp: [align_run_321]
journal: [align.run_321@1]
updated: 2026-11-16
---

# ALN.RPS.CLASSIC: Classic 3-2-1 alignment and frame from geometry

## Purpose

The closed-form 3-2-1 alignment: a primary direction element locks three DoF (one translation, two rotations), a secondary element locks two (one rotation, one translation), and a tertiary element locks the last translation, producing a right-handed frame. It also offers the SpatialAnalyzer-style three-point frame (origin point, axis point, plane point). Inputs are already-fitted or constructed elements from KRN.

## Scope

**In:**
- `align.run_321@1` in two modes: `elements` (primary, secondary, tertiary elements) and `three_points` (origin, axis point, plane point)
- Extraction of a direction and a locating point from each supported element type
- Axis assignment (which part axis each element defines, and signs)
- Degeneracy checks and the right-handedness guarantee

**Out** (and which node owns it instead):
- Fitting the elements (`KRN.LSQ`) and constructions such as plane intersections (`KRN.CONS`)
- Iterative 3-2-1 (`ALN.RPS.ITER`); RPS (`ALN.RPS.LOCK`); vector-point methods (`ALN.RPS.VECTOR`)
- Datum-precedence alignment under a dialect (`ALN.DATUM`)

## Requirements

- **REQ-ALN.RPS.CLASSIC-001** In `elements` mode, the system shall take the primary axis from the primary element's direction (plane normal, cylinder or cone axis, or line direction), the secondary axis from the secondary direction projected onto the plane normal to the primary axis, and the third axis as their cross product, so that the rotation matrix has determinant +1 within 1e-15.
- **REQ-ALN.RPS.CLASSIC-002** In `elements` mode, the origin shall lie on the primary element's locating plane (or point) along the primary axis, on the secondary element's locating line along the axis normal to it in the primary plane, and at the tertiary element's locating point along the remaining axis.
- **REQ-ALN.RPS.CLASSIC-003** The user shall assign which part axis (+-X, +-Y, +-Z) the primary and secondary elements define (default +Z and +X), and any assignment that is not two distinct axes shall fail with `invalid_axis_assignment`.
- **REQ-ALN.RPS.CLASSIC-004** If the angle between the secondary direction and the primary axis is below a tolerance (default 0.1 degree [unverified]) or above 180 degrees minus it, then the command shall fail with `secondary_parallel_to_primary` and journal nothing.
- **REQ-ALN.RPS.CLASSIC-005** If the tertiary locating point lies within a tolerance (default 1e-6 m [unverified]) of the secondary locating line, then the command shall fail with `tertiary_on_secondary` and journal nothing.
- **REQ-ALN.RPS.CLASSIC-006** In `three_points` mode, the origin shall be the first point, the first axis shall point to the second point, and the plane of the first two axes shall contain the third point; if the three points are collinear within 1e-6 m [unverified], then the command shall fail with `collinear_points`.
- **REQ-ALN.RPS.CLASSIC-007** The computation shall be closed-form in the CPU kernel under KNP-1 and declare class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/rps/block-321` (a block whose LSQ top plane, front-edge line and end point are exact, with a known part pose), when `fid align run-321 --primary top --secondary front --tertiary end --json` runs, then the alignment reproduces the known pose within 1e-12 m on the block corners and the rotation determinant is +1 within 1e-15.
- **AC-2** Given the same fixture with axis assignment primary +Y, secondary -X, then the frame matches the fixture's second expected pose within 1e-12 m.
- **AC-3** (error path) Given a secondary line parallel to the primary normal, then the command fails with `secondary_parallel_to_primary` and journals nothing.
- **AC-4** (error path) Given a tertiary point on the secondary line, then the command fails with `tertiary_on_secondary`.
- **AC-5** Given `three_points` mode with the fixture's three corner points, then the frame equals the expected frame within 1e-12 m; with collinear points, it fails with `collinear_points`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode on another platform, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `align.run_321@1` (W2). Params: `mode: elements | three_points`; `primary`, `secondary`, `tertiary` (element IDs) or `points: [3]` (coordinates or point elements); `primary_axis`, `secondary_axis` (signed axis); `parent`; tolerances. Result: alignment ID.
- **Data structures:** `DirectionSource { direction: UnitVec3, locus: Locus (Plane | Line | Point) }` extracted per element type: plane (normal, plane), line (direction, line), cylinder or cone (axis, axis line), circle (normal, centre), point (none, point), sphere (none, centre). Directions oriented by the element's nominal when present, else by the user sign.
- **Algorithm:** Gram-Schmidt in fixed order (primary, secondary projected and normalised, third by cross product); origin by solving the 3x3 linear system of the three locus conditions in the new axes (fid-num). Residual report: distances of each locus condition, all zero by construction, plus input fit residual references.
- **Errors:** `invalid_axis_assignment`, `secondary_parallel_to_primary`, `tertiary_on_secondary`, `collinear_points`, `unsupported_element_type`, `no_direction` (element type gives no direction where one is needed).
- **Determinism:** D0 (closed form, KNP-1).
- **Performance budget:** under 50 µs per call [unverified until measured].
- **Crate:** solver in `fid-align-solve`, command in `fid-align` (provisional names).

## Test fixtures

`fixtures/aln/rps/block-321`: seeded generator (seed 8), a 200 x 100 x 50 mm block with exact LSQ plane, line and point elements, two known poses and the expected frames for two axis assignments. Project-generated, CC-BY-4.0.

## Build prompt

> Implement classic 3-2-1 alignment: the solver in kernel crate `fid-align-solve`, the command `align.run_321@1` in `fid-align` (class W2; CLI `fid align run-321 --primary <id> --secondary <id> --tertiary <id> [--primary-axis +Z] [--secondary-axis +X] [--mode three_points --points ...] --json`; MCP `align_run_321`; journal `align.run_321@1`).
> Requirements: elements mode takes the primary axis from the primary element's direction (plane normal, cylinder or cone axis, line direction), the secondary axis from the secondary direction projected perpendicular to the primary axis, the third by cross product (determinant +1 within 1e-15); origin from the primary locus along the primary axis, the secondary locus along the in-plane axis normal to it, and the tertiary point along the remaining axis. Axis assignment defaults +Z and +X and must name two distinct axes (`invalid_axis_assignment`). Errors journal nothing: `secondary_parallel_to_primary` (within 0.1 degree), `tertiary_on_secondary` (within 1e-6 m), `collinear_points` in three-points mode (origin, axis point, plane point), `unsupported_element_type`, `no_direction`. Create the alignment through the element wrapper with a residual report.
> Acceptance: fixture `fixtures/aln/rps/block-321` (seed 8); tests for known-pose recovery within 1e-12 m, alternative axis assignment, both degeneracy errors, three-points mode and its collinear error, cross-platform exact replay IDENTICAL.
> Constraints: KNP-1 (no FMA, fid-num for sqrt and trig, fixed-order arithmetic, NaN is an error); no panic, no unwrap/expect, no unsafe; determinism D0.

## Open questions

- REQ-005 follows the parent (REQ-ALN.RPS-002). Geometrically, a tertiary point on the secondary line still fixes the last translation; the check guards against a point taken on the wrong face. A domain expert should confirm whether it should be a warning instead.

## Children

_Leaf: none._
