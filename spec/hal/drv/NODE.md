---
id: HAL.DRV
title: Device drivers and the device-support catalog
level: 2
parent: HAL
kind: subsystem
status: open
phase: P2
size: XXL
owners: []
claimed_by:
satisfies: [REQ-HAL-001, REQ-HAL-009]
covers: [SA.INS.01, SA.INS.02, SA.INS.03, SA.INS.04, SA.INS.05, SA.INS.06, SA.INS.09, PCD.MCH.03, PCD.LAS.03, DEV.CMM.03, DEV.TRK.*, DEV.ARM.*, DEV.LIN.*, DEV.ARE.*, DEV.PHO.*, DEV.TST.*, DEV.GAU.*, DEV.ENV.*, DEV.CAM.*, DEV.CTL.*]
depends_on: [HAL.IDL, HAL.ACQ, HAL.HOST, HAL.SIM, IO]
cli: [fid device read-*, fid device *-catalog]
mcp: [device_read_*, device_*_catalog]
journal: [device.read_*, device.*_catalog]
updated: 2026-11-16
---

# HAL.DRV: Device drivers and the device-support catalog

## Purpose

The concrete integrations for every non-I++ instrument class in `seeds/devices.md`, each through a named integration path (`SDK`, `FILE` or `STD`), plus the generated device-support page that states each device's status per release. It covers gauges, environment sensors, line-profile sensors, laser trackers and their 6DoF probes, portable arms, structured-light and handheld scanners, photogrammetry systems, total stations, indoor positioning systems, industrial cameras, encoder-retrofit manual CMMs and shop-floor protocols. It is separate because it is the per-device work that scales with the catalog, while the other HAL children are device-independent machinery.

## Scope

**In:**
- The device-support catalog: one entry per DEV row with integration path, tier, capabilities, phase, tested firmware range, conformance status and trust grade reachable
- A generated device-support page per release (supported, experimental, planned, blocked on vendor terms)
- `Gauge` drivers: wired and wireless gauge interfaces, Bluetooth LE gauges, serial and keyboard-wedge input (P2)
- `Environment` drivers: temperature sensors feeding part and machine temperature compensation (P2)
- `LineScan` drivers for line-profile sensors through vendor SDKs (P2 first sensor, others later)
- `PortablePose`, `TactileProbe`, `LineScan` drivers for laser trackers, 6DoF probes, tracker-mounted scanners and portable arms through vendor-built or project-built Tier-2 hosts (P3)
- `AreaScan` integration for structured-light and handheld scanners: the file path (exported meshes and clouds) in P1b, an SDK path later
- Photogrammetry, total stations, theodolites and indoor positioning systems: exported coordinate files or SDK hosts (P3)
- `Camera` drivers through an open camera standard for vision measurement (P4)
- Encoder-interface drivers for manual CMMs and retrofits (P3)
- Shop-floor protocols (OPC UA, MTConnect) as monitoring and publishing paths that can never create device I/O or motion commands (P4)
- An instrument profile (`instrument-profile/v1`) per catalogued device class, with spec-sheet MPEs

**Out** (and which node owns it instead):
- I++ CMM controllers, rotary tables and probe heads reported by the controller (owned by `HAL.IPP`)
- Simulated devices (owned by `HAL.SIM`)
- Parsing exported mesh, cloud and coordinate files (owned by `IO`; this node maps them onto device provenance)
- Live probing and scanning workflows that use these drivers (owned by `HAL.LIVE`)
- Temperature compensation of results (owned by `CMM` and `UNC`; this node supplies the observations)
- Network adjustment of multi-station tracker or total-station data (owned by `LVM`)

## Requirements

- **REQ-HAL.DRV-001** Every row in `seeds/devices.md` other than DEV.CMM.01, DEV.CMM.02, DEV.CMM.04, DEV.CMM.05 and DEV.SIM.* shall have a catalog entry in this subsystem naming its integration path, tier, capabilities and target phase.
- **REQ-HAL.DRV-002** When a release is built, the system shall generate the device-support page from the catalog and conformance reports, listing each DEV row with status `supported | experimental | planned | blocked`, and the page shall never be hand-edited.
- **REQ-HAL.DRV-003** Every driver in this subsystem shall pass its capability's conformance profile against the matching simulator in `HAL.SIM` before its catalog status can be `supported`.
- **REQ-HAL.DRV-004** When a gauge reading arrives through a `Gauge` driver, the system shall journal it with gauge identity, resolution, calibration reference and device timestamp; a keyboard-wedge input with no device identity shall be recorded as `entered` provenance, not as a device observation.
- **REQ-HAL.DRV-005** When a scanner or portable device is integrated through the `FILE` path, imported observations shall carry trust grade `declared` and the chosen instrument profile by digest.
- **REQ-HAL.DRV-006** If a catalogued device needs a vendor SDK whose terms are unverified, then its catalog status shall be `blocked` and the page shall state the reason, until a maintainer records the terms.
- **REQ-HAL.DRV-007** Shop-floor protocol integrations shall be publish-or-monitor only and shall expose no path that creates a D1 or M1 command.
- **REQ-HAL.DRV-008** Environment drivers shall report temperature in kelvin with the sensor's stated uncertainty and sampling time, at a rate of at least 1 Hz per sensor [unverified].

## Interfaces

- **Offers:** device-catalog data and the generated support page; drivers implementing HAL.IDL capabilities; `device.read_gauge@1` (D1), `device.read_environment@1` (D1), `device.list_catalog@1` (R0), `device.show_catalog@1` (R0); instrument profiles per device class.
- **Consumes:** HAL.IDL contracts; HAL.ACQ stamping; HAL.HOST tiers and SDK; HAL.SIM simulators and conformance; IO readers for the FILE path.

## Decomposition hints

CAT (catalog and generated support page), GAU (gauges), ENV (environment sensors), LIN (line-profile sensors), TRK (trackers and 6DoF probes), ARM (portable arms and arm scanners), ARE (structured-light and handheld scanners), LVI (photogrammetry, total stations, indoor positioning), MISC (cameras, encoder retrofits, shop-floor protocols). That is nine; the expander may group the P3–P4 classes to stay within limits.

## Open questions

- All DEV rows are `[unverified]`: protocol and SDK terms for trackers, arms, scanners and total stations are unknown (founder-decisions D gaps).
- SA.INS.06 (indoor GPS-style systems) has no DEV row; this node covers it, but a DEV row should be added to the seed catalog by its owner.
- Concept doc 08 suggests a later I++ path for vision CMMs (camera objects in I++ 3.0); if that lands, DEV.CAM.01 and PCD.LAS.03 may move to `HAL.IPP`.

## Children

_None yet: status `open`._
