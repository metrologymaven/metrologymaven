---
id: HAL.IPP
title: I++ DME client for CMM controllers
level: 2
parent: HAL
kind: subsystem
status: split
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL-001, REQ-HAL-003, REQ-HAL-009]
covers: [PCD.MCH.01, PCD.MCH.02, PCD.MCH.04, PCD.MCH.05, PCD.MCH.06, PCD.LAS.01, PW.PRB.05, DEV.CMM.01, DEV.CMM.02, DEV.CMM.04, DEV.CMM.05]
depends_on: [HAL.IDL, HAL.ACQ, HAL.SIM]
cli: [fid device *-cmm]
mcp: [device_*_cmm]
journal: [device.*_cmm]
updated: 2026-11-16
---

# HAL.IPP: I++ DME client for CMM controllers

## Purpose

A Tier-1 client that implements the `CncMotion`, `TactileProbe` and `Rotary` capabilities (and `LineScan` where the controller offers it) against any controller exposing an I++ DME server. It runs inside the machine-side process next to the Motion Executive and is the only code that speaks I++ on the wire. It is separate because it is the single audited motion path, and its conformance is tested in both directions against the open simulator.

## Scope

**In:**
- I++ DME session handling: connect, start and end session, queues, events, errors, reconnection
- Tool and probe information as reported by the controller: tools, stars, articulating heads, changers, scanning probes
- Commands for homing, tool change, positioning, point measurement and scanning, head angles, hold and abort, issued only on behalf of the Motion Executive
- Rotary tables driven through the controller (angle reports and controller-level motion)
- Manual and DCC modes and jog-box events as reported by the controller
- Laser line sensors on a CMM, when exposed through the controller
- Probe-compensation ownership per machine profile (server compensates by default; client compensates as an option)
- Controller dialect profiles (known server behaviours) and the published controller and option matrix
- Mapping I++ responses into stamped `TactileProbe`, `CncMotion` and `Rotary` batches

**Out** (and which node owns it instead):
- Run tickets, approval, lease, validators and the decision to move (owned by `CMM` execution, CMM.EXEC)
- Probe qualification workflow and probe models (owned by `CMM`)
- The I++ simulator and the conformance suite (owned by `HAL.SIM`)
- Non-I++ CMM controllers and encoder retrofits (owned by `HAL.DRV`)
- Vision measurement through cameras (owned by `HAL.DRV`)
- Temperature sensors (owned by `HAL.DRV`)

## Requirements

- **REQ-HAL.IPP-001** The I++ client shall accept motion-class (M1) instructions only from the Motion Executive over local IPC, and shall reject any other caller with a typed error and a journaled refusal.
- **REQ-HAL.IPP-002** The I++ client shall pass every client-conformance test category of the suite in `HAL.SIM` against the simulator at fidelity levels L0 and L1 in CI, on every commit.
- **REQ-HAL.IPP-003** When the I++ connection drops during motion, the client shall report `COMM_LOST` to the Motion Executive within 200 ms of detection and shall send no further motion commands until a new session is opened [unverified until measured on hardware].
- **REQ-HAL.IPP-004** The client shall map each measured point from the controller into a `TactileProbe` batch carrying position, direction vector, tip identity and radius, and a flag stating whether the controller already compensated the point.
- **REQ-HAL.IPP-005** The client shall read the controller's tool, head, changer and rotary-table configuration at session start and journal it by digest, without ever writing probe constants or compensation data to the controller (A0 boundary).
- **REQ-HAL.IPP-006** When the controller reports a manual-mode or jog-box event, the client shall record it as a device event without converting it into a motion command.
- **REQ-HAL.IPP-007** For each supported controller family, the client shall ship a dialect profile and a row in the device-support catalog naming path `IPP`, the tested firmware range and the conformance report digest.

## Interfaces

- **Offers:** `CncMotion`, `TactileProbe`, `Rotary` (and optional `LineScan`) implementations; `device.connect_cmm@1` (D1, request only on MCP), `device.show_cmm@1` (R0), `device.read_config_cmm@1` (D1); controller and option matrix.
- **Consumes:** HAL.IDL contracts; HAL.ACQ stamping; HAL.SIM for tests; instructions from the Motion Executive (CMM).

## Decomposition hints

SESS (session, framing, events, reconnection), TOOLS (tools, heads, changers, rotary), MEAS (point and scan measurement mapping), COMP (compensation ownership), DIAL (dialect profiles and controller matrix), MODES (manual, DCC, jog box).

## Open questions

- The I++ DME 3.0 specification (VDMA 8722:2026-10) is single-user licensed; requirements here cite clause IDs only through founder-written spec cards [unverified].
- Which I++ 3.0 features (camera objects, line-laser streaming, multi-axis) shipping servers implement is unverified.
- Hold latency on real controllers is unknown; REQ-HAL.IPP-003's bound needs a hardware measurement.
- Whether I++ camera objects (vision CMMs) belong here or in `HAL.DRV` is deferred to P4; `HAL.DRV` covers vision for now.

## Split rationale

The split follows **the hint list, by protocol concern**: the session core with the motion-caller guard and connection-loss behaviour (SESS), what the controller reports about its hardware (TOOLS), how measurements become observations (MEAS), who compensates the probe tip (COMP), what the operator does at the machine (MODES), and how controller support is proven and published (DIAL). Every motion instruction stays behind the SESS guard, so the single audited motion path is one module, not spread across children.

Alternatives considered:
- **Split by controller family.** Rejected: one client serves all I++ servers; family differences are data (dialect profiles), not code.
- **COMP inside MEAS.** Rejected: compensation ownership is a machine-profile policy with A0 configuration and depends on spec cards, while point mapping does not.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| HAL.IPP.SESS | I++ session, framing, queues, events and connection loss | 001 003 | PCD.MCH.01, PCD.MCH.02, DEV.CMM.01, DEV.CMM.02 |
| HAL.IPP.TOOLS | Tools, probe heads, changers and rotary tables via the controller | 001 005 | PCD.MCH.04, DEV.CMM.04, DEV.CMM.05 |
| HAL.IPP.MEAS | Point and scan measurement mapping, including laser line sensors | 001 004 | PW.PRB.05, PCD.LAS.01 |
| HAL.IPP.COMP | Probe-compensation ownership per machine profile | 004 005 | none |
| HAL.IPP.MODES | Manual and DCC modes and jog-box events — leaf | 006 | PCD.MCH.05, PCD.MCH.06 |
| HAL.IPP.DIAL | Controller dialect profiles, conformance gate and controller matrix | 002 007 | DEV.CMM.01, DEV.CMM.02 |
