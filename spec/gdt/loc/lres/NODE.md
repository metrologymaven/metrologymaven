---
id: GDT.LOC.LRES
title: Location and orientation result payload
level: 3
parent: GDT.LOC
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC-001, REQ-GDT.LOC-002]
covers: []
depends_on: [GDT.DRF.BUILD, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.LOC.LRES: Location and orientation result payload

## Purpose

One payload type and one set of pure helpers for every orientation and location result: the value, the toleranced element used, per-axis deviations of the actual feature from its true (basic) location expressed in the DRF, actual and virtual condition sizes, stated and bonus tolerance. Every LOC child fills this payload so reports and verifiers read one shape.

## Scope

**In:** `LocPayload` type; `drf_deviation(actual, nominal, drf)` returning dx, dy, dz in DRF axes; direction deviation in radians between actual and nominal directions; formatting-neutral SI values; JSON Schema for MCP output.
**Out:** computing the tolerance value (owned by `GDT.LOC.ORIENT`, `GDT.LOC.POS`, `GDT.LOC.COAX`, `GDT.LOC.SYM`); bonus computation (owned by `GDT.ZONE.MATL`); DRF construction (owned by `GDT.DRF.BUILD`).

## Requirements

- **REQ-GDT.LOC.LRES-001** `LocPayload` shall hold value, toleranced-element kind, per-axis deviations in the DRF, direction deviation, actual size, virtual-condition size, stated tolerance and bonus, each optional where not applicable and never filled with a placeholder number.
- **REQ-GDT.LOC.LRES-002** `drf_deviation` shall express (actual − nominal) in the DRF's axes using the DRF's orthonormal basis, with fixed-order dot products.
- **REQ-GDT.LOC.LRES-003** The direction deviation shall be atan2(|a×b|, a·b) between unit directions, through `fid-num`.
- **REQ-GDT.LOC.LRES-004** If a required DRF axis is not unit within 1e-12 or the inputs contain NaN, then the helpers shall return `bad_drf_basis` or `nan_input`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/lres_rotated_drf.json` (DRF rotated 30 degrees about z, actual hole centre offset (0.02, −0.01, 0) mm in DRF axes), when `drf_deviation` runs, then dx = 0.02 mm and dy = −0.01 mm within 1e-15 m.
- **AC-2** Given directions 1e-6 rad apart, then the direction deviation is 1e-6 rad within 1e-18 rad.
- **AC-3** Given a payload with no bonus (RFS), then the serialised JSON has no `bonus` key (not 0).
- **AC-4** (error path) Given a non-orthonormal basis, then `bad_drf_basis`.
- **AC-5** The JSON Schema validates every payload produced in AC-1..AC-3; outputs are bit-identical across CI platforms.

## Design

- **Registry command(s):** none (library).
- **Data structures:** `LocPayload { value_si, element: Surface | Axis | MedianPlane | MedianPoints | Line, dev_drf: Option<[f64; 3]>, dir_dev_rad: Option<f64>, actual_size: Option<f64>, virtual_condition: Option<f64>, stated: f64, bonus: Option<f64> }`.
- **Algorithm:** closed-form projections onto DRF axes; atan2 form for angles.
- **Errors:** `bad_drf_basis`, `nan_input`.
- **Determinism:** D0.
- **Performance budget:** < 1 µs per call [unverified].
- **Crate:** `fid-gps` (module `loc::payload`).

## Test fixtures

Synthetic rotated DRFs and offsets (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel, pure, KNP-1), module `loc::payload`, define `LocPayload` (fields per REQ-001; optional fields are `Option` and serialise as absent, never as 0) with a JSON Schema 2020-12 export, plus helpers `drf_deviation(actual: P3, nominal: P3, drf: &Drf) -> Result<[f64; 3], LocError>` (components of actual − nominal along the DRF's orthonormal axes, fixed-order dot products) and `direction_deviation(a, b) -> Result<f64, LocError>` (atan2(|a×b|, a·b) via `fid-num`). Errors `bad_drf_basis` (axes not unit or orthogonal within 1e-12) and `nan_input`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num`, fixed-order reductions, no std `HashMap`/`HashSet`, NaN is an error).

## Open questions

- None.

## Children

_Leaf: none._
