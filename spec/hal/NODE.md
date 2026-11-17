---
id: HAL
title: Instrument integration (hardware abstraction)
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-002, REQ-FID-003, REQ-FID-011, REQ-FID-014, REQ-FID-024]
covers: [SA.INS.*, PW.PRB.*, PW.SCN.*, PCD.MCH.*, PCD.LAS.01, PCD.LAS.03, DEV.*]
depends_on: [CORE]
cli: [fid device *]
mcp: [device_*]
journal: [device.*]
updated: 2026-11-16
---

# HAL: Instrument integration

## Purpose

A vendor-neutral hardware abstraction layer:
- typed device capabilities;
- driver tiers (in-process Rust, out-of-process hosts, WASM readers);
- an I++ DME client;
- a simulator for every device class;
- instrument I/O transcripts and playback;
- the provenance stamp on every observation.

## Scope

**In:**
- Device base and capabilities: TactileProbe, CncMotion, PortablePose, LineScan, AreaScan, HeightMap, Profile, Gauge, Environment, Rotary, Camera
- Driver tiers and the driver host protocol
- I++ DME client
- Simulators (I++ simulator levels, synthetic scanners and trackers)
- Live probing and scanning sessions with portable devices
- Leapfrog bookkeeping at the device level
- Instrument field checks
- Transcripts and playback
- Pairing ceremony
- Provenance stamp (attested, declared or unknown, plus `simulated`)

**Out:**
- Motion approval (CMM.EXEC)
- Network adjustment (LVM)
- Point processing (SCAN)

## Requirements

- **REQ-HAL-001** Every device shall implement the Device base, and every capability it claims, through a versioned interface definition.
- **REQ-HAL-002** Every device class shall have a simulator that implements the same interface, so all workflows are testable without hardware.
- **REQ-HAL-003** The system shall include an I++ DME client that drives conforming CMM controllers, with conformance tests against the simulator.
- **REQ-HAL-004** Every observation shall carry a provenance stamp. Observations from simulators shall carry an indelible `simulated` flag and shall never be released as measurements.
- **REQ-HAL-005** Instrument I/O shall be recordable as transcripts at the HAL boundary and replayable as a device.
- **REQ-HAL-006** Drivers shall be installable plugins (in-process, out-of-process host or WASM), and a failing driver shall not crash the engine.
- **REQ-HAL-007** Pairing a driver host or station shall require a local ceremony (administrator + physical presence + short-authentication-string comparison).
- **REQ-HAL-008** Live probing and scanning sessions shall stream observations to CORE as journaled commands at device rate, with back-pressure.
- **REQ-HAL-009** Every device in `seeds/devices.md` shall be owned by a HAL node with a named integration path (IPP, SDK, FILE or STD), and a generated device-support page shall list each device's status per release.

## Interfaces

- **Offers:** HAL interface definitions; `device.*` commands (D1 request only, I1 exposed); driver SDK; simulators; transcript format.
- **Consumes:** CORE.

## Decomposition hints

IDL (device interfaces), HOST (driver tiers and host protocol), IPP (I++ client), SIM (simulators), LIVE (live probing and scanning sessions), XSCR (transcripts and playback), PAIR (pairing and provenance).

## Open questions

- Vendor SDK terms (trackers, arms, scanners) are unverified (founder-decisions D gaps).

## Split rationale

The split is by **layer of the device stack**, with the per-device work isolated in one child. Device-independent machinery comes first: the contract (IDL), the engine-side runtime that stamps and journals (ACQ), how drivers run and are isolated (HOST), the security boundary (PAIR), and the recording layer (XSCR). Then the two test and motion anchors: the I++ client (IPP), the one audited motion path, and the simulators and conformance suites (SIM), which every driver is tested against. Last come the consumers of that machinery: concrete non-I++ drivers and the device-support catalog (DRV), and the portable live workflows (LIVE).

This keeps every DEV row in exactly one child (IPP for I++ controllers, SIM for simulated devices, DRV for everything else) and lets device classes be added without touching the runtime.

Alternatives considered:
- **Split by device class** (CMM, tracker, arm, scanner, gauge...). Rejected: it duplicates sessions, stamping, transcripts and isolation in every class.
- **Split by driver tier.** Rejected: tiers are an isolation mechanism, not a responsibility; one device class can move between tiers.
- **Provenance stamp in its own child.** Rejected: its schema belongs with the contracts (IDL) and its writing with the single writer (ACQ); trust grading sits with pairing (PAIR).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.IDL | Device interface definitions and capability contracts | 001 | none |
| HAL.ACQ | Device runtime and acquisition manager | 004 008 | none |
| HAL.HOST | Driver tiers, driver host protocol and driver SDK | 001 006 | none |
| HAL.IPP | I++ DME client for CMM controllers | 001 003 009 | PCD.MCH.01/02/04/05/06, PCD.LAS.01, PW.PRB.05, DEV.CMM.01/02/04/05 |
| HAL.SIM | Simulators and conformance suites | 002 003 004 009 | DEV.SIM |
| HAL.XSCR | Instrument I/O transcripts and playback | 005 | none |
| HAL.PAIR | Pairing ceremony and driver trust grades | 004 007 | none |
| HAL.DRV | Device drivers and the device-support catalog | 001 009 | SA.INS.01-06/09, PCD.MCH.03, PCD.LAS.03, DEV.CMM.03, DEV.TRK/ARM/LIN/ARE/PHO/TST/GAU/ENV/CAM/CTL |
| HAL.LIVE | Live probing and scanning sessions with portable devices | 002 008 | PW.PRB.01-04/06, PW.SCN, SA.INS.07/08 |
