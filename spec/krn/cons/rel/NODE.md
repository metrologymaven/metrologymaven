---
id: KRN.CONS.REL
title: Live relationships
level: 3
parent: KRN.CONS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS-008, REQ-KRN.CONS-009, REQ-KRN.CONS-010]
covers: [SA.REL.02, SA.REL.03]
depends_on: [KRN.CONS.DECL, KRN.NUM.KDEC, KRN.NUM.FN, KRN.LSQ.TRAIT, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct relate-elements]
mcp: [construct_relate_elements]
journal: [construct.relate_elements@1]
updated: 2026-11-17
---

# KRN.CONS.REL: Live relationships

## Purpose

Point-to-object and object-to-object relationships (distance, angle, deviation along a direction) as constructions whose output is a value. Because they are constructions, they recompute when either element changes, which is how SpatialAnalyzer-style live relationships behave. They can carry an optional nominal and tolerance for reporting, without making a conformance decision.

## Scope

**In:**
- The variant table below, each a `Construction` with algorithm ID `construct.relate.<variant>@1.0.0` and output quantity kind
- Signed distances where a side is defined (point-to-plane by normal; point-to-cylinder and point-to-sphere positive outside)
- Optional nominal value and tolerance limits held as exact decimals (`KRN.NUM.KDEC`) with unit, carried unchanged to `RPT` and `UNC`
- The registry command `construct.relate_elements@1` and spec cards

| Group | Variants (output) |
|---|---|
| point-to-object | `point_point` (length), `point_line` (length), `point_plane` (signed length), `point_circle` (length to the circle curve, plus radial and axial components), `point_cylinder` (signed length to the surface), `point_sphere` (signed length to the surface) |
| object-to-object | `line_line` (angle; length of the common perpendicular), `line_plane` (angle; length when parallel), `plane_plane` (angle; length when parallel), `centre_centre` (length between centres of circles, spheres, slots) |
| directional | `deviation_along` (signed component of the vector between two elements' reference points along a given unit direction) |

**Out** (and which node owns it instead):
- Conformance decisions on relationship tolerances (`UNC`) and their presentation (`RPT`)
- Point-to-mesh and point-to-CAD distances (`GEO.DIST`, `SCAN`)
- Legacy dimensions of GD&T and size (`GDT`)

## Requirements

- **REQ-KRN.CONS.REL-001** The module shall compute each relationship in its published variant table as a `Construction` whose output is a value with quantity kind (length or plane angle) and SI unit (m or rad).
- **REQ-KRN.CONS.REL-002** When either input element changes, recomputing the relationship shall give the same value bit for bit as computing it fresh.
- **REQ-KRN.CONS.REL-003** A relationship shall carry an optional nominal and tolerance limits as exact decimals with unit, and shall never convert them to f64 or make a conformance decision.
- **REQ-KRN.CONS.REL-004** If a relationship is undefined for the given kinds (for example the angle between a point and a plane) or its inputs are degenerate (a "length when parallel" requested for non-parallel planes beyond the angular threshold), then it shall return `Undefined` or `IllConditioned` with the measured value.
- **REQ-KRN.CONS.REL-005** When the command is committed, the system shall journal one `construct.relate_elements@1` event with algorithm ID, input element IDs, parameter digest and output digest.
- **REQ-KRN.CONS.REL-006** Angles shall be reported in [0, pi/2] for line and plane pairs (acute angle between undirected elements) unless the caller sets `directed: true`, in which case they are in [0, pi].
- **REQ-KRN.CONS.REL-007** The module shall publish its variant table (variant, input kinds, output kind, sign convention) as data with a digest, for `AUTO` docs and `VIZ` menus.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/rel-v1` (500 seeded cases per variant with closed-form references, including inputs 1 km from the origin), when `fid construct relate-elements --variant <v> --json` runs on each, then values agree within 1e-12 relative [unverified tolerance] and the sign follows the published convention.
- **AC-2** Given a point-to-plane relationship with nominal `12.500 mm` and limits `-0.05 mm` / `+0.05 mm`, then the result carries the three decimals byte-for-byte as authored and no decision field.
- **AC-3** Given one input element replaced, when the `KRN.CONS.DECL` harness runs, then recompute equals a fresh construction bit for bit, and output digests match on x86-64, ARM64 and wasm32.
- **AC-4** (error path) Given a point and a plane with `--variant line_plane`, then the command returns `WrongInputKind{index: 0}`; given two planes 2 degrees apart with "length when parallel" and a 1e-6 rad threshold, it returns `IllConditioned{measure: "angle"}`; neither journals anything.
- **AC-5** Given a committed AC-1 case, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `construct.relate_elements@1`, W2 (dry-run R1). Params: `variant`, two ordered input element handles (or one plus `direction` for `deviation_along`), `directed: bool`, optional `nominal: KDec` and `limits: (KDec, KDec)` with unit, thresholds. Result: `ConstructedElement::Value{value, kind, unit}` plus components (for `point_circle`) and the carried nominal and limits.
- **Algorithm:** closed forms; angles via `fid_num::f::atan2(|a x b|, a . b)` for stability near 0 and pi/2; point-to-circle by projecting into the circle plane.
- **Errors:** `Undefined`, `IllConditioned`, `WrongInputKind`, `InputCount`, `Num`.
- **Determinism:** D0.
- **Performance budget:** < 1 microsecond per relationship [unverified until measured].
- **Crate:** `fid-construct` (module `rel`).

## Test fixtures

`fixtures/cons/rel-v1`: seeded generator with closed-form references, sign-convention cases and near-parallel cases, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` (including `kdec`) and `fid-fit` only), add module `rel` implementing the `Construction` trait for `construct.relate.<variant>@1.0.0` with variants `point_point`, `point_line`, `point_plane`, `point_circle`, `point_cylinder`, `point_sphere`, `line_line`, `line_plane`, `plane_plane`, `centre_centre` and `deviation_along`. Output a value with quantity kind and SI unit; signed distances follow the published convention (plane normal side positive; outside cylinder or sphere positive); angles by `atan2(|a x b|, a . b)` in [0, pi/2] unless `directed`. Carry optional nominal and limits as `kdec` decimals with unit, never converted to f64, and make no decision. Publish the variant table as data with a digest. Add the registry data row `construct.relate_elements@1` (W2, dry-run R1, D0; CLI `fid construct relate-elements`; MCP `construct_relate_elements`; one journal event with algorithm ID, input element IDs, parameter digest, output digest) and spec cards. Tests: `fixtures/cons/rel-v1` closed-form agreement far from origin, decimal pass-through, recompute harness and cross-target digests on x86-64/ARM64/wasm32, wrong-kind and near-parallel errors, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.CONS.REL.

## Open questions

- Phase: P1a per the concept baseline; SpatialAnalyzer users need relationships by P3 at the latest [unverified].
- Relationships to meshes and CAD surfaces (point-to-surface) are a `SCAN` or `GEO.DIST` concern today; if users expect them as live relationships, a later variant can call the `GEO.DIST` libraries (L0) the way `KRN.CONS.DERIV.SPT` does.

## Children

_Leaf: none._
