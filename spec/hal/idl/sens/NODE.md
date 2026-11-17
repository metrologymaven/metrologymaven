---
id: HAL.IDL.SENS
title: Sensor capability packages (pose, scan, surface, gauge, environment, camera)
level: 3
parent: HAL.IDL
kind: module
status: split
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL-001]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.STAMP, HAL.IDL.GEN.FMT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.SENS: Sensor capability packages

## Purpose

The eight non-motion capability packages: `PortablePose`, `LineScan`, `AreaScan`, `HeightMap`, `Profile`, `Gauge`, `Environment` and `Camera`. Each has a control-plane interface, data-plane batch schemas with declared encodings, semantics notes and a conformance profile. They are grouped because they share the same pattern (configure, start, stream, stop) and none of them moves a machine.

## Scope

**In:**
- `Gauge@1` (read, stream, zero; scalar with resolution and time) and `Environment@1` (subscribe; temperature, humidity, pressure) — P2
- `LineScan@1` (exposure, spacing, trigger mode, start and stop; profile batches with declared encoding, intensity, pose reference) — P2
- `PortablePose@1` (6DoF pose stream with covariance, triggers, reflector state, weather) — P3
- `AreaScan@1` (configure, capture; point frames, normals, pose reference) — P3
- `HeightMap@1` (areal topography with metadata compatible with ISO 25178-72 / X3P [unverified]) and `Profile@1` (stylus profiles and roundness traces) — P3
- `Camera@1` (frames in GenICam pixel formats, intrinsics) — P4
- Namespaced vendor extensions declared in describe and included in the configuration digest
- Conformance profiles per package (`LineScan-core@1`, `Gauge-core@1`, ...)

**Out** (and which node owns it instead):
- CMM motion, tactile probing and rotary packages (owned by `HAL.IDL.CNC`; `PortablePose` devices reuse `TactileProbe` from there)
- Simulators implementing these packages (owned by `HAL.SIM`)
- Drivers (owned by `HAL.DRV`)

## Requirements

- **REQ-HAL.IDL.SENS-001** Each of the eight packages shall define describe, configure and acquisition operations, at least one data-plane batch schema with a declared encoding, and a named conformance profile.
- **REQ-HAL.IDL.SENS-002** The `LineScan@1` profile batch shall carry sequence number, device and host timestamps, profile ID, encoded x and z arrays with their encoding descriptor, an optional intensity array, a validity mask and an optional pose reference.
- **REQ-HAL.IDL.SENS-003** The `PortablePose@1` batch shall carry a 6DoF pose with a 6×6 covariance in SI units and the frame it is expressed in.
- **REQ-HAL.IDL.SENS-004** The `Gauge@1` reading shall carry value with SI unit, resolution, gauge identity, calibration reference and device timestamp when the gauge supplies one.
- **REQ-HAL.IDL.SENS-005** The `Environment@1` reading shall carry temperature in kelvin with the sensor's stated standard uncertainty and sampling time, and optional humidity and pressure.
- **REQ-HAL.IDL.SENS-006** A vendor extension setting shall be accepted only if the driver declared it in describe under its own namespace, and every extension value shall be included in the configuration digest.
- **REQ-HAL.IDL.SENS-007** The recommended batch size for every streaming package shall be 1 to 8 MB or 50 ms of data, and the conformance profile shall check it [unverified].

## Interfaces

- **Offers:** the eight packages and conformance profiles.
- **Consumes:** `HAL.IDL.BASE`, `HAL.IDL.UNITS`, `HAL.IDL.STAMP`.

## Decomposition hints

One child per phase group: SENSP2 (`Gauge`, `Environment`, `LineScan`), PORT (`PortablePose`), AREA (`AreaScan`), SURF (`HeightMap`, `Profile`), CAM (`Camera`). `Gauge` plus `Environment` is likely the first leaf.

## Split rationale

The split follows the **phase groups** of the packages, with the shared streaming pattern pulled out first. STREAM holds what all eight packages share (the configure/start/stream/stop template, the vendor-extension rule and the batch-size profile items), so each package child only defines payloads. The P2 packages are finished as leaves: `Gauge@1` with `Environment@1` (GAUENV) and `LineScan@1` (LINE). The P3 and P4 packages (PORT, AREA, SURF, CAM) stay open with refined requirements until the integrations that consume them start.

Alternatives considered:
- **Hint SENSP2 as one child for all three P2 packages.** Rejected: LineScan's columnar, throughput-bound layout is a different design problem from scalar gauge and environment readings.
- **Shared rules duplicated in every package.** Rejected: REQ-HAL.IDL.SENS-006 and -007 would then be tested eight times in slightly different ways.

## Open questions

- Whether an open "I++ for portables" standard should replace `PortablePose` later (concept doc 08 §10) is out of scope until P4.
- X3P metadata compatibility is [unverified] against the published ISO 25178-72 schema (carried in `HAL.IDL.SENS.SURF`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.IDL.SENS.STREAM | Shared streaming template, vendor-extension rule and batch-size profile items (leaf) | 001 006 007 | none |
| HAL.IDL.SENS.GAUENV | Gauge@1 and Environment@1 packages (leaf) | 001 004 005 | none |
| HAL.IDL.SENS.LINE | LineScan@1 package (leaf) | 001 002 | none |
| HAL.IDL.SENS.PORT | PortablePose@1 package (P3) | 001 003 | none |
| HAL.IDL.SENS.AREA | AreaScan@1 package (P3) | 001 | none |
| HAL.IDL.SENS.SURF | HeightMap@1 and Profile@1 packages (P3) | 001 | none |
| HAL.IDL.SENS.CAM | Camera@1 package (P4) | 001 | none |
