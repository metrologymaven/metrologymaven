---
id: KRN.CONS.OPS.OFFS
title: Offsets of points, lines, planes and circles
level: 4
parent: KRN.CONS.OPS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.OPS-001, REQ-KRN.CONS.OPS-002, REQ-KRN.CONS.OPS-004]
covers: [GOM.CON.16]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.CONS.OPS.OFFS: Offsets of points, lines, planes and circles

## Purpose

Moves an element by a signed distance along a defined direction: a plane along its normal, a point or line along a direction taken from another element, a circle along its axis or radially (a circle offset radially changes its radius). Offsets are how users build a "plane 5 mm below the top face" or a "circle 1 mm inside the hole".

## Scope

**In:** variants `construct.offset.plane_normal@1.0.0`, `point_dir@1.0.0` and `line_dir@1.0.0` (direction from a line or plane normal element), `circle_axial@1.0.0`, `circle_radial@1.0.0`; the signed distance as an authored quantity converted once to SI; the negative-radius case.
**Out:** command and dispatch (`KRN.CONS.OPS.TABLE`); offsets of meshes or CAD surfaces (`SCAN`, `GEO`).

## Requirements

- **REQ-KRN.CONS.OPS.OFFS-001** Each variant shall implement the `Construction` trait and translate the element by the signed distance along the defined direction, keeping orientation and other parameters unchanged.
- **REQ-KRN.CONS.OPS.OFFS-002** `circle_radial` shall change the radius by the signed distance, and if the resulting radius is not positive, then it shall return `Undefined{reason: non_positive_radius}`.
- **REQ-KRN.CONS.OPS.OFFS-003** If the direction element has zero length or is undefined, then the variant shall return `IllConditioned{measure: "direction_norm"}`.
- **REQ-KRN.CONS.OPS.OFFS-004** Each variant shall pass the `KRN.CONS.DECL` recompute-equivalence harness on fixture `fixtures/cons/offs-v1`, with the distance in the parameter digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/offs-v1` (1 000 seeded cases per variant), when each variant runs, then results agree with closed-form references within 1e-12 relative [unverified tolerance].
- **AC-2** Given a 5 mm circle and `circle_radial` of -6 mm, then the variant returns `Undefined{reason: non_positive_radius}`.
- **AC-3** Given the same offset with distances 1 mm and 2 mm, then the parameter digests differ.
- **AC-4** Given the harness, then repeat and replace-then-evaluate are bit-identical on x86-64, ARM64 and wasm32.

## Design

- **Registry command(s):** none; dispatched by `construct.offset_element@1` in `KRN.CONS.OPS.TABLE`.
- **Algorithm:** translation by d times the unit direction (direction renormalised with `fid_num::f::sqrt`).
- **Errors:** `Undefined`, `IllConditioned`, `WrongInputKind`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 0.5 microseconds per offset [unverified until measured].
- **Crate:** `fid-construct` (module `ops_offs`).

## Test fixtures

`fixtures/cons/offs-v1`: seeded generator with closed-form references, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `ops_offs` implementing the `Construction` trait for `construct.offset.plane_normal@1.0.0`, `point_dir@1.0.0`, `line_dir@1.0.0`, `circle_axial@1.0.0` and `circle_radial@1.0.0`: translate by the signed distance along the unit direction (or change the radius), returning `Undefined{reason: non_positive_radius}` and `IllConditioned{measure: "direction_norm"}` where applicable; the distance is part of the parameter digest. No command rows here. Tests: `fixtures/cons/offs-v1`, negative-radius case, digest change with distance, `KRN.CONS.DECL` harness on x86-64/ARM64/wasm32. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.CONS.OPS.OFFS.

## Open questions

- None.

## Children

_Leaf: none._
