---
id: KRN.CONS.OPS.INTQUAD
title: Intersections involving circles, spheres, cylinders and cones
level: 4
parent: KRN.CONS.OPS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.OPS-001, REQ-KRN.CONS.OPS-002, REQ-KRN.CONS.OPS-004]
covers: [GOM.CON.14]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.LA, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-construct]
updated: 2026-11-23
---

# KRN.CONS.OPS.INTQUAD: Intersections involving circles, spheres, cylinders and cones

## Purpose

Intersections where at least one input is curved: line-sphere, line-cylinder, line-cone, plane-sphere, plane-cylinder, plane-cone, circle-circle (coplanar) and sphere-sphere. Most have two results or a tangent case, so each variant takes a required `branch` selector and fails on tangency within a threshold rather than returning an unstable point.

## Scope

**In:** variants `construct.intersect.<pair>@1.0.0` for `line_sphere`, `line_cylinder`, `line_cone` (two points, `branch: first | second` ordered along the line direction), `plane_sphere` (circle), `plane_cylinder` (circle if perpendicular, ellipse otherwise, two lines if parallel with `branch`), `plane_cone` (circle or ellipse only), `circle_circle` (coplanar, two points with `branch`), `sphere_sphere` (circle); tangency and miss detection.
**Out:** linear intersections (`KRN.CONS.OPS.INTLIN`); command and dispatch (`KRN.CONS.OPS.TABLE`); intersections with meshes or CAD (`GEO.DIST`, `KRN.CONS.DERIV`).

## Requirements

- **REQ-KRN.CONS.OPS.INTQUAD-001** Each variant shall implement the `Construction` trait and return the intersection element selected by the required `branch` parameter where the intersection has more than one component.
- **REQ-KRN.CONS.OPS.INTQUAD-002** If the inputs do not intersect, then the variant shall return `Undefined{reason: no_intersection, gap}` with the signed gap distance.
- **REQ-KRN.CONS.OPS.INTQUAD-003** If the inputs are tangent within the distance threshold (discriminant-based measure), then the variant shall return `IllConditioned{measure: "tangency", value, threshold}`.
- **REQ-KRN.CONS.OPS.INTQUAD-004** If a plane-cone section is a parabola or hyperbola, then the variant shall return `Undefined{reason: conic_not_supported}`.
- **REQ-KRN.CONS.OPS.INTQUAD-005** Each variant shall pass the `KRN.CONS.DECL` recompute-equivalence harness on fixture `fixtures/cons/intquad-v1`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/intquad-v1` (500 seeded cases per variant with closed-form references), when each variant runs, then results agree within 1e-12 relative [unverified tolerance], and `branch` order follows the line direction.
- **AC-2** Given a plane at 30 degrees to a cylinder axis, then `plane_cylinder` returns an ellipse whose semi-axes equal r and r / cos(60 degrees) within 1e-12 relative.
- **AC-3** (error path) Given a line 1e-12 m from tangent to a sphere and a 1e-9 m threshold, then the variant returns `IllConditioned{measure: "tangency"}`; given a line 1 mm outside the sphere, `Undefined{reason: no_intersection, gap: 0.001}`.
- **AC-4** Given the harness, then repeat and replace-then-evaluate are bit-identical on x86-64, ARM64 and wasm32.

## Design

- **Registry command(s):** none; dispatched by `construct.intersect_elements@1` in `KRN.CONS.OPS.TABLE`.
- **Algorithm:** reduce each pair to a quadratic in the line parameter or to a planar section in a local frame; solve quadratics with the cancellation-free form (choose the root formula by the sign of b); ellipse parameters from the plane-axis angle.
- **Errors:** `Undefined{reason}`, `IllConditioned`, `WrongInputKind`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 2 microseconds per intersection [unverified until measured].
- **Crate:** `fid-construct` (module `ops_intquad`).

## Test fixtures

`fixtures/cons/intquad-v1`: seeded generator with closed-form references, tangent and miss cases, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `ops_intquad` implementing the `Construction` trait for `construct.intersect.<pair>@1.0.0` with pairs `line_sphere`, `line_cylinder`, `line_cone`, `plane_sphere`, `plane_cylinder`, `plane_cone`, `circle_circle`, `sphere_sphere`. Multi-component results are selected by a required `branch` parameter (points ordered along the line direction). Return `Undefined{reason: no_intersection, gap}`, `IllConditioned{measure: "tangency"}` within the explicit threshold, and `Undefined{reason: conic_not_supported}` for parabolic or hyperbolic plane-cone sections. Solve quadratics in the cancellation-free form. No command rows here. Tests: `fixtures/cons/intquad-v1`, oblique plane-cylinder ellipse, tangent and miss cases, `KRN.CONS.DECL` harness on x86-64/ARM64/wasm32. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.CONS.OPS.INTQUAD.

## Open questions

- Returning both branches at once needs a `Set` output from `KRN.CONS.DECL`; until then callers construct each branch separately.

## Children

_Leaf: none._
