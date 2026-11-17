---
id: KRN.CONS.ELEM.SHAPE
title: Definitional cylinder, cone, slot and rectangle builds
level: 4
parent: KRN.CONS.ELEM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.ELEM-001, REQ-KRN.CONS.ELEM-004, REQ-KRN.CONS.ELEM-005]
covers: [GOM.CON.05, GOM.CON.06, GOM.CON.07, GOM.CON.08, PCD.CON.01, PW.CON.04]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.LA, KRN.NUM.FN, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct build-cylinder, fid construct build-cone, fid construct build-slot, fid construct build-rectangle]
mcp: [construct_build_cylinder, construct_build_cone, construct_build_slot, construct_build_rectangle]
journal: [construct.build_cylinder@1, construct.build_cone@1, construct.build_slot@1, construct.build_rectangle@1]
updated: 2026-11-17
---

# KRN.CONS.ELEM.SHAPE: Definitional cylinder, cone, slot and rectangle builds

## Purpose

Builds compound and axial primitives from other elements by definition: a cylinder from an axis and a radius or from a circle and a length, a cone from two coaxial circles or from an axis, an apex and a half-angle, a slot from two equal circles, and a rectangle from two pairs of parallel lines or from a centre, direction and sizes.

## Scope

**In:** the variant table below, each a `Construction` with algorithm ID `construct.build_<primitive>.<variant>@1.0.0`; coaxiality, parallelism and equal-radius checks with explicit thresholds; the four registry commands; spec cards.

| Primitive | Variants |
|---|---|
| cylinder | `axis_radius` (line plus radius), `circle_length` (circle plus signed length along its normal), `two_circles` (coaxial, equal radius) |
| cone | `two_circles` (coaxial, different radii), `axis_apex_angle` (line, point on it, half-angle) |
| slot | `two_circles` (equal radius, coplanar), `centre_line_sizes` (line in a plane plus length and width) |
| rectangle | `two_line_pairs` (two pairs of parallel coplanar lines, pairs orthogonal), `centre_dir_sizes` (point, line in a plane, length and width) |

**Out:** point, line, plane, circle and sphere (`KRN.CONS.ELEM.PRIM`); fitted versions (`KRN.LSQ.AXIS`, `KRN.LSQ.PLAN2D`); best fit through centres (`KRN.CONS.ELEM.FITC`).

## Requirements

- **REQ-KRN.CONS.ELEM.SHAPE-001** The module shall implement every variant in the table as a `Construction` with documented input roles, giving the exact geometric definition.
- **REQ-KRN.CONS.ELEM.SHAPE-002** If inputs required to be coaxial, coplanar, parallel, orthogonal or of equal radius differ beyond the matching `ConstructParams` threshold, then the build shall return `IllConditioned` with the measured angle, distance or radius difference.
- **REQ-KRN.CONS.ELEM.SHAPE-003** If inputs are fewer than the variant needs or of the wrong kind, then the build shall return `InputCount` or `WrongInputKind`.
- **REQ-KRN.CONS.ELEM.SHAPE-004** Where a variant averages two nearly equal inputs (for example two equal-radius circles), it shall use the arithmetic mean in input order and record the measured difference in the result diagnostics.
- **REQ-KRN.CONS.ELEM.SHAPE-005** When a command is committed, the system shall journal one `construct.build_<primitive>@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/shape-defs-v1` (300 seeded cases per variant with closed-form expected results), when each `fid construct build-<primitive> --variant <v> --json` runs, then parameters agree within 1e-12 relative [unverified tolerance].
- **AC-2** Given two circles whose axes differ by 1e-3 rad with an angular threshold of 1e-6 rad, when `build-cylinder --variant two_circles` runs, then it returns `IllConditioned{measure: "coaxiality"}` and journals nothing.
- **AC-3** Given a cone `two_circles` with equal radii, then it returns `IllConditioned{measure: "radius_difference"}` (a cylinder, not a cone).
- **AC-4** Given each variant, when the `KRN.CONS.DECL` harness runs, then repeat and replace-then-evaluate are bit-identical, and digests match on x86-64, ARM64 and wasm32.
- **AC-5** Given a committed case, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `construct.build_cylinder@1`, `construct.build_cone@1`, `construct.build_slot@1`, `construct.build_rectangle@1`; W2 (dry-run R1). Params: `variant`, ordered input element handles, sizes with quantity kind and unit, thresholds. Result: `FittedFeature` of the primitive plus diagnostics.
- **Algorithm:** closed forms; cone half-angle by `fid_num::f::atan2` of radius difference over axial distance; rectangle from the four pairwise line intersections in the common plane.
- **Errors:** `InputCount`, `WrongInputKind`, `IllConditioned`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 5 microseconds per build [unverified until measured].
- **Crate:** `fid-construct` (module `elem_shape`).

## Test fixtures

`fixtures/cons/shape-defs-v1`: seeded generator with closed-form expectations and threshold-edge cases, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `elem_shape` implementing the `Construction` trait for cylinder (`axis_radius`, `circle_length`, `two_circles`), cone (`two_circles`, `axis_apex_angle`), slot (`two_circles`, `centre_line_sizes`) and rectangle (`two_line_pairs`, `centre_dir_sizes`), algorithm IDs `construct.build_<primitive>.<variant>@1.0.0`. Check coaxiality, coplanarity, parallelism, orthogonality and radius equality against explicit `ConstructParams` thresholds, returning `IllConditioned{measure, value, threshold}`; average nearly equal inputs by arithmetic mean in input order and record the difference. Add registry data rows `construct.build_cylinder@1`, `construct.build_cone@1`, `construct.build_slot@1`, `construct.build_rectangle@1` (W2, dry-run R1, D0; CLI `fid construct build-<primitive>`; MCP `construct_build_<primitive>`; one journal event each with algorithm ID, input element IDs, parameter digest, output digest) and spec cards. Tests: `fixtures/cons/shape-defs-v1`, coaxiality and equal-radius errors, input-count errors, recompute harness, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.ELEM.SHAPE.

## Open questions

- Whether `cylinder.circle_length` takes the length along the circle normal or along the opposite direction by default; this leaf requires the sign explicitly.

## Children

_Leaf: none._
