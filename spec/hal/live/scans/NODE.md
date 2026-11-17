---
id: HAL.LIVE.SCANS
title: Live scanning sessions, scanner parameters and preview meshing
level: 3
parent: HAL.LIVE
kind: module
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.LIVE-004, REQ-HAL.LIVE-008]
covers: [PW.SCN.01, PW.SCN.02, PW.SCN.03, PW.SCN.04]
depends_on: [HAL.ACQ.SESS, HAL.ACQ.INGEST, HAL.DRV.ARM, HAL.DRV.TRK, HAL.SIM.SCANSIM, SCAN, VIZ]
cli: [fid device start-scanning, fid device stop-scanning, fid device configure-scanner]
mcp: [device_start_scanning, device_stop_scanning, device_configure_scanner]
journal: [device.start_scanning@1, device.stop_scanning@1, device.configure_scanner@1]
updated: 2026-11-16
---

# HAL.LIVE.SCANS: Live scanning sessions, scanner parameters and preview meshing

## Purpose

Scanning with arm-mounted and tracker-based scanners: start, pause and stop, pass management, scanner parameter control (exposure, density, filters) as journaled configuration, and a preview mesh updated while scanning so the operator sees coverage. The preview is display-only; the mesh of record is made later by `SCAN`.

## Scope

**In:**
- `device.start_scanning@1`, `device.stop_scanning@1` (D1), pause and resume, passes as named groups of batches
- `device.configure_scanner@1` (D1): exposure, point density, filters, as journaled configuration with digests
- Fusion of pose and profile streams with the time-sync check
- Preview meshing during scanning, at least twice per second, display only
- Running against the scanner simulators with results flagged simulated

**Out** (and which node owns it instead):
- Meshing and point processing of record (owned by `SCAN`)
- Rendering (owned by `VIZ`)
- Drivers (owned by `HAL.DRV`)

## Requirements

- **REQ-HAL.LIVE.SCANS-001** While a live scanning session runs, the system shall update a preview mesh at least twice per second from the incoming batches [unverified rate].
- **REQ-HAL.LIVE.SCANS-002** The preview mesh shall never be stored as a value of record and shall carry a display-only marker.
- **REQ-HAL.LIVE.SCANS-003** Every scanner parameter change shall be a journaled `device.configure_scanner@1` command with configuration digests before and after.
- **REQ-HAL.LIVE.SCANS-004** When pose and profile streams are fused and their time sync is insufficient, the system shall flag the affected passes and show the flag to the operator.
- **REQ-HAL.LIVE.SCANS-005** The scanning workflow shall run end to end against the scanner simulators with all results flagged simulated.

## Interfaces

- **Offers:** the commands; live scan stream and preview mesh to `VIZ`; passes to `SCAN`.
- **Consumes:** `HAL.ACQ` sessions and ingest; `HAL.DRV.ARM`, `HAL.DRV.TRK`; `HAL.SIM.SCANSIM`; `SCAN` preview meshing primitives.

## Decomposition hints

SESSION (start, stop, pause, passes), PARAM (configure-scanner), FUSE (pose and profile fusion), PREVIEW (preview meshing pipeline).

## Open questions

- Whether preview meshing can use the GPU for speed is allowed by CONSTITUTION C2 (display only) but must be marked so in its outputs.

## Children

_None yet: status `open`._
