---
id: KRN.CONS.AUTO.PAIR
title: Nominal-actual point pairing with gates and conflict rule
level: 4
parent: KRN.CONS.AUTO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-construct]
built_pr: 669
satisfies: [REQ-KRN.CONS.AUTO-002, REQ-KRN.CONS.AUTO-003]
covers: [GOM.CON.22]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.RED, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.CONS.AUTO.PAIR: Nominal-actual point pairing with gates and conflict rule

## Purpose

The pure function that decides which actual points belong to which nominal element. For each point it measures the orthogonal distance and normal angle to every candidate nominal primitive, applies explicit gates, resolves points claimed by more than one element by an explicit conflict rule, and marks an element unpaired when it keeps too few points. `KRN.CONS.AUTO.FACE` calls it; `SCAN` may call it for its own per-face extraction.

## Scope

**In:** `PairParams { distance_gate_m, normal_angle_gate_rad, min_points, conflict_rule }`; analytic orthogonal distance and surface normal for nominal plane, cylinder, cone, sphere, torus, and planar circle and slot boundaries (in-plane distance plus out-of-plane gate); optional per-element candidate index lists to bound the work; the conflict rules `nearest` (smallest distance, ties by smaller angle, then by element order) and `exclude_ambiguous`; the `Pairing` result.
**Out:** spatial queries over large scans (`SCAN`); eligibility and the command (`KRN.CONS.AUTO.FACE`); fitting the paired points (`KRN.LSQ`).

## Requirements

- **REQ-KRN.CONS.AUTO.PAIR-001** The function shall assign a point to a nominal element only if its orthogonal distance is at most `distance_gate_m` and, when the point has a normal, the angle between the point normal and the nominal normal is at most `normal_angle_gate_rad`.
- **REQ-KRN.CONS.AUTO.PAIR-002** When a point passes the gates of more than one element, the function shall resolve it by the `conflict_rule` parameter: `nearest` (smallest distance, then smallest angle, then lowest element index) or `exclude_ambiguous` (assigned to none and counted as ambiguous).
- **REQ-KRN.CONS.AUTO.PAIR-003** If an element receives fewer than `min_points` points after conflict resolution, then the function shall mark it `unpaired{count}` and release its points to no other element.
- **REQ-KRN.CONS.AUTO.PAIR-004** Every field of `PairParams` shall be required (no defaults inside the kernel) and part of the parameter digest.
- **REQ-KRN.CONS.AUTO.PAIR-005** For the same points, nominal elements and parameters, the `Pairing` result shall be bit-identical on x86-64, ARM64 and wasm32, and independent of whether candidate index lists are given, provided they contain every gated point.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/auto-pair-block-v1` (a block with 6 planes, 2 cylinders and 1 sphere, 200 k seeded points with normals, 5 micrometre noise), when pairing runs with gates 0.1 mm and 10 degrees, then every point is assigned to its generating element and the counts equal the fixture's expected file.
- **AC-2** Given the edge band of the same fixture (points within 0.05 mm of two planes), when `conflict_rule = nearest`, then each point goes to the nearer plane with ties resolved by element index; when `exclude_ambiguous`, they are counted as ambiguous and assigned to none.
- **AC-3** Given `min_points = 1000` and a cylinder with 400 gated points, then the cylinder is `unpaired{count: 400}`.
- **AC-4** Given AC-1 run with and without candidate index lists, and on x86-64, ARM64 and wasm32, then the `Pairing` encodings are byte-identical.
- **AC-5** (error path) Given a negative distance gate or a NaN coordinate, then the function returns `ConstructError::Num` or `InvalidParams` and no pairing.

## Design

- **Registry command(s):** none; a library function called by `construct.auto_from_cad@1`.
- **Data structures:** `NominalPrimitive` (plane, cylinder, cone, sphere, torus, circle-in-plane, slot-in-plane; SI metres); `PairParams`; `Pairing { per_element: Vec<ElementPairing { indices: Vec<u32>, status: Paired | Unpaired{count} }>, ambiguous: Vec<u32>, ungated: u32 }` with indices ascending.
- **Algorithm:** for each element in order, iterate its candidate indices (or all points) in ascending order; compute distance and angle with closed-form expressions (torus by the circle-of-centres construction); record gated claims in a per-point best-claim array (no hash maps); resolve conflicts in point order; apply `min_points`.
- **Errors:** `InvalidParams{field}`, `ConstructError::Num(NonFinite)`, `WrongInputKind` for an unsupported nominal kind.
- **Determinism:** D0; fixed iteration order, no threads, total-order comparisons.
- **Performance budget:** 1 M points against 200 elements with candidate lists in < 300 ms on reference hardware [unverified until measured].
- **Crate:** `fid-construct` (module `auto_pair`).

## Test fixtures

`fixtures/cons/auto-pair-block-v1`: seeded synthetic block generator with expected counts, CC-BY-4.0; generated by the test, not committed as a large file.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends only on `fid-num` and `fid-fit`), add module `auto_pair` with `NominalPrimitive` (plane, cylinder, cone, sphere, torus, circle-in-plane, slot-in-plane), `PairParams { distance_gate_m, normal_angle_gate_rad, min_points, conflict_rule: Nearest | ExcludeAmbiguous }` (all required, all in the parameter digest via the `contract` canonical encoding) and `fn pair(points: &PointsView, normals_required: bool, elems: &[NominalPrimitive], candidates: Option<&[Vec<u32>]>, p: &PairParams) -> Result<Pairing, ConstructError>`. Gate by orthogonal distance and normal angle; resolve multiple claims by `Nearest` (distance, then angle, then element index) or `ExcludeAmbiguous`; mark elements with fewer than `min_points` as `Unpaired{count}`. Iterate in fixed order with a per-point best-claim array; no HashMap/HashSet, no threads. Tests: `fixtures/cons/auto-pair-block-v1` seeded block (counts match expected), edge-band conflict cases for both rules, `min_points` case, byte-identical results with and without candidate lists and across x86-64/ARM64/wasm32, invalid-parameter and NaN error paths. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, fixed-order reductions via `fid_num::red`; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.AUTO.PAIR.

## Open questions

- Whether `SCAN` per-face extraction should reuse this function or own its own gating (it should reuse it, so scan and kernel agree); confirm when `SCAN` is expanded.
- Build choices in `fid_construct::auto_pair`, for review: the angle gate is oriented (`atan2(|n_p x n_nom|, n_p . n_nom)` in `[0, pi]`), and every curved primitive carries a `NormalSense` (`Outward` away from the axis, centre, circle of centres or centre segment; `Inward` for holes and pockets). The circle and slot out-of-plane gate reuses `distance_gate_m`; a separate parameter would need a spec change.
- Where the nominal normal is undefined (a point on a cylinder or cone axis, at a sphere centre, on a torus circle of centres, on a slot centre segment, or whose closest cone point is the apex), a point with a normal that passes the distance gate goes to no element and is listed in `Pairing.normal_undefined` (fail closed). Points of unpaired elements are listed in that element's `released` list, so every point is accounted for once.
- Gate floors: `distance_gate_m` must be at least `16 * f64::EPSILON` times the largest coordinate magnitude, and `normal_angle_gate_rad` at least `16 * f64::EPSILON`; `min_points` must be at least 1. `WrongInputKind` is not used: `NominalPrimitive` is a closed enum, so an unsupported kind cannot be passed.
- The performance budget is not yet measured.

## Children

_Leaf: none._
