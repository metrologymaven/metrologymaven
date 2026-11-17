---
id: HAL.SIM.PORT
title: Portable-device simulators (trackers, arms, 6DoF probes)
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-001, REQ-HAL.SIM-005]
covers: [DEV.SIM.02]
depends_on: [HAL.IDL.SENS, HAL.IDL.CNC, HAL.SIM.WORK, HAL.SIM.IPPSIM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.SIM.PORT: Portable-device simulators (trackers, arms, 6DoF probes)

## Purpose

Synthetic laser trackers, portable arms and 6DoF probes that implement `PortablePose` and `TactileProbe` against the virtual workpiece, with operator-like trigger sequences, reflector states and device moves. They let every live portable workflow (`HAL.LIVE`) and large-volume network test run without hardware, years before vendor SDKs are available.

## Scope

**In:**
- Tracker simulator: pose and point stream, reflector lock and loss, weather readings, station position
- Arm simulator: joint-chain pose stream, ball-probe triggers, articulation for field-check procedures
- 6DoF probe simulator on top of the tracker
- Scripted operator sessions (trigger sequences, device moves for leapfrog)
- Seeded noise through the `HAL.SIM.ERR` hooks when enabled
- Declaring every source simulated

**Out** (and which node owns it instead):
- Arm- and tracker-mounted line scanners (owned by `HAL.SIM.SCANSIM`)
- Live workflows (owned by `HAL.LIVE`)
- Network adjustment (owned by `LVM`)

## Requirements

- **REQ-HAL.SIM.PORT-001** The tracker, arm and 6DoF-probe simulators shall each implement `PortablePose@1` and, where they probe, `TactileProbe@1`, and pass those packages' conformance profiles.
- **REQ-HAL.SIM.PORT-002** The arm simulator shall report ball-probe centres from contact with the virtual workpiece, so compensation by tip radius can be verified against the analytic surface.
- **REQ-HAL.SIM.PORT-003** The tracker simulator shall support scripted station moves and report the same physical targets from each station in that station's frame.
- **REQ-HAL.SIM.PORT-004** Every portable simulator shall declare itself simulated.
- **REQ-HAL.SIM.PORT-005** A scripted session with a fixed seed shall produce bit-identical pose streams on the same platform (D2).

## Interfaces

- **Offers:** simulators registered with the launcher in `HAL.SIM.IPPSIM`; DEV.SIM.02 (tracker and arm part).
- **Consumes:** `HAL.IDL.SENS`, `HAL.IDL.CNC` (`TactileProbe`), `HAL.SIM.WORK`.

## Decomposition hints

TRK (tracker and 6DoF probe), ARM (arm and articulation), OPS (scripted operator sessions).

## Open questions

- Realistic noise for trackers and arms needs published MPE-based models from `UNC.INST` [unverified].

## Children

_None yet: status `open`._
