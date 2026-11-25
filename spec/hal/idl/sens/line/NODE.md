---
id: HAL.IDL.SENS.LINE
title: LineScan@1 package (profile batches)
level: 4
parent: HAL.IDL.SENS
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
built_in: [fid-hal-idl]
built_pr: 615
satisfies: [REQ-HAL.IDL.SENS-001, REQ-HAL.IDL.SENS-002]
covers: []
depends_on: [HAL.IDL.SENS.STREAM, HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.SENS.LINE: LineScan@1 package

## Purpose

The `LineScan@1` package for laser line and profile sensors, stationary or mounted on an arm or tracker: exposure, spacing and trigger configuration, and profile batches with declared encodings. It carries the HAL's throughput budget (10 000 profiles per second of 2 000 points), so its batch layout is columnar and encoded, never per-point objects.

## Scope

**In:**
- Settings: exposure (s), point spacing (m), trigger mode (`time | encoder | external`), trigger period or encoder divisor, region of interest (m)
- `ProfileBatch`: sequence range, per-profile `t_dev` and `t_host`, profile IDs, encoded x and z arrays with their `EncodingDescriptor`s, optional intensity array, validity mask, optional pose reference per profile
- `LineScan-core@1` conformance profile

**Out:**
- Pose streams (owned by `HAL.IDL.SENS.PORT`)
- Conversion to f64 metres (owned by `HAL.ACQ.CONV`)
- Fusing pose and profile (owned by `HAL.ACQ.INGEST.SINK` and `HAL.ACQ.TIME`)

## Requirements

- **REQ-HAL.IDL.SENS.LINE-001** A `ProfileBatch` shall carry the sequence range, `t_dev` per profile (optional) and `t_host` per profile, profile IDs, encoded x and z arrays with one `EncodingDescriptor` each, an optional intensity array, a validity mask and an optional pose reference per profile.
- **REQ-HAL.IDL.SENS.LINE-002** All per-point arrays of a batch shall have length profiles x points-per-profile, and all per-profile arrays length profiles; otherwise validation shall fail with `array_length_mismatch`.
- **REQ-HAL.IDL.SENS.LINE-003** The x and z encodings shall have target unit m, and the batch shall state the sensor frame they are expressed in.
- **REQ-HAL.IDL.SENS.LINE-004** `LineScan-core@1` shall check encoding declaration, array lengths, timestamp monotonicity, masked-sample handling, trigger-mode behaviour and the `Stream-common@1` items.
- **REQ-HAL.IDL.SENS.LINE-005** A batch of 100 profiles x 2 000 points with i32 x and z encodings and no intensity array shall serialise to no more than 1.7 MB including mask, timestamps and header (columnar layout: 1.6 MB of coordinates).

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/sens/line/profiles_100x2000.bin` (i32 x and z, no intensity) with its header JSON, when validated, then it passes and its size is at most 1.7 MB.
- **AC-2** (error path) Given a batch whose z array has one point fewer, then validation fails with `array_length_mismatch`.
- **AC-3** (error path) Given an x encoding with target unit `rad`, then validation fails with `encoding_invalid`.
- **AC-4** (error path) Given `t_host` decreasing between profiles, then validation fails with `timestamps_not_monotonic`.
- **AC-5** Given the package export, then it validates as `capability-package/v1`, includes the streaming operations, and `LineScan-core@1` lists the items of REQ-HAL.IDL.SENS.LINE-004.

## Design

- **Registry commands:** none; library package.
- **Data structures:** `ProfileBatchHeader` (JSON) plus columnar binary payload; `EncodedArray { descriptor, bytes }`; `ValidityMask` bitset; `PoseRef { stream_id, seq }`.
- **Algorithm:** validation only; length and monotonicity checks in one pass.
- **Errors:** `array_length_mismatch`, `encoding_invalid`, `timestamps_not_monotonic`.
- **Determinism:** D0.
- **Performance budget:** validate a 100 x 2 000 batch in under 1 ms [unverified until measured].
- **Crate:** `fid-hal-idl` (module `caps::linescan`), layer L0.

## Test fixtures

Generated binary fixture with seed under `fixtures/hal/idl/sens/line/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), module `caps::linescan`, define `LineScan@1` with settings (exposure s, spacing m, trigger mode time|encoder|external, period or divisor, ROI m), streaming operations from `caps::stream`, and the columnar `ProfileBatch` (header JSON + binary arrays): sequence range, per-profile `t_dev` (optional) and `t_host`, profile IDs, x and z `EncodedArray`s with target unit m and a sensor frame, optional intensity, validity mask, optional per-profile pose reference. Validate lengths (`array_length_mismatch`), encodings (`encoding_invalid`), monotonic `t_host` (`timestamps_not_monotonic`). Define `LineScan-core@1`. Implement `CapabilityPackage` and add it to the inventory. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
