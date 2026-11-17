---
id: GDT.ZONE.ZTYPE
title: Tolerance-zone shapes and fixed-pose usage
level: 3
parent: GDT.ZONE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE-001]
covers: []
depends_on: [KRN.NUM, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.ZONE.ZTYPE: Tolerance-zone shapes and fixed-pose usage

## Purpose

The geometric vocabulary of the zone engine: each zone shape, given a pose and toleranced points, answers two pure questions: how large must the zone be to contain the points at this pose (usage), and which point drives it. The optimizer (`GDT.ZONE.POSE`) varies the pose; the tolerance families choose the shape. Keeping shapes as small closed-form functions makes them exhaustively testable and reusable by every family.

## Scope

**In:** zone shapes: two parallel planes, two parallel lines in a plane, cylinder, circle (2D), sphere, uniform offset of a nominal surface or line given signed deviations with a disposition shift; per shape `usage(pose, points) -> (size, driving_index)` and `contains(pose, size, points) -> bool`; zone sizes in SI metres; sign and size conventions documented per shape.
**Out:** pose optimization (owned by `GDT.ZONE.POSE`); non-uniform profile zones and their definitions (owned by `GDT.PROF.ZDEF`); boundary (virtual-condition) zones (owned by `GDT.ZONE.MATL`); which shape a symbol uses (owned by DMT rows via the families).

## Requirements

- **REQ-GDT.ZONE.ZTYPE-001** For each shape, `usage` shall return the smallest zone size, centred as the shape defines at the given pose, that contains all points, together with the index of a point that attains it (lowest index on ties).
- **REQ-GDT.ZONE.ZTYPE-002** Usage shall be: two planes or two lines, 2·max|n·(p − o)|; cylinder, 2·max distance to the axis; circle, 2·max in-plane distance to the centre; sphere, 2·max distance to the centre; uniform offset, 2·max|d_i − c| for signed deviations d_i and disposition shift c.
- **REQ-GDT.ZONE.ZTYPE-003** `contains(pose, size, points)` shall return true exactly when `usage(pose, points).size ≤ size`, using an exact comparison of f64 values with no tolerance.
- **REQ-GDT.ZONE.ZTYPE-004** If the point set is empty, a direction is not unit within 1e-12, or any coordinate is NaN, then the function shall return `empty_points`, `non_unit_direction` or `nan_input`.
- **REQ-GDT.ZONE.ZTYPE-005** All reductions shall run in fixed index order so results are bit-identical across platforms (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/shapes_analytic.json` (for each shape, points placed at known distances, including one point exactly on the boundary), when `usage` runs, then each size matches the expected value exactly (bit-for-bit where the inputs are dyadic) or within 1 ulp otherwise, and the driving index matches.
- **AC-2** Given two points tying for the maximum, then the lower index is returned.
- **AC-3** Given `contains` with the size from AC-1, then true; with that size's next lower f64, then false.
- **AC-4** (error path) Given an empty set, a direction of length 1.1, or a NaN coordinate, then the corresponding typed error is returned.
- **AC-5** Given 1 M random points from seed `0x5eed`, then the output bits are identical on all CI platforms, and a property test (10 000 seeded cases) confirms `contains(pose, usage(...).size, points)` is always true.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `ZoneShape { TwoPlanes, TwoLines{plane}, Cylinder, Circle{plane}, Sphere, Offset{disposition_shift} }`; `Pose` from GEO.FRAME; `Usage { size_m: f64, driving_index: u32 }`.
- **Algorithm:** transform points by the pose inverse, compute the per-point distance in closed form, take a total-order max in index order, double it (exact in binary floating point).
- **Errors:** `empty_points`, `non_unit_direction`, `nan_input`, `deviation_count_mismatch` (offset).
- **Determinism:** D0.
- **Performance budget:** 1 M points per call < 10 ms single-threaded [unverified].
- **Crate:** `fid-gps` (module `zone::shape`).

## Test fixtures

Analytic fixtures with dyadic coordinates (exactly representable) and seeded random sets, generated in-repo; CC0.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), module `zone::shape`, implement `ZoneShape` (TwoPlanes, TwoLines{plane}, Cylinder, Circle{plane}, Sphere, Offset{disposition_shift}) with `usage(&self, pose: &Pose, pts: &[P3] | deviations: &[f64]) -> Result<Usage, ZoneError>` and `contains(&self, pose, size, pts) -> Result<bool, ZoneError>`. Usage is the smallest centred zone size containing all points: 2·max|n·(p−o)| (planes and lines), 2·max distance to axis (cylinder), 2·max in-plane distance to centre (circle), 2·max distance to centre (sphere), 2·max|d_i − c| (offset). Return the driving index (lowest on ties). `contains` compares exactly. Errors: `empty_points`, `non_unit_direction` (|‖n‖−1| > 1e-12), `nan_input`, `deviation_count_mismatch`. Tests: the acceptance criteria, including a 10 000-case seeded property test and cross-platform bit identity. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` for sqrt and transcendentals, fixed-order reductions, total-order max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
