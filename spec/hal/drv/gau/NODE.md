---
id: HAL.DRV.GAU
title: Gauge drivers (gauge interfaces, Bluetooth LE, serial and keyboard wedge)
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-003, REQ-HAL.DRV-004]
covers: [DEV.GAU.01, DEV.GAU.02, DEV.GAU.03]
depends_on: [HAL.IDL.SENS, HAL.HOST.TIER1, HAL.SIM.GAUENV, HAL.ACQ.FILEDEV]
cli: [fid device read-gauge]
mcp: [device_read_gauge]
journal: [device.read_gauge@1]
updated: 2026-11-16
---

# HAL.DRV.GAU: Gauge drivers

## Purpose

`Gauge` drivers for hand tools and digital gauges: wired and wireless gauge interfaces, Bluetooth LE gauges, and serial or keyboard-wedge input. Gauges are the first real devices on the full acquisition path and serve the P1a–P2 manual-inspection workflows with honest provenance.

## Scope

**In:**
- Wired and wireless gauge interfaces (DEV.GAU.01) through the vendor interface or SDK where terms allow [unverified]
- Bluetooth LE gauges through the standard GATT profile where devices publish one (DEV.GAU.02) [unverified]
- Serial (RS-232) and keyboard-wedge input (DEV.GAU.03)
- `device.read_gauge@1` (D1): read one value or start a stream
- Gauge identity, resolution, calibration reference and device timestamp in every reading
- Keyboard-wedge input without device identity recorded as `entered` provenance through `HAL.ACQ.FILEDEV`

**Out** (and which node owns it instead):
- Manual typed entry (owned by `HAL.ACQ.FILEDEV`)
- Gauge simulators (owned by `HAL.SIM.GAUENV`)
- Gauge R&R and capability studies (owned by `RPT`)

## Requirements

- **REQ-HAL.DRV.GAU-001** When a reading arrives through a gauge driver, the system shall journal it with gauge identity, resolution, calibration reference and device timestamp when the gauge supplies one.
- **REQ-HAL.DRV.GAU-002** A keyboard-wedge reading with no device identity shall be recorded with `source_kind: entered` and never as a device observation.
- **REQ-HAL.DRV.GAU-003** Each gauge driver shall pass `Gauge-core@1` against the gauge simulator before its catalog status can be `supported`.
- **REQ-HAL.DRV.GAU-004** `device.read_gauge@1` shall be class D1 and exposed on MCP only as a request.
- **REQ-HAL.DRV.GAU-005** The serial driver shall be bounded and fail closed on malformed frames, returning `CONFIG_INVALID` or `TRANSIENT` without crashing.

## Interfaces

- **Offers:** gauge drivers (Tier 1); `device.read_gauge@1`; catalog entries for DEV.GAU.*.
- **Consumes:** `HAL.IDL.SENS` (`Gauge@1`), `HAL.HOST.TIER1`, `HAL.SIM.GAUENV`, `HAL.ACQ.FILEDEV`.

## Decomposition hints

SERIAL (RS-232 and keyboard wedge; likely the first leaf), BLE (Bluetooth LE GATT), IFACE (vendor gauge interfaces), CMD (`device.read_gauge@1`).

## Open questions

- Protocol documentation and terms for proprietary gauge interfaces are unverified (seed catalog); until recorded, DEV.GAU.01 stays `blocked` in the catalog.

## Children

_None yet: status `open`._
