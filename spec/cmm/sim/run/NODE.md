---
id: CMM.SIM.RUN
title: Simulation driver over the I++ simulator
level: 3
parent: CMM.SIM
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.SIM-001, REQ-CMM.SIM-004, REQ-CMM.SIM-008]
covers: [PCD.PTH.04]
depends_on: [HAL.SIM, CMM.PATH.PLAN, CMM.COLL.CHECK, CMM.SIM.HITS, CMM.SIM.REPORT]
cli: [fid cmm simulate-program]
mcp: [cmm_simulate_program]
journal: [cmm.simulate_program@1]
updated: 2026-11-16
---

# CMM.SIM.RUN: Simulation driver over the I++ simulator

## Purpose

Runs a compiled motion plan to completion against the HAL I++ simulator (ideal-kinematics fidelity L1) and the CMM models, with no device connection: it streams the plan's moves to the simulator, collects per-move collision verdicts from the collision checker and simulated hits from `CMM.SIM.HITS`, estimates cycle time, and hands everything to `CMM.SIM.REPORT`. It is the one simulation entry point, exposed to agents as an I1 command.

## Scope

**In:**
- `cmm.simulate_program@1` (I1): start, drive and finish a simulation of a plan
- Session management with the HAL simulator (start, configure with machine envelope, stop)
- Per-move verdict collection via `CMM.COLL.CHECK`
- Cycle-time estimate from move lengths, velocities and a configurable acceleration model
- Determinism controls: simulator version and seed recorded; no wall-clock dependence

**Out** (and which node owns it instead):
- The simulator binary and fidelity levels (owned by `HAL.SIM`)
- Report model and digest (owned by `CMM.SIM.REPORT`)
- Simulated hit generation (owned by `CMM.SIM.HITS`)
- Animation timeline (owned by `CMM.SIM.TIME`)

## Requirements

- **REQ-CMM.SIM.RUN-001** When asked to simulate a plan, the system shall run every move against the HAL I++ simulator without any device connection and shall refuse to start if the target handle is not a simulator handle.
- **REQ-CMM.SIM.RUN-002** The system shall obtain a collision verdict for every move from the collision checker and pass all verdicts, simulated hits and the cycle-time estimate to the report builder.
- **REQ-CMM.SIM.RUN-003** The system shall estimate cycle time from move lengths, commanded velocities and a trapezoidal acceleration model with configurable acceleration, recorded in the report.
- **REQ-CMM.SIM.RUN-004** Re-simulating the same plan with the same models, simulator version and settings shall produce a byte-identical report.
- **REQ-CMM.SIM.RUN-005** The simulation command shall be class I1 and available to agents over MCP.

## Interfaces

- **Offers:** `cmm.simulate_program@1` (I1).
- **Consumes:** `HAL.SIM` simulator handles (`device.start_sim@1`); `CMM.PATH.PLAN` plan; `CMM.COLL.CHECK`; `CMM.SIM.HITS`; `CMM.SIM.REPORT`.
- **Journal:** `cmm.simulate_program@1`.

## Decomposition hints

DRIVER (simulator session and move streaming), TIME (cycle-time estimate), CMD (command and orchestration).

## Open questions

- Whether the simulator is driven over the real I++ protocol (exercising the client) or in process is a `HAL.SIM` choice; driving over the protocol tests more of the real path.

## Children

_None yet: status `open`._
