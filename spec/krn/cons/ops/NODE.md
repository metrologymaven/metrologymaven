---
id: KRN.CONS.OPS
title: Construction operations on element pairs
level: 3
parent: KRN.CONS
kind: module
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-001, REQ-KRN.CONS-002, REQ-KRN.CONS-004, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [GOM.CON.14, GOM.CON.15, GOM.CON.16, GOM.CON.17, PCD.CON.01, PW.CON.04]
depends_on: [KRN.CONS.DECL]
cli: [fid construct intersect-elements, fid construct project-element, fid construct offset-element, fid construct midpoint-elements, fid construct symmetry-elements, fid construct bisect-elements]
mcp: [construct_intersect_elements, construct_project_element, construct_offset_element, construct_midpoint_elements, construct_symmetry_elements, construct_bisect_elements]
journal: [construct.intersect_elements@1, construct.project_element@1, construct.offset_element@1, construct.midpoint_elements@1, construct.symmetry_elements@1, construct.bisect_elements@1]
updated: 2026-11-16
---

# KRN.CONS.OPS: Construction operations on element pairs

## Purpose

The geometric operations that build a new element from one or two existing ones: intersection, projection, offset, midpoint and mid-plane, symmetry element and bisector, for every pair of point, line, plane, circle, cylinder, cone and sphere where the result is defined. Each is a pure construction under the `KRN.CONS.DECL` contract with typed errors for undefined and ill-conditioned cases.

## Scope

**In:**
- Intersect: line-line (with a documented closest-point rule for skew lines), line-plane, plane-plane, plane-plane-plane, line-sphere, line-cylinder, plane-cylinder, plane-sphere, plane-cone, circle-circle, and similar defined pairs
- Project: point, line and circle onto a plane or a line
- Offset: point, line, plane and circle by a signed distance along a defined direction
- Midpoint and mid-plane; symmetry element of two like features; bisector of two lines or two planes
- An operation-pair support table served as data, with `Undefined` for every other pair
- Registry commands and spec cards per variant

**Out** (and which node owns it instead):
- Building a primitive from several elements by fit (`KRN.CONS.ELEM`)
- Projection onto meshes and CAD surfaces (`GEO.DIST`; used by `KRN.CONS.DERIV`)
- Relationship values (`KRN.CONS.REL`)

## Requirements

- **REQ-KRN.CONS.OPS-001** The module shall provide intersect, project, offset, midpoint, symmetry and bisector constructions for every pair in its published support table, each implementing the `Construction` trait.
- **REQ-KRN.CONS.OPS-002** If a pair is not in the support table, or inputs are parallel, coincident or tangent beyond the thresholds in `ConstructParams`, then the construction shall return `Undefined` or `IllConditioned` with the measured angle or distance.
- **REQ-KRN.CONS.OPS-003** When two lines are skew, the line-line intersection shall return the midpoint of the common perpendicular and report the skew distance, or return `Undefined` if the caller selects the strict rule.
- **REQ-KRN.CONS.OPS-004** Each operation shall pass the `KRN.CONS.DECL` recompute-equivalence harness on a seeded fixture set.
- **REQ-KRN.CONS.OPS-005** When a command is committed, the system shall journal one `construct.<verb_object>@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Interfaces

- **Offers:** the six commands (W2, dry-run R1); the support table as data for `AUTO` docs and `VIZ` menus.
- **Consumes:** `KRN.CONS.DECL`; fitted-feature types from `KRN.LSQ.TRAIT`.

## Decomposition hints

INTER (intersections, the largest group), PROJ (projections), OFFS (offsets), MID (midpoint, mid-plane, symmetry and bisector), TABLE (support table and `Undefined` behaviour).

## Open questions

- Incumbents differ on skew-line intersection (closest point on the first line, or midpoint of the common perpendicular) [unverified]; this node defaults to the midpoint and records the rule as a parameter.

## Children

_None yet: status `open`._
