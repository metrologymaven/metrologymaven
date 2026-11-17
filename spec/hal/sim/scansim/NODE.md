---
id: HAL.SIM.SCANSIM
title: Line and area scanner simulators
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-001, REQ-HAL.SIM-005, REQ-HAL.SIM-008]
covers: [DEV.SIM.02]
depends_on: [HAL.IDL.SENS, HAL.SIM.WORK, HAL.SIM.IPPSIM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.SIM.SCANSIM: Line and area scanner simulators

## Purpose

Synthetic `LineScan` and `AreaScan` sources that sample the virtual workpiece: a stationary or encoder-triggered line-profile sensor, an arm- or tracker-mounted line scanner driven by a pose stream, and a structured-light area scanner. They carry the throughput tests of the runtime (10 000 profiles per second) and every live-scanning workflow test.

## Scope

**In:**
- Line-profile simulator with declared scaled-integer encoding, intensity, validity mask, time and encoder triggering
- Mounted line scanner fed by a `PortablePose` simulator through pose references
- Area-scan simulator producing point frames with normals
- Configurable rate up to at least 10 000 profiles per second of 2 000 points
- Occlusion and out-of-range samples marked invalid
- Declaring every source simulated

**Out** (and which node owns it instead):
- Pose sources (owned by `HAL.SIM.PORT`)
- Real scanner drivers (owned by `HAL.DRV`)
- Preview meshing (owned by `HAL.LIVE.SCANS`)

## Requirements

- **REQ-HAL.SIM.SCANSIM-001** The line-profile simulator shall implement `LineScan@1` and pass `LineScan-core@1`, and the area simulator shall implement `AreaScan@1` and pass its profile.
- **REQ-HAL.SIM.SCANSIM-002** The line-profile simulator shall sustain 10 000 profiles per second of 2 000 points in CI without dropping samples on its side [unverified until measured].
- **REQ-HAL.SIM.SCANSIM-003** Profile samples shall lie on the virtual workpiece surface within the configured noise, and occluded or out-of-range samples shall be marked invalid rather than omitted.
- **REQ-HAL.SIM.SCANSIM-004** Every scanner simulator shall declare itself simulated and run headless in CI.

## Interfaces

- **Offers:** scanner simulators registered with the launcher; DEV.SIM.02 (scanner part); throughput test source for `HAL.ACQ.INGEST`.
- **Consumes:** `HAL.IDL.SENS`, `HAL.SIM.WORK`, pose streams from `HAL.SIM.PORT` (optional).

## Decomposition hints

LINE (line-profile simulator; likely a leaf), MOUNT (pose-referenced mounted scanner), AREA (area scanner).

## Open questions

- Whether vendor emulators (concept doc 08 mentions a line-sensor vendor emulator) can complement these in CI depends on their licences [unverified].

## Children

_None yet: status `open`._
