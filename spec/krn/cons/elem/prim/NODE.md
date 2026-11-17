---
id: KRN.CONS.ELEM.PRIM
title: Definitional point, line, plane, circle and sphere builds
level: 4
parent: KRN.CONS.ELEM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.ELEM-001, REQ-KRN.CONS.ELEM-004, REQ-KRN.CONS.ELEM-005]
covers: [GOM.CON.01, GOM.CON.02, GOM.CON.03, GOM.CON.04, GOM.CON.09, PCD.CON.01, PW.CON.04]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.LA, KRN.NUM.FN, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct build-point, fid construct build-line, fid construct build-plane, fid construct build-circle, fid construct build-sphere]
mcp: [construct_build_point, construct_build_line, construct_build_plane, construct_build_circle, construct_build_sphere]
journal: [construct.build_point@1, construct.build_line@1, construct.build_plane@1, construct.build_circle@1, construct.build_sphere@1]
updated: 2026-11-17
---

# KRN.CONS.ELEM.PRIM: Definitional point, line, plane, circle and sphere builds

## Purpose

Builds the basic primitives from other elements by definition, with no fitting: a point at a circle's centre, a line through two points, a plane through three points or through a point normal to a line, a circle through three points, a sphere through four points or from a centre and a radius. These are the everyday "construct" steps of every incumbent.

## Scope

**In:** the variant table below, each a `Construction` with algorithm ID `construct.build_<primitive>.<variant>@1.0.0`, documented input roles and minimum counts; conditioning checks with explicit thresholds; the five registry commands; spec cards.

| Primitive | Variants |
|---|---|
| point | `coords` (from a quantity triple), `centre_of` (circle, sphere, slot, rectangle, ellipse), `apex_of` (cone), `along_axis` (line plus signed distance) |
| line | `two_points`, `axis_of` (cylinder, cone, slot centre line), `point_direction` (point plus a line or plane normal), `normal_through` (plane plus point) |
| plane | `three_points`, `point_normal` (point plus line), `line_point`, `two_lines` (intersecting or parallel) |
| circle | `three_points` (3D), `centre_axis_radius` (point, line, radius) |
| sphere | `four_points`, `centre_radius` |

**Out:** cylinder, cone, slot and rectangle (`KRN.CONS.ELEM.SHAPE`); best fit through many elements (`KRN.CONS.ELEM.FITC`); pairwise operations such as intersections (`KRN.CONS.OPS`).

## Requirements

- **REQ-KRN.CONS.ELEM.PRIM-001** The module shall implement every variant in the table as a `Construction` with its documented input roles, and the result shall be the exact geometric definition (for example the unique circle through three non-collinear points).
- **REQ-KRN.CONS.ELEM.PRIM-002** If inputs are fewer than the variant needs or of the wrong kind, then the build shall return `InputCount` or `WrongInputKind` naming the input index.
- **REQ-KRN.CONS.ELEM.PRIM-003** If inputs are degenerate beyond a `ConstructParams` threshold (coincident points, collinear points for a plane or circle, coplanar points for a sphere, parallel line and normal), then the build shall return `IllConditioned` with the measured distance or angle.
- **REQ-KRN.CONS.ELEM.PRIM-004** Results shall be sign-normalised by the `KRN.LSQ.TRAIT` rules so the same inputs in the same order give bit-identical parameters on x86-64, ARM64 and wasm32.
- **REQ-KRN.CONS.ELEM.PRIM-005** When a command is committed, the system shall journal one `construct.build_<primitive>@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/prim-defs-v1` (500 seeded cases per variant with closed-form expected results), when each `fid construct build-<primitive> --variant <v> --json` runs, then parameters agree with the expected values within 1e-12 relative [unverified tolerance].
- **AC-2** Given three points 1e-13 m from collinear and a threshold of 1e-9 m, when `build-plane --variant three_points` runs, then it returns `IllConditioned{measure: "collinearity", value, threshold}` and journals nothing.
- **AC-3** Given `build-sphere --variant four_points` with three inputs, then it returns `InputCount{expected: 4, got: 3}`.
- **AC-4** Given each variant, when the `KRN.CONS.DECL` harness runs, then repeat and replace-then-evaluate are bit-identical, and digests match on x86-64, ARM64 and wasm32.
- **AC-5** Given a committed case, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `construct.build_point@1`, `construct.build_line@1`, `construct.build_plane@1`, `construct.build_circle@1`, `construct.build_sphere@1`; W2 (dry-run R1). Params: `variant`, ordered input element handles, numeric parameters with quantity kind and unit (radius, distance), thresholds. Result: `FittedFeature` of the primitive.
- **Algorithm:** closed forms; circle through three points by the perpendicular-bisector solve with `fid_num::la` (3 x 3), sphere through four points by a 3 x 3 linear solve; collinearity and coplanarity measured as distances relative to the input span.
- **Errors:** `InputCount`, `WrongInputKind`, `IllConditioned`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 2 microseconds per build [unverified until measured].
- **Crate:** `fid-construct` (module `elem_prim`).

## Test fixtures

`fixtures/cons/prim-defs-v1`: seeded generator with closed-form expectations and near-degenerate cases, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `elem_prim` implementing the `Construction` trait for every variant of point (`coords`, `centre_of`, `apex_of`, `along_axis`), line (`two_points`, `axis_of`, `point_direction`, `normal_through`), plane (`three_points`, `point_normal`, `line_point`, `two_lines`), circle (`three_points`, `centre_axis_radius`) and sphere (`four_points`, `centre_radius`), with algorithm IDs `construct.build_<primitive>.<variant>@1.0.0`. Use closed forms and `fid_num::la` small solves; return `InputCount`, `WrongInputKind` or `IllConditioned{measure, value, threshold}` with explicit thresholds from `ConstructParams`; sign-normalise results as in `fid-fit`. Add registry data rows `construct.build_point@1`, `construct.build_line@1`, `construct.build_plane@1`, `construct.build_circle@1`, `construct.build_sphere@1` (W2, dry-run R1, D0; CLI `fid construct build-<primitive>`; MCP `construct_build_<primitive>`; one journal event each with algorithm ID, input element IDs, parameter digest, output digest) and spec cards. Tests: `fixtures/cons/prim-defs-v1` closed-form agreement, near-collinear plane error, input-count error, recompute harness and cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.ELEM.PRIM.

## Open questions

- Incumbents offer more variants (for example a point as the projection of one point onto another element) [unverified]; projections belong to `KRN.CONS.OPS`, and further variants are added by minor algorithm versions.

## Children

_Leaf: none._
