---
id: KRN.CONS.OPS.INTLIN
title: Intersections of points, lines and planes
level: 4
parent: KRN.CONS.OPS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.OPS-001, REQ-KRN.CONS.OPS-002, REQ-KRN.CONS.OPS-003, REQ-KRN.CONS.OPS-004]
covers: [GOM.CON.14]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.LA, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.CONS.OPS.INTLIN: Intersections of points, lines and planes

## Purpose

The linear intersections: line-line (2D and 3D, with the skew-line rule), line-plane, plane-plane and plane-plane-plane. They are the most used intersections and the ones where near-parallel inputs must fail loudly, so each variant measures its conditioning against an explicit threshold.

## Scope

**In:** variants `construct.intersect.line_line@1.0.0` (rules `common_perpendicular_midpoint` and `strict`), `construct.intersect.line_plane@1.0.0`, `construct.intersect.plane_plane@1.0.0` (result line), `construct.intersect.plane3@1.0.0` (result point); angle and distance conditioning; skew distance reporting.
**Out:** intersections with circles and quadrics (`KRN.CONS.OPS.INTQUAD`); the command and dispatch table (`KRN.CONS.OPS.TABLE`).

## Requirements

- **REQ-KRN.CONS.OPS.INTLIN-001** Each variant shall implement the `Construction` trait and return the exact intersection (point or line) of its inputs.
- **REQ-KRN.CONS.OPS.INTLIN-002** When two 3D lines are skew under rule `common_perpendicular_midpoint`, the result shall be the midpoint of their common perpendicular, and the skew distance shall be reported in the result diagnostics; under rule `strict`, a skew distance above the distance threshold shall return `Undefined{reason: skew}`.
- **REQ-KRN.CONS.OPS.INTLIN-003** If the angle between two lines, a line and a plane, two planes, or any two of three planes is below the angular threshold, then the variant shall return `IllConditioned{measure: "angle", value, threshold}` and no element.
- **REQ-KRN.CONS.OPS.INTLIN-004** Each variant shall pass the `KRN.CONS.DECL` recompute-equivalence harness on fixture `fixtures/cons/intlin-v1`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/intlin-v1` (1 000 seeded cases per variant, including inputs 1 km from the origin), when each variant runs, then results agree with a closed-form reference evaluated in the fixture generator within 1e-12 relative [unverified tolerance].
- **AC-2** Given two lines with a known 0.3 mm common perpendicular, then the default rule returns its midpoint and reports skew distance 0.3 mm; the `strict` rule with a 1 micrometre threshold returns `Undefined{reason: skew}`.
- **AC-3** (error path) Given planes 1e-10 rad from parallel and a 1e-8 rad threshold, then `plane_plane` returns `IllConditioned{measure: "angle"}`.
- **AC-4** Given the harness, then repeat and replace-then-evaluate are bit-identical on x86-64, ARM64 and wasm32.

## Design

- **Registry command(s):** none; dispatched by `construct.intersect_elements@1` in `KRN.CONS.OPS.TABLE`.
- **Algorithm:** line-line by solving the 2 x 2 normal equations of the closest points; plane-plane direction by cross product, point by the minimum-norm solution of the two plane equations; plane3 by a 3 x 3 solve with `fid_num::la`; angles via `fid_num::f`.
- **Errors:** `IllConditioned`, `Undefined{reason}`, `WrongInputKind`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 1 microsecond per intersection [unverified until measured].
- **Crate:** `fid-construct` (module `ops_intlin`).

## Test fixtures

`fixtures/cons/intlin-v1`: seeded generator with closed-form references and near-parallel cases, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `ops_intlin` implementing the `Construction` trait for `construct.intersect.line_line@1.0.0` (2D and 3D; rule `common_perpendicular_midpoint` reports the skew distance, rule `strict` returns `Undefined{reason: skew}` above the distance threshold), `construct.intersect.line_plane@1.0.0`, `construct.intersect.plane_plane@1.0.0` and `construct.intersect.plane3@1.0.0`. Return `IllConditioned{measure: "angle", value, threshold}` below the explicit angular threshold. Use small `fid_num::la` solves; no command rows here (dispatch is in `ops_table`). Tests: `fixtures/cons/intlin-v1` closed-form agreement far from origin, skew-line rules, near-parallel planes error, `KRN.CONS.DECL` harness on x86-64/ARM64/wasm32. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.CONS.OPS.INTLIN.

## Open questions

- Incumbent skew-line rules differ [unverified]; a third rule (closest point on the first line) can be added as a minor version if users ask.

## Children

_Leaf: none._
