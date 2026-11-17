---
id: GDT.FORM.STRAIGHT.LINEEL
title: Line-element extraction and line-element straightness
level: 4
parent: GDT.FORM.STRAIGHT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.FORM.STRAIGHT-001, REQ-GDT.FORM.STRAIGHT-004]
covers: [GOM.GDT.02, PW.GDT.02]
depends_on: [GDT.FORM.FCORE, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.FORM.STRAIGHT.LINEEL: Line-element extraction and line-element straightness

## Purpose

Line-element straightness evaluates each surface line element separately as a 2D minimum-zone line problem and reports the worst. The extraction (which family of cutting planes, their spacing, the band of points assigned to each, the line direction in each plane) is given explicitly by the resolved chain; this leaf applies it mechanically and never chooses defaults, so expert defaults can arrive later as DMT content without code changes. The extraction function is also used by line-element orientation in `GDT.LOC.ORIENT.ZONE`.

## Scope

**In:** `LineElementParams { plane_normal, origin, spacing, count_or_range, band_half_width, line_direction }` read from the chain; `extract_line_elements(points, params) -> [LineElement { index, plane, point_indices, points_2d }]`; per-element evaluation through `GDT.FORM.FCORE` with element `Line2d`; payload: maximum element value, per-element values, the element attaining the maximum, skipped elements with reasons.
**Out:** form policy (owned by `GDT.FORM.FCORE`); the defaults for the parameters (owned by DMT rows via `GDT.RES`); derived-median-line straightness (owned by `GDT.FORM.STRAIGHT.DERIVED`); the command (owned by `GDT.FORM.STRAIGHT.CMD`).

## Requirements

- **REQ-GDT.FORM.STRAIGHT.LINEEL-001** The extractor shall assign a point to line element k exactly when its signed distance to the k-th cutting plane (origin + k·spacing along the plane normal) is within the band half-width, project it onto that plane, and express it in 2D coordinates along the chain's line direction.
- **REQ-GDT.FORM.STRAIGHT.LINEEL-002** Each element shall be evaluated through the form core with element `Line2d`, and the result shall report every element's value, the maximum and the index of the element attaining it (lowest index on ties).
- **REQ-GDT.FORM.STRAIGHT.LINEEL-003** If an element has fewer points than the row minimum, then it shall be listed in `skipped_elements` with `too_few_points`, and if no element remains, then evaluation shall fail with `no_line_elements`.
- **REQ-GDT.FORM.STRAIGHT.LINEEL-004** If the chain lacks any extraction parameter, then the extractor shall return `extraction_params_missing{param}` rather than assume a value.
- **REQ-GDT.FORM.STRAIGHT.LINEEL-005** Values shall match the synthetic fixtures within 1e-12 m and, when run, the TraCIM Chebyshev 2D line test within its MPE [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/form/straight_lines_planted.json` (a planar strip with 5 line elements at 2 mm spacing, element 3 with planted straightness 0.006 mm, others 0.002 mm), when evaluated with matching parameters, then values are as planted within 1e-12 m, the maximum is 0.006 mm and the attaining element is 3.
- **AC-2** Given a band half-width that leaves element 5 with 1 point, then element 5 is skipped with `too_few_points` and the others are evaluated.
- **AC-3** (error path) Given a chain without `spacing`, then `extraction_params_missing{param: spacing}`.
- **AC-4** Given a KRN stub with MZ unavailable for `Line2d`, then each element carries `conservative_upper_bound` from the form core.
- **AC-5** (determinism) Outputs are bit-identical on all CI platforms.

## Design

- **Registry command(s):** none (kernel functions used by `GDT.FORM.STRAIGHT.CMD` and `GDT.LOC.ORIENT.ZONE`).
- **Data structures:** `LineElementParams`, `LineElement`, `LineElementPayload { max_value, attaining_element, per_element: [(k, value, flags)], skipped_elements }`.
- **Algorithm:** signed distances in index order; assignment by integer element index range; projection; 2D frame from plane normal and line direction (orthonormalised in fixed order); per-element form core.
- **Errors:** `extraction_params_missing{param}`, `no_line_elements`, `non_unit_direction`, `nan_input`.
- **Determinism:** D0.
- **Performance budget:** 1 M points, 100 elements < 300 ms [unverified].
- **Crate:** `fid-gps` (module `form::straightness::line_elements`).

## Test fixtures

Seeded synthetic strips with planted per-element extremes (CC0); TraCIM data only through the PTB service [unverified].

## Build prompt

> In crate `fid-gps` (kernel), module `form::straightness::line_elements`, implement `extract_line_elements(points, params)` and `evaluate_line_elements(points, params, chain, krn)`. Parameters come only from the chain (`plane_normal, origin, spacing, count_or_range, band_half_width, line_direction`; any missing gives `extraction_params_missing{param}`). Assign a point to element k when its signed distance to plane k is within the band, project, express in 2D along the line direction, and evaluate each element through `form::evaluate_form(FormElement::Line2d, ...)`. Report per-element values, the maximum and its element (lowest index on ties), and skipped elements with reasons; fail with `no_line_elements` if none remain. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- Default extraction parameters per callout are DMT content (D-009, D-016); until rows supply them, users or programs must state them explicitly in the characteristic's override.

## Children

_Leaf: none._
