---
id: KRN.CONS.OPS.MID
title: Midpoint, mid-plane, symmetry element and bisector
level: 4
parent: KRN.CONS.OPS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-construct]
satisfies: [REQ-KRN.CONS.OPS-001, REQ-KRN.CONS.OPS-002, REQ-KRN.CONS.OPS-004]
covers: [GOM.CON.17]
depends_on: [KRN.CONS.DECL, KRN.LSQ.TRAIT, KRN.NUM.FN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.CONS.OPS.MID: Midpoint, mid-plane, symmetry element and bisector

## Purpose

Elements halfway between two others: the midpoint of two points (or of two feature centres), the mid-plane of two planes (for the centre plane of a slot or a web), the symmetry element of two like features (mid-line of two lines, mid-circle of two coaxial circles), and the bisector of two lines or two planes. These underlie width, symmetry and centre-plane datums.

## Scope

**In:** variants `construct.midpoint.points@1.0.0` (points or centres), `construct.symmetry.planes@1.0.0` (mid-plane; parallel or intersecting with rule), `construct.symmetry.lines@1.0.0`, `construct.symmetry.circles@1.0.0`, `construct.bisect.lines@1.0.0` and `construct.bisect.planes@1.0.0` (`branch: acute | obtuse`); orientation alignment of the two inputs before averaging.
**Out:** command and dispatch (`KRN.CONS.OPS.TABLE`); width and symmetry tolerance evaluation (`GDT`).

## Requirements

- **REQ-KRN.CONS.OPS.MID-001** Each variant shall implement the `Construction` trait; the midpoint shall be the arithmetic mean of the two locations in input order.
- **REQ-KRN.CONS.OPS.MID-002** Before averaging two directions, the variant shall flip the second to make the dot product non-negative, so swapping input orientation does not change the result.
- **REQ-KRN.CONS.OPS.MID-003** If two planes or lines given to a symmetry variant are not parallel within the angular threshold, then the variant shall return the bisector only when the caller set `allow_intersecting: true`, and otherwise `IllConditioned{measure: "angle"}`.
- **REQ-KRN.CONS.OPS.MID-004** If the inputs of a bisector are parallel within the angular threshold, then the variant shall return `Undefined{reason: parallel}`; the `branch` parameter shall select the acute or obtuse bisector and is required.
- **REQ-KRN.CONS.OPS.MID-005** Each variant shall pass the `KRN.CONS.DECL` recompute-equivalence harness on fixture `fixtures/cons/mid-v1`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/mid-v1` (1 000 seeded cases per variant), when each variant runs, then results agree with closed-form references within 1e-12 relative [unverified tolerance].
- **AC-2** Given two parallel planes with opposite normals 10 mm apart, then the mid-plane lies 5 mm from each, and swapping one normal's sign gives a bit-identical result.
- **AC-3** (error path) Given two planes 5 degrees apart without `allow_intersecting`, then `symmetry.planes` returns `IllConditioned{measure: "angle"}`; given two parallel lines, `bisect.lines` returns `Undefined{reason: parallel}`.
- **AC-4** Given the harness, then repeat and replace-then-evaluate are bit-identical on x86-64, ARM64 and wasm32.

## Design

- **Registry command(s):** none; dispatched by `construct.midpoint_elements@1`, `construct.symmetry_elements@1` and `construct.bisect_elements@1` in `KRN.CONS.OPS.TABLE`.
- **Algorithm:** orientation alignment by dot-product sign; normalised sum (acute bisector) or difference (obtuse); mid-plane point as the mean of the two foot points from the first plane's point.
- **Errors:** `Undefined`, `IllConditioned`, `WrongInputKind`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 1 microsecond per construction [unverified until measured].
- **Crate:** `fid-construct` (module `ops_mid`).

## Test fixtures

`fixtures/cons/mid-v1`: seeded generator with closed-form references and orientation-swapped pairs, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `ops_mid` implementing the `Construction` trait for `construct.midpoint.points@1.0.0`, `construct.symmetry.planes@1.0.0`, `construct.symmetry.lines@1.0.0`, `construct.symmetry.circles@1.0.0`, `construct.bisect.lines@1.0.0` and `construct.bisect.planes@1.0.0`. Align the second direction to a non-negative dot product before averaging; symmetry of non-parallel inputs returns `IllConditioned{measure: "angle"}` unless `allow_intersecting`; bisectors need a required `branch: acute | obtuse` and return `Undefined{reason: parallel}` for parallel inputs. No command rows here. Tests: `fixtures/cons/mid-v1`, opposite-normal mid-plane invariance, the two error cases, `KRN.CONS.DECL` harness on x86-64/ARM64/wasm32. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.CONS.OPS.MID.

## Open questions

- None.

## Children

_Leaf: none._
