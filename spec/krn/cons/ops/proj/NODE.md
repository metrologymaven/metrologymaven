---
id: KRN.CONS.OPS.PROJ
title: Projections onto planes and lines
level: 4
parent: KRN.CONS.OPS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.OPS-001, REQ-KRN.CONS.OPS-002, REQ-KRN.CONS.OPS-004]
covers: [GOM.CON.15]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-construct]
updated: 2026-11-23
---

# KRN.CONS.OPS.PROJ: Projections onto planes and lines

## Purpose

Orthogonal projections of a point, line or circle onto a plane, and of a point onto a line. Projecting a circle onto a plane that is not parallel to it gives an ellipse, so the module says which result type it returns and fails on the degenerate cases (a line perpendicular to the plane projects to a point).

## Scope

**In:** variants `construct.project.point_plane@1.0.0`, `point_line@1.0.0`, `line_plane@1.0.0` (line, or `Undefined{reason: projects_to_point}` when perpendicular within threshold), `circle_plane@1.0.0` (circle when parallel within threshold, ellipse otherwise, `IllConditioned` when edge-on); signed projection distance in diagnostics.
**Out:** projection onto meshes and CAD surfaces (`GEO.DIST`, used by `KRN.CONS.DERIV.SPT`); command and dispatch (`KRN.CONS.OPS.TABLE`).

## Requirements

- **REQ-KRN.CONS.OPS.PROJ-001** Each variant shall implement the `Construction` trait, return the orthogonal projection, and report the signed distance from input to target in the diagnostics.
- **REQ-KRN.CONS.OPS.PROJ-002** If a line is perpendicular to the target plane within the angular threshold, then `line_plane` shall return `Undefined{reason: projects_to_point}`.
- **REQ-KRN.CONS.OPS.PROJ-003** When a circle's plane is parallel to the target within the angular threshold, `circle_plane` shall return a circle; otherwise it shall return an ellipse, and if the circle plane is perpendicular to the target within the threshold, it shall return `IllConditioned{measure: "angle"}`.
- **REQ-KRN.CONS.OPS.PROJ-004** Each variant shall pass the `KRN.CONS.DECL` recompute-equivalence harness on fixture `fixtures/cons/proj-v1`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/proj-v1` (1 000 seeded cases per variant), when each variant runs, then results agree with closed-form references within 1e-12 relative [unverified tolerance].
- **AC-2** Given a 10 mm circle tilted 60 degrees to the plane, then the projection is an ellipse with semi-axes 10 mm and 5 mm.
- **AC-3** (error path) Given a line 1e-10 rad from the plane normal with a 1e-8 rad threshold, then `line_plane` returns `Undefined{reason: projects_to_point}`.
- **AC-4** Given the harness, then repeat and replace-then-evaluate are bit-identical on x86-64, ARM64 and wasm32.

## Design

- **Registry command(s):** none; dispatched by `construct.project_element@1` in `KRN.CONS.OPS.TABLE`.
- **Algorithm:** closed-form projections; ellipse from the projected circle frame.
- **Errors:** `Undefined`, `IllConditioned`, `WrongInputKind`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 1 microsecond per projection [unverified until measured].
- **Crate:** `fid-construct` (module `ops_proj`).

## Test fixtures

`fixtures/cons/proj-v1`: seeded generator with closed-form references and degenerate cases, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `ops_proj` implementing the `Construction` trait for `construct.project.point_plane@1.0.0`, `point_line@1.0.0`, `line_plane@1.0.0` and `circle_plane@1.0.0`, with the signed distance in diagnostics, `Undefined{reason: projects_to_point}` for a perpendicular line, circle-or-ellipse output by the parallel threshold and `IllConditioned{measure: "angle"}` for an edge-on circle. No command rows here. Tests: `fixtures/cons/proj-v1`, tilted-circle ellipse, perpendicular-line case, `KRN.CONS.DECL` harness on x86-64/ARM64/wasm32. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.CONS.OPS.PROJ.

## Open questions

- None.

## Children

_Leaf: none._
