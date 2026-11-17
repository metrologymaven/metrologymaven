---
id: KRN.CONS.DERIV.SYS
title: System planes, axes and coordinate-system elements
level: 4
parent: KRN.CONS.DERIV
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.DERIV-003, REQ-KRN.CONS.DERIV-005]
covers: [GOM.CON.13]
depends_on: [KRN.CONS.DECL, GEO.FRAME.XFORM, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct create-system-element]
mcp: [construct_create_system_element]
journal: [construct.create_system_element@1]
updated: 2026-11-17
---

# KRN.CONS.DERIV.SYS: System planes, axes and coordinate-system elements

## Purpose

Creates the elements every project starts with: the origin point, the X, Y and Z axes, the XY, YZ and ZX planes, and a coordinate-system element of a given frame. They are constructions over the frame, so when an alignment changes the frame they recompute and every feature built on them follows.

## Scope

**In:** element kinds `origin`, `axis_x|y|z`, `plane_xy|yz|zx`, `csys`; input as a plain-data rigid frame (rotation matrix and translation, SI metres, as defined by `GEO.FRAME.XFORM`); algorithm `construct.create_system_element.frame@1.0.0`; the command `construct.create_system_element@1`; a spec card.
**Out:** frames, frame trees and alignments (`GEO.FRAME`, `ALN`); transforming other elements (`GEO.FRAME.APPLY`).

## Requirements

- **REQ-KRN.CONS.DERIV.SYS-001** When asked for a system element of a frame, the module shall return the origin as the frame translation, each axis as a line through the origin along the corresponding rotation column, each plane through the origin with the normal along the remaining column (XY: Z, YZ: X, ZX: Y), and `csys` as the frame itself.
- **REQ-KRN.CONS.DERIV.SYS-002** Element parameters shall be copied from the frame without arithmetic, so the result is bit-identical to the frame's columns.
- **REQ-KRN.CONS.DERIV.SYS-003** If the frame's rotation is not orthonormal within the threshold in `ConstructParams` (Frobenius norm of R^T R - I), then the construction shall return `IllConditioned{measure: "orthonormality"}`.
- **REQ-KRN.CONS.DERIV.SYS-004** Each element shall declare the frame ID as its only input, so it recomputes when the frame changes.
- **REQ-KRN.CONS.DERIV.SYS-005** When the command is committed, the system shall journal one `construct.create_system_element@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/sys-frames-v1` (20 seeded rigid frames), when `fid construct create-system-element --kind plane_zx --frame <id> --json` runs for every kind and frame, then each result's parameters equal the frame's columns bit for bit.
- **AC-2** Given a frame replaced by a new one, when the element is recomputed through the `KRN.CONS.DECL` harness, then the result equals a fresh construction on the new frame bit for bit.
- **AC-3** (error path) Given a frame whose rotation has a 1e-3 shear and threshold 1e-9, then the command returns `IllConditioned{measure: "orthonormality"}` and journals nothing.
- **AC-4** Given a committed AC-1 run, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `construct.create_system_element@1`, W2 (dry-run R1). Params: `kind`, frame handle (resolved by the bus into a plain-data rigid frame), orthonormality threshold. Result: `FittedFeature::{Point, Line3, Plane}` or a coordinate-system element.
- **Algorithm:** column copies; orthonormality check by fixed-order sums.
- **Errors:** `IllConditioned`, `WrongInputKind`, `Num(NonFinite)`.
- **Determinism:** D0.
- **Performance budget:** negligible (< 1 microsecond per element) [unverified until measured].
- **Crate:** `fid-construct` (module `deriv_sys`).

## Test fixtures

`fixtures/cons/sys-frames-v1`: seeded rigid frames generated in test code, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, f64, no I/O, no `unsafe`; may depend on `fid-num`, `fid-fit` and `fid-geo-frame` transform types only), add module `deriv_sys` implementing the `Construction` trait for `construct.create_system_element.frame@1.0.0` with kinds `origin`, `axis_x|y|z`, `plane_xy|yz|zx`, `csys`. Copy parameters from the frame's rotation columns and translation without arithmetic; check orthonormality (Frobenius norm of R^T R - I, fixed-order sums) against the explicit threshold and return `IllConditioned{measure: "orthonormality"}` beyond it; declare the frame as the only input. Add the registry data row `construct.create_system_element@1` (W2, dry-run R1, D0; CLI `fid construct create-system-element`; MCP `construct_create_system_element`; one journal event with algorithm ID, input element IDs, parameter digest, output digest) and a spec card. Tests: `fixtures/cons/sys-frames-v1` bit-exact column copies for all kinds, recompute harness, sheared-frame error, exact replay IDENTICAL. KNP-1: no FMA, no `f64::min`/`max`, no HashMap/HashSet; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.CONS.DERIV.SYS.

## Open questions

- `FittedFeature` (`KRN.LSQ.TRAIT`) has no coordinate-system variant; `csys` needs one (or a `ConstructedElement` variant in `KRN.CONS.DECL`). Raised for those owners; until then `csys` returns the frame rows in this module's own result type.

## Children

_Leaf: none._
