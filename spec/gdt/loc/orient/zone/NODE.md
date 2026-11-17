---
id: GDT.LOC.ORIENT.ZONE
title: Orientation zone evaluation with fixed direction and free location
level: 4
parent: GDT.LOC.ORIENT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.ORIENT-001, REQ-GDT.LOC.ORIENT-002, REQ-GDT.LOC.ORIENT-003, REQ-GDT.LOC.ORIENT-004]
covers: [GOM.GDT.05, GOM.GDT.06, GOM.GDT.07, PW.GDT.03]
depends_on: [GDT.LOC.DERIV.TYPES, GDT.FORM.STRAIGHT.LINEEL, GDT.ZONE.ZTYPE, GDT.ZONE.POSE, GDT.DRF.BUILD, KRN.ENV]
uses_artifacts: [UNC.ORS.GDT]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.LOC.ORIENT.ZONE: Orientation zone evaluation with fixed direction and free location

## Purpose

The one evaluation path behind parallelism, perpendicularity and angularity. The zone's direction is fixed in the DRF (the toleranced feature's nominal direction expressed in the DRF, which carries the basic angle); its location floats. For a planar surface or an axis with a width zone the value is the spread of the points along the fixed zone normal; for an axis with a diameter zone it is the diameter of the smallest circle containing the axis points projected along the fixed direction; for line elements it is the worst per-element spread. When the DRF leaves a rotation of the zone free, the row decides: minimise over it through the zone-pose solver, or refuse.

## Scope

**In:** `orientation_value(element, zone_kind, zone_direction_in_drf, drf, free_rotation_policy) -> OrientValue`; toleranced elements: planar surface points, `DerivedElement::Axis` (points sampled along the axis extent at the chain's sample count) or any derived element's points, line elements from `GDT.FORM.STRAIGHT.LINEEL`; zone kinds: two parallel planes, cylinder of fixed direction, two parallel lines per element; zone direction from the nominal geometry in DRF coordinates; free-rotation handling (`optimize` via `GDT.ZONE.POSE` with the free DoF list from the DRF's DoF table, or `unsupported`); extreme point indices.
**Out:** commands, MMC and payload (owned by `GDT.LOC.ORIENT.CMDS`); tangent-plane evaluation (owned by `GDT.LOC.ORIENT.TANG`); derivation (owned by `GDT.LOC.DERIV`); zone-pose optimisation (owned by `GDT.ZONE.POSE`); the free-rotation policy value (owned by DMT rows).

## Requirements

- **REQ-GDT.LOC.ORIENT.ZONE-001** For a two-parallel-planes zone with fixed unit normal n, the value shall be max(n·p) − min(n·p) over the toleranced points, with the indices of both extremes (lowest index on ties).
- **REQ-GDT.LOC.ORIENT.ZONE-002** For a cylindrical zone with fixed direction d, the value shall be the diameter of the minimum circumscribed circle of the points projected onto the plane perpendicular to d, computed with the KRN minimum circumscribed 2D circle fit.
- **REQ-GDT.LOC.ORIENT.ZONE-003** For line elements, the value shall be the maximum over elements of the per-element spread perpendicular to the fixed in-plane zone direction, with the attaining element reported.
- **REQ-GDT.LOC.ORIENT.ZONE-004** Parallelism, perpendicularity and angularity shall call this one function, differing only in the zone direction input, so angularity at 0 and 90 degrees equals parallelism and perpendicularity bit for bit.
- **REQ-GDT.LOC.ORIENT.ZONE-005** If the DRF's DoF table leaves a rotation of the zone direction free, then the function shall minimise the value over that rotation through `GDT.ZONE.POSE` when the chain's policy is `optimize`, and return `UNSUPPORTED{reason: free_rotation}` when it is `unsupported` or absent.
- **REQ-GDT.LOC.ORIENT.ZONE-006** Values shall match every ORS-GDT orientation case in the P1a subset (planes and axes to planar datums) within its stated tolerance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/orient_plane_tilted.json` (a plane tilted 0.0002 rad about x over a 50 mm extent, datum plane A, exact points), when parallelism is evaluated, then value = 50 mm × sin(0.0002) within 1e-12 m.
- **AC-2** Given fixture `orient_axis_bowed.json` (hole axis points on a known circle when projected), when perpendicularity with a diameter zone is evaluated, then value = that circle's diameter within the KRN fit's certified tolerance.
- **AC-3** Given the same plane fixture evaluated as angularity with basic angle 0 and as parallelism, then results are bit-identical; likewise 90 degrees versus perpendicularity.
- **AC-4** Given a perpendicularity of a plane to a single datum plane (free rotation about the datum normal) and policy `unsupported`, then `UNSUPPORTED{reason: free_rotation}`; with policy `optimize` and a zone-pose stub, then the stub's minimum is returned.
- **AC-5** (ORS) Every P1a ORS-GDT orientation case passes when published.
- **AC-6** (determinism) Bit-identical outputs on all CI platforms for AC-1..AC-3.

## Design

- **Registry command(s):** none (kernel function).
- **Data structures:** `OrientZoneKind { TwoPlanes, Cylinder, LineElements }`; `OrientValue { value_m, extremes, attaining_element?, flags }`; `FreeRotationPolicy { Optimize, Unsupported }`.
- **Algorithm:** dot products in index order with total-order min and max; projection then `fit.mcc_circle2d`; line elements via the extractor; free rotation via `solve_zone` with the free DoF list.
- **Errors:** `UNSUPPORTED{reason}`, `non_unit_direction`, `empty_points`, `nan_input`, pass-through fit and solver errors.
- **Determinism:** D0 for planes and line elements; D3 when the MCC fit or the zone-pose solver runs (recorded).
- **Performance budget:** 1 M points < 20 ms for the planar case [unverified].
- **Crate:** `fid-gps` (module `loc::orient`).

## Test fixtures

Analytic tilted planes, bowed axes and line-element strips (CC0); ORS-GDT suite from `UNC.ORS.GDT`.

## Build prompt

> In crate `fid-gps` (kernel), module `loc::orient`, implement `orientation_value(element, zone_kind, zone_direction_in_drf, drf, policy)`. Two-planes zone: value = max(n·p) − min(n·p) (index order, total-order min/max, report extreme indices, lowest on ties). Cylinder zone: project points along d and take the diameter of the KRN minimum circumscribed 2D circle. Line elements: extract with the line-element extractor and take the worst per-element spread perpendicular to the in-plane zone direction. One function for all three orientation symbols, so angularity at 0 or 90 degrees is bit-identical to parallelism or perpendicularity. If the DRF's DoF table leaves the zone direction a free rotation, minimise through `solve_zone` when the chain says `optimize`, else return `UNSUPPORTED{reason: free_rotation}`. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- The axis sample count along the extent is a chain parameter; if rows prefer evaluating the axis endpoints only, that is a parameter value, not a code change.

## Children

_Leaf: none._
