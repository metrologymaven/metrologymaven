---
id: HAL.SIM
title: Simulators and conformance suites
level: 2
parent: HAL
kind: subsystem
status: open
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL-002, REQ-HAL-003, REQ-HAL-004, REQ-HAL-009]
covers: [DEV.SIM.01, DEV.SIM.02]
depends_on: [HAL.IDL, HAL.XSCR]
cli: [fid device *-sim, fid device *-conformance]
mcp: [device_*_sim, device_*_conformance]
journal: [device.*_sim, device.*_conformance]
updated: 2026-11-16
---

# HAL.SIM: Simulators and conformance suites

## Purpose

A simulator for every device class and a conformance suite for every capability. The centrepiece is an open I++ DME simulator with fidelity levels from scripted responses to error-injected kinematics, plus synthetic trackers, arms, line scanners, area scanners, gauges and environment sensors. The conformance suites test any driver or client (ours or a third party's) against these simulators and emit signed reports. It is separate because hardware-free testing is a contract with every other HAL child and with outside driver authors.

## Scope

**In:**
- I++ DME simulator: protocol engine, Cartesian kinematics (rotary tables and heads later), virtual workpiece with an unknown placement, stylus contact model and collision detection
- Fidelity levels: L0 scripted, L1 ideal, L2 seeded error and noise injection, L3 controller replay built on transcript playback
- Dialect profiles of known controller behaviour (public or own observation only)
- Synthetic simulators for every other capability: PortablePose (tracker, arm), LineScan, AreaScan, HeightMap, Profile, Gauge, Environment, Rotary, Camera
- Conformance suites per capability: client conformance against the simulators, and server conformance a controller maker can run against a real controller
- Signed conformance reports (subject digests, suite version, per-test outcomes)
- Declaring every simulator source as simulated to the runtime

**Out** (and which node owns it instead):
- The I++ client under test (owned by `HAL.IPP`)
- Generic transcript recording and playback (owned by `HAL.XSCR`)
- Stamping the `simulated` flag on batches (owned by `HAL.ACQ`)
- Offline CMM program simulation with machine models and path display (owned by `CMM`)
- Signature formats and keys (owned by `TRUST`)

## Requirements

- **REQ-HAL.SIM-001** For each capability defined in `HAL.IDL`, there shall be at least one simulator that implements the same capability package and passes that capability's conformance profile.
- **REQ-HAL.SIM-002** The I++ simulator shall offer fidelity levels L0 (scripted), L1 (ideal kinematics and geometry), L2 (seeded error injection) and L3 (replay of a recorded controller session).
- **REQ-HAL.SIM-003** Given the same seed and inputs, an L2 simulator run shall produce bit-identical outputs on the same platform (determinism class D2).
- **REQ-HAL.SIM-004** When a simulated probe touches the virtual workpiece outside a commanded measurement move, the I++ simulator shall report a collision event and stop the move.
- **REQ-HAL.SIM-005** Every simulator shall declare itself as simulated in its capability declaration, so the runtime stamps its batches `simulated = true`.
- **REQ-HAL.SIM-006** The conformance suite shall emit a signed machine-readable report recording the subject digests, the suite version and the outcome of every test, and shall measure hold latency, look-ahead depth and behaviour on disconnect for I++ clients.
- **REQ-HAL.SIM-007** The I++ simulator and the conformance suites shall be releasable under a permissive licence independent of the main product licence, and shall contain no text from the I++ specification [unverified pending D-001].
- **REQ-HAL.SIM-008** All simulators shall run without hardware in CI on every supported platform, and simulator-level I/O shall be class I1 and exposed on MCP.

## Interfaces

- **Offers:** `fid-ipp-sim`-style simulator binary; synthetic device simulators; conformance suites and profiles; `device.start_sim@1` (I1), `device.stop_sim@1` (I1), `device.run_conformance@1` (I1), `device.show_conformance@1` (R0); device-catalog rows DEV.SIM.01 and DEV.SIM.02.
- **Consumes:** HAL.IDL packages and conformance profiles; HAL.XSCR playback (for L3); TRUST signing for reports.

## Decomposition hints

IPPSIM (I++ protocol engine and kinematics), WORK (virtual workpiece and contact model), ERR (L2 error and noise injection), PORT (tracker and arm simulators), SCANSIM (line and area scanner simulators), MISC (gauge, environment, profile, height map, camera simulators), CONF (conformance suites and signed reports).

## Open questions

- The seed catalog places DEV.SIM.01 in P0, while concept doc 08 makes the L0 skeleton a P1b stretch and L1/L2 P2. The node phase is P0 to match the earliest deliverable; children will carry their own phases.
- Licence of the simulator and suite depends on D-001; concept doc 08 proposes a permissive licence under every option (H-D13).

## Children

_None yet: status `open`._
