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
built_in: [fid-gps]
satisfies: [REQ-GDT.ZONE-001]
covers: []
depends_on: [KRN.NUM, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **Crate:** `fid-gps` (module `zone::shape`). `layers.toml` places `fid-gps` in the model layer, so KNP-1 is not mandatory there; the module is still written to KNP-1 rules (no FMA, no `f64::min`/`f64::max`, square root through `fid-num`, fixed-order reductions, NaN an error) and its tests run on wasm32.

### Pinned choices (as built)

- **Pose.** `Pose { origin: P3, direction: P3 }` in the part frame (SI metres; `P3 = [f64; 3]`), the vector form the usage formulas are written in. `Pose::from_rigid(&Rigid)` takes it from a GEO.FRAME rigid transform (zone local frame to part frame): origin = the translation, direction = the third rotation column (image of local z), copied without arithmetic. Usage is evaluated directly on `r = p − o` in the part frame, not by first mapping points through the inverse transform: the two agree mathematically and the direct form rounds less.
- **Shape semantics.** Two planes: normal = pose direction, centred on the plane through the origin. Two lines: lines lie in the plane with unit normal `plane`; the pose direction is the in-plane direction across the lines and must satisfy `|n·plane| ≤ 1e-12` (`direction_not_in_plane`); a point's out-of-plane component is ignored. Cylinder: axis through the origin along the pose direction. Circle: centred at the origin in the plane with unit normal `plane`; the pose direction is not used. Sphere: centred at the origin; direction not used. Offset: input is one signed deviation per toleranced point (`ZoneInput::Deviations { points, deviations }`), usage `2·max|d_i − c|`; the pose is not used by the arithmetic. The pose direction is validated for every shape, so a pose is valid or not independently of the shape.
- **Sizes.** Full width or diameter in metres, never a half-width or radius; always ≥ 0.
- **Expressions.** `dot3(a, b) = (a0·b0 + a1·b1) + a2·b2`; `r = [p0 − o0, p1 − o1, p2 − o2]`; planes and lines `|dot3(n, r)|`; cylinder (`a = n`) and circle (`a = plane`) `h = dot3(a, r)`, `q_k = r_k − h·a_k`, `sqrt(dot3(q, q))`; sphere `sqrt(dot3(r, r))`; offset `|d_i − c|`. `sqrt` is `fid_num::f::sqrt` (correctly rounded). Directions are used as given (no renormalisation).
- **Maximum and ties.** Index order from point 0; a value replaces the running maximum only if strictly greater, so the lowest index wins ties; size = `2.0 × max` (exact).
- **Unit test.** `|sqrt(dot3(d, d)) − 1| ≤ 1e-12` for the pose direction and each plane normal (squared length overflow counts as length +∞).
- **Validation order** (first failure wins): input kind (`input_kind_mismatch`: Offset takes deviations, every other shape points); deviation count (`deviation_count_mismatch`); `empty_points`; more than 2^32 − 1 points (`too_many_points`, the driving index is a `u32`); any NaN in pose, shape parameters, `contains` size, points or deviations (`nan_input`); any ±∞ in the same (`non_finite_input`); pose direction unit, then plane normal unit (`non_unit_direction`); two lines in-plane check (`direction_not_in_plane`); a non-finite per-point value or doubled maximum (`overflow`, with the point index). `input_kind_mismatch`, `too_many_points`, `non_finite_input` and `direction_not_in_plane` are additions to the error list above, needed to make the enum and the API total.
- **contains.** `usage(...).size_m <= size`, exact; `size` NaN is `nan_input`, ±∞ is `non_finite_input`; a negative size contains nothing.
- **Test data.** `fixtures/gdt/zone/make_shapes_fixtures.py` (stdlib Python): `shapes_analytic.json` holds the exact-rational oracle values (rounded once to f64; bit-exact where the exact value is an f64, else ≤ 1 ulp) and the spec-labelled errors; `shapes_random_v1.json` holds, for 1 000 000 Philox4x32-10 points (seed `0x5eed`; point i = `2u − 1` from draws 3i..3i+2 of stream 0, deviation i from draw i of stream 1; pose origin (0.1, −0.2, 0.3), direction (2/7, 3/7, 6/7), plane (6/7, 2/7, −3/7), offset shift 0.1), the whole-set usage and the SHA-256 of the 1 000 chunk usages per shape, from the pinned expressions re-implemented in Python floats. The 10 000-case property test uses seed `0x5eed`, stream 2.

## Test fixtures

Analytic fixtures with dyadic coordinates (exactly representable) and seeded random sets, generated in-repo; CC0.

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), module `zone::shape`, implement `ZoneShape` (TwoPlanes, TwoLines{plane}, Cylinder, Circle{plane}, Sphere, Offset{disposition_shift}) with `usage(&self, pose: &Pose, pts: &[P3] | deviations: &[f64]) -> Result<Usage, ZoneError>` and `contains(&self, pose, size, pts) -> Result<bool, ZoneError>`. Usage is the smallest centred zone size containing all points: 2·max|n·(p−o)| (planes and lines), 2·max distance to axis (cylinder), 2·max in-plane distance to centre (circle), 2·max distance to centre (sphere), 2·max|d_i − c| (offset). Return the driving index (lowest on ties). `contains` compares exactly. Errors: `empty_points`, `non_unit_direction` (|‖n‖−1| > 1e-12), `nan_input`, `deviation_count_mismatch`. Tests: the acceptance criteria, including a 10 000-case seeded property test and cross-platform bit identity. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` for sqrt and transcendentals, fixed-order reductions, total-order max, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
