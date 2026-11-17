---
id: HAL.IDL.UNITS
title: Quantities, declared encodings, frames and timestamp types
level: 3
parent: HAL.IDL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-003]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.IDL.UNITS: Quantities, declared encodings, frames and timestamp types

## Purpose

The shared value types that every capability package uses on its control and data planes: SI quantities, the declared exact encoding of raw payloads, frame identifiers in the session frame graph, and timestamp and time-source fields. Defining them once keeps every package and every driver language consistent and makes lossless conversion to f64 metres possible.

## Scope

**In:**
- `Quantity { value: f64, unit: SiUnit }` with `SiUnit` limited to m, rad, s, K, Pa (plus dimensionless `1`)
- `EncodingDescriptor`: native numeric type, scale, offset, target unit
- `FrameId` and `FrameKind` (sensor, mount, device, machine, part) and the frame-edge record (parent, child, transform reference, time-varying flag, pose reference)
- Timestamp fields `t_dev` and `t_host` (i64 nanoseconds) and the ordered `TimeSource` enum
- Direction-vector convention for probe and normal data

**Out:**
- Performing the conversion from encoded payloads to f64 metres (owned by `HAL.ACQ.CONV`)
- Estimating clock offset and drift (owned by `HAL.ACQ.TIME`)
- Frame transforms of record and their uncertainty (owned by `GEO` and `ALN`)
- Display units (owned by `VIZ`)

## Requirements

- **REQ-HAL.IDL.UNITS-001** The `Quantity` type shall accept only the units m, rad, s, K, Pa and dimensionless, and shall reject NaN and infinite values with `quantity_not_finite`.
- **REQ-HAL.IDL.UNITS-002** The `EncodingDescriptor` shall declare the native numeric type (one of i16, i32, i64, u16, u32, f32, f64), a finite f64 scale, a finite f64 offset and the target SI unit, and shall define the decoded value as offset + scale × raw.
- **REQ-HAL.IDL.UNITS-003** If an `EncodingDescriptor` has a zero scale, a non-finite scale or offset, or a target unit outside the `Quantity` unit set, then validation shall fail with `encoding_invalid`.
- **REQ-HAL.IDL.UNITS-004** The `TimeSource` enum shall list `HW_TRIGGER`, `PTP`, `DEVICE_CLOCK`, `NTP` and `HOST_RECEIVE` in that quality order, and the type shall provide a total order consistent with it.
- **REQ-HAL.IDL.UNITS-005** Frame edges shall name parent and child `FrameId`s and a transform reference, and a frame graph containing a cycle or two parents for one frame shall fail validation with `frame_graph_invalid`.
- **REQ-HAL.IDL.UNITS-006** Direction vectors shall be right-handed unit vectors; for probe and normal payloads the vector shall point out of the material, and a vector whose norm differs from 1 by more than 1e-9 shall fail validation with `direction_not_unit`.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/units/quantities.json` (valid values in each unit), when parsed, then each round-trips to identical canonical JSON.
- **AC-2** (error path) Given a quantity with unit `mm`, when parsed, then it fails with `unit_unsupported`; given value `NaN`, it fails with `quantity_not_finite`.
- **AC-3** Given `fixtures/hal/idl/units/encodings.json`, when each descriptor is validated, then the valid ones pass and `scale_zero` and `unit_px` fail with `encoding_invalid`.
- **AC-4** Given `fixtures/hal/idl/units/frames_cycle.json` (sensor → mount → sensor), when the frame graph is validated, then it fails with `frame_graph_invalid`; `frames_arm.json` (sensor → mount → device → machine → part) passes.
- **AC-5** Given the list of time sources shuffled, when sorted, then the order is `HW_TRIGGER < PTP < DEVICE_CLOCK < NTP < HOST_RECEIVE` (best first).
- **AC-6** (error path) Given direction `(1, 0, 1e-6)`, when validated, then it fails with `direction_not_unit`.

## Design

- **Registry commands:** none; library types used by every capability package and by the stamp.
- **Data structures:** `SiUnit`, `Quantity`, `NumericType`, `EncodingDescriptor`, `FrameId` (UUIDv5 under the session namespace), `FrameKind`, `FrameEdge`, `TimeSource`, `Timestamp { t_dev: Option<i64>, t_host: i64 }`, `UnitVector3`.
- **Algorithm:** validation only; frame-graph check is a depth-first walk over a `BTreeMap` adjacency list.
- **Errors:** `unit_unsupported`, `quantity_not_finite`, `encoding_invalid`, `frame_graph_invalid`, `direction_not_unit`.
- **Determinism:** D0; validation is pure and order-independent.
- **Performance budget:** trivial; validating a 64-frame graph < 10 µs [unverified until measured].
- **Crate:** `fid-hal-idl` (module `units`).

## Test fixtures

Project-authored JSON under `fixtures/hal/idl/units/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl`, module `units`, define `SiUnit` (m, rad, s, K, Pa, dimensionless), `Quantity { value: f64, unit }` that rejects NaN/infinity (`quantity_not_finite`) and unknown units (`unit_unsupported`); `EncodingDescriptor { numeric: NumericType (i16|i32|i64|u16|u32|f32|f64), scale: f64, offset: f64, unit: SiUnit }` with decoded value = offset + scale × raw, rejecting zero or non-finite scale or offset (`encoding_invalid`); `TimeSource` ordered HW_TRIGGER < PTP < DEVICE_CLOCK < NTP < HOST_RECEIVE (best first); `Timestamp { t_dev: Option<i64>, t_host: i64 }` in ns; `FrameId`, `FrameKind` (sensor, mount, device, machine, part), `FrameEdge` and a frame-graph validator that rejects cycles and multiple parents (`frame_graph_invalid`); `UnitVector3`, right-handed, pointing out of material, rejecting |norm − 1| > 1e-9 (`direction_not_unit`). Serde with canonical JSON round-trip. BTreeMap only; no panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures in `fixtures/hal/idl/units/`. Do not implement conversion of payloads; that belongs to the acquisition runtime.

## Open questions

- Pressure in Pa and humidity as dimensionless fraction cover the `Environment` capability; whether position quantities for indoor positioning need a geodetic frame kind is left to `HAL.IDL.SENS`.

## Children

_Leaf: none._
