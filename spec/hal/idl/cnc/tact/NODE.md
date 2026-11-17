---
id: HAL.IDL.CNC.TACT
title: TactileProbe@1 package (tip selection, point and scan-point payloads)
level: 4
parent: HAL.IDL.CNC
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.CNC-003]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.CNC.TACT: TactileProbe@1 package

## Purpose

The `TactileProbe@1` capability package used by CNC CMMs, articulated arms and 6DoF tactile probes: tip selection, the single-point payload and the scan-point batch. It is separate from `CncMotion` because portable devices probe without any motion contract.

## Scope

**In:**
- Operations: `list_tips` (R0), `select_tip` (D1; selects an already-qualified tip, no rack motion), `get_tip` (R0)
- `ProbePoint { position, direction, tip_id, tip_radius, t_dev, t_host, quality, compensated }`
- `ScanPointBatch`: a sequence of probe points with a declared encoding per coordinate array, validity mask and sequence range
- `TipInfo { tip_id, radius, qualification_ref }`, qualification as a reference only
- `TactileProbe-core@1` conformance profile

**Out:**
- Tool changes through a rack (owned by `HAL.IDL.CNC.MOTION`, M1)
- Probe compensation (owned by `HAL.IPP.COMP` and `KRN`)
- Qualification procedures (owned by `CMM`)

## Requirements

- **REQ-HAL.IDL.CNC.TACT-001** `ProbePoint` shall carry position (m, in the declared machine or device frame), a unit direction vector pointing out of the material, tip ID, tip radius (m), `t_dev` and `t_host`, a quality indicator in [0, 1] or absent, and a boolean `compensated`.
- **REQ-HAL.IDL.CNC.TACT-002** `ScanPointBatch` shall declare an `EncodingDescriptor` for each coordinate and direction array, a validity mask, the sequence range and the tip ID, and all arrays shall have equal length.
- **REQ-HAL.IDL.CNC.TACT-003** If a point has `compensated: false`, then its tip radius shall be present and positive; if `compensated: true`, the radius shall still be recorded for traceability.
- **REQ-HAL.IDL.CNC.TACT-004** `select_tip` shall be class D1 and shall be refused with `NOT_QUALIFIED` when the tip has no qualification reference.
- **REQ-HAL.IDL.CNC.TACT-005** The `TactileProbe-core@1` profile shall list: tip listing consistency, select-tip refusal for unqualified tips, point payload validity (unit direction, positive radius), scan-batch array-length agreement and timestamp monotonicity.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/cnc/tact/point_ok.json`, when validated, then it passes and round-trips to identical canonical JSON.
- **AC-2** (error path) Given a point with direction (1, 0, 1e-6), then validation fails with `direction_not_unit`.
- **AC-3** (error path) Given an uncompensated point with radius 0, then validation fails with `tip_radius_invalid`.
- **AC-4** (error path) Given a scan batch with 500 x values and 499 z values, then validation fails with `array_length_mismatch`.
- **AC-5** Given the package export, then it validates as `capability-package/v1` with `select_tip` classed D1 and the other two R0.
- **AC-6** Given the profile, then it lists the five items of REQ-HAL.IDL.CNC.TACT-005 with stable IDs.

## Design

- **Registry commands:** none; library package. Runtime implementations are in `HAL.IPP`, `HAL.DRV.ARM` and `HAL.SIM`.
- **Data structures:** `ProbePoint`, `ScanPointBatch { seq_from, seq_to, tip_id, x, y, z, i, j, k: EncodedArray, valid: ValidityMask, t_dev: Option<Vec<i64>>, t_host: Vec<i64> }`, `TipInfo`. Types from `HAL.IDL.UNITS` (`Quantity`, `UnitVector3`, `EncodingDescriptor`, `Timestamp`).
- **Errors:** `direction_not_unit`, `tip_radius_invalid`, `array_length_mismatch`, `timestamps_not_monotonic`, `NOT_QUALIFIED`.
- **Determinism:** D0.
- **Performance budget:** validate a 100 000-point scan batch in under 5 ms [unverified until measured].
- **Crate:** `fid-hal-idl` (module `caps::tactile`), layer L0.

## Test fixtures

Project-authored JSON under `fixtures/hal/idl/cnc/tact/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), module `caps::tactile`, define the `TactileProbe@1` package: operations `list_tips` (R0), `get_tip` (R0), `select_tip` (D1; refuse `NOT_QUALIFIED` without a qualification reference); `ProbePoint { position: [Quantity; 3] (m), direction: UnitVector3 (out of material), tip_id, tip_radius: Quantity (m), t_dev: Option<i64>, t_host: i64, quality: Option<f64 in [0,1]>, compensated: bool }`; `ScanPointBatch` with an `EncodingDescriptor` per array, validity mask, sequence range and equal-length arrays; the `TactileProbe-core@1` profile (five items). Validation errors: `direction_not_unit`, `tip_radius_invalid` (uncompensated points need radius > 0), `array_length_mismatch`, `timestamps_not_monotonic`. Implement the `CapabilityPackage` trait for export as `capability-package/v1`. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures under `fixtures/hal/idl/cnc/tact/`.

## Open questions

- Whether I++ servers deliver tip-centre or compensated points by default is open in `HAL.IPP.COMP`; this payload carries the flag either way.

## Children

_Leaf: none._
