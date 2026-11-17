---
id: KRN.CONS.DERIV.SPT
title: Surface points with nominal normals
level: 4
parent: KRN.CONS.DERIV
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.DERIV-002, REQ-KRN.CONS.DERIV-004, REQ-KRN.CONS.DERIV-005]
covers: [GOM.CON.12]
depends_on: [KRN.CONS.DECL, GEO.DIST.FACE, GEO.DIST.MESH, GEO.NURBS.EVAL, GEO.CAD.TOPO, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct sample-surface-points]
mcp: [construct_sample_surface_points]
journal: [construct.sample_surface_points@1]
updated: 2026-11-17
---

# KRN.CONS.DERIV.SPT: Surface points with nominal normals

## Purpose

Creates nominal surface points: a location on a nominal face with its outward normal, either at given surface parameters (u, v) or by orthogonal projection of a given 3D location onto a face, a body or a nominal mesh. Surface points are the nominal side of single-point checks in scan inspection and of vector-point probing on a CMM.

## Scope

**In:** variants `construct.sample_surface_points.uv@1.0.0` (face plus parameter pairs), `construct.sample_surface_points.project@1.0.0` (face or body plus locations) and `construct.sample_surface_points.mesh@1.0.0` (mesh plus locations, pseudo-normal rule); trim checks; the `outside_trim` and `no_projection` cases; batch input with per-point outcomes; the command `construct.sample_surface_points@1`; spec cards.
**Out:** projection and evaluation primitives (`GEO.DIST.FACE`, `GEO.DIST.MESH`, `GEO.NURBS.EVAL`); measuring actual points at these locations (`SCAN`, `CMM`); deviation computation (`SCAN`, `GDT`).

## Requirements

- **REQ-KRN.CONS.DERIV.SPT-001** When given a face and parameter pairs, the module shall return for each pair the surface point and unit outward normal evaluated on the exact surface, oriented by the face's sense.
- **REQ-KRN.CONS.DERIV.SPT-002** When given locations and a face, body or mesh, the module shall return for each location the orthogonal foot point and unit outward normal (pseudo-normal for meshes) together with the face ID and (u, v) where defined.
- **REQ-KRN.CONS.DERIV.SPT-003** If a parameter pair or projection foot lies outside the face's trimmed region, then that point's outcome shall be `Undefined{reason: outside_trim, index}`; if no foot is found, `Undefined{reason: no_projection, index}`; and no other point's result shall change.
- **REQ-KRN.CONS.DERIV.SPT-004** Each surface point shall declare the nominal element and its parameters, so it recomputes when the nominal changes.
- **REQ-KRN.CONS.DERIV.SPT-005** When the command is committed, the system shall journal one `construct.sample_surface_points@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/spt-analytic-v1` (plane, cylinder, sphere and torus faces with closed-form points and normals at 500 seeded (u, v) pairs), when `fid construct sample-surface-points --variant uv --json` runs, then points agree with the closed form within 1e-12 m and normals within 1e-12 rad [unverified tolerance].
- **AC-2** Given 1 000 seeded locations within 5 mm of a trimmed NURBS face, when the `project` variant runs, then every foot lies on the surface within 1e-9 m [unverified] and every location whose foot lies in a hole returns `outside_trim` with its index.
- **AC-3** Given a nominal mesh and the `mesh` variant, then normals equal the pseudo-normal rule of `GEO.DIST.SIGN` at vertices and edges.
- **AC-4** Given a committed AC-1 run, when replayed in exact mode, then the outcome is `IDENTICAL`, and output digests are identical on x86-64, ARM64 and wasm32.
- **AC-5** (error path) Given a non-finite location, then the command fails with `Num(NonFinite)` and journals nothing.

## Design

- **Registry command(s):** `construct.sample_surface_points@1`, W2 (dry-run R1). Params: `variant`, nominal handle (face, body or mesh, resolved by the bus into GEO library references or a plain-data mesh view), `uv: [[f64;2]]` or `locations: [[f64;3]]` (m), projection options passed through to `GEO.DIST`. Result: one surface point per input with outcome, foot (m), unit normal, face ID and (u, v).
- **Data structures:** `SurfacePoint { c: [f64;3], n: [f64;3], face: Option<[u8;16]>, uv: Option<[f64;2]> }`, emitted as a vector-point feature (see Open questions).
- **Algorithm:** `uv`: evaluate surface and normal with `GEO.NURBS.EVAL`, check `classify_uv`; `project`: call the `GEO.DIST.FACE` targets; `mesh`: `GEO.DIST.MESH` closest point plus pseudo-normal.
- **Errors:** per-point `Undefined`; command-level `WrongInputKind`, `Num`.
- **Determinism:** D0 (inherits the GEO libraries' determinism; they are in the kernel graph).
- **Performance budget:** 10 000 projected points onto a 50-face body in < 100 ms [unverified until measured].
- **Crate:** `fid-construct` (module `deriv_spt`).

## Test fixtures

`fixtures/cons/spt-analytic-v1` (closed-form faces built in test code, seeded parameters) and a trimmed NURBS face from `GEO.NURBS.REF` fixtures, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; may depend on `fid-num`, `fid-fit`, `fid-geo-nurbs`, `fid-geo-dist` and the GEO CAD topology library, never on CORE or any GEO command or storage crate), add module `deriv_spt` with three `Construction` variants: `construct.sample_surface_points.uv@1.0.0`, `.project@1.0.0` and `.mesh@1.0.0`. Each returns per input a `SurfacePoint { c, n, face, uv }` or a per-point `Undefined{reason: outside_trim | no_projection, index}` without affecting other points; normals are unit and outward by face sense (mesh: pseudo-normal). Add the registry data row `construct.sample_surface_points@1` (W2, dry-run R1, D0; CLI `fid construct sample-surface-points`; MCP `construct_sample_surface_points`; one journal event with algorithm ID, input element IDs, parameter digest, output digest) and spec cards. Tests: `fixtures/cons/spt-analytic-v1` against closed forms, trimmed NURBS projections with hole cases, mesh pseudo-normals, exact replay IDENTICAL, cross-target digests on x86-64/ARM64/wasm32, non-finite input error. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.DERIV.SPT.

## Open questions

- `KRN.LSQ.TRAIT`'s `FittedFeature` has no point-with-direction variant. This leaf needs one (`VectorPoint{c, n}`) or a `ConstructedElement` variant in `KRN.CONS.DECL`; raised for those owners. Until then the leaf returns `SurfacePoint` inside its own result type.

## Children

_Leaf: none._
