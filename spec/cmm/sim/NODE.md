---
id: CMM.SIM
title: Offline program simulation
level: 2
parent: CMM
kind: subsystem
status: split
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM-004, REQ-CMM-006]
covers: [PCD.PTH.04]
depends_on: [HAL, CMM.PATH, CMM.COLL, CMM.PROBE]
cli: [fid cmm simulate-*, fid cmm *-simulation*]
mcp: [cmm_simulate_*, cmm_*_simulation*]
journal: [cmm.simulate_*@*, cmm.*_simulation*@*]
updated: 2026-11-16
---

# CMM.SIM: Offline program simulation

## Purpose

Runs a compiled motion plan **without hardware** against the I++ simulator and the machine, fixture, part and probe models, and produces a digested **simulation report**: motion timeline, simulated probe hits, collision verdicts and cycle-time estimate. The report serves two readers: the programmer, who watches the animation and fixes the program, and the Motion Executive, whose simulation gate refuses a ticket unless the byte-identical plan has a clean report for the same machine, setup and probe configuration.

## Scope

**In:**
- Simulation runs of a compiled plan against the I++ simulator (ideal-kinematics level) and the CMM models
- Simulated probe hits on the nominal part (marked `simulated`) and simulated scan data
- The simulation report: plan digest, model digests, probe configuration digest, per-move verdicts, unintended contacts, estimated cycle time, report digest
- Animation timeline data for the viewer (poses over time); the viewer renders it
- Stepping, pausing and seeking within a simulation
- The simulation-gate query: does a clean report exist for this exact (plan, machine, setup, probe) tuple?

**Out** (and which node owns it instead):
- The I++ simulator itself and its fidelity levels (owned by `HAL`)
- Collision geometry and verdict computation (owned by `CMM.COLL`)
- Rendering and animation UI (owned by `VIZ`)
- Enforcing the gate at ticket issue and arming (owned by `CMM.EXEC`)
- Error-injected uncertainty simulation (owned by `UNC`)

## Requirements

- **REQ-CMM.SIM-001** The system shall simulate a compiled motion plan to completion without any device connection, using the HAL I++ simulator and the CMM models.
- **REQ-CMM.SIM-002** The system shall produce a simulation report containing the plan digest, machine, fixture, part-placement and probe-configuration digests, per-move collision verdicts, unintended contacts and estimated cycle time, and shall store it under its own content digest.
- **REQ-CMM.SIM-003** Every observation produced by simulation shall carry the indelible `simulated` provenance flag.
- **REQ-CMM.SIM-004** A simulation shall be deterministic: re-simulating the same plan with the same models and simulator version shall produce a byte-identical report.
- **REQ-CMM.SIM-005** The system shall report a simulation as clean only if no move has a `COLLISION`, `MARGIN` or `UNCHECKED` verdict and no unintended contact occurred.
- **REQ-CMM.SIM-006** When queried with a (plan, machine, setup, probe configuration) digest tuple, the system shall return the digest of a clean simulation report for exactly that tuple, or a typed `NO_CLEAN_SIMULATION` result.
- **REQ-CMM.SIM-007** The system shall emit an animation timeline of tool poses at a configurable rate (default 30 Hz [unverified]) so the viewer can play, pause, step and seek the simulation.
- **REQ-CMM.SIM-008** Simulation commands shall be class I1 or R1 and therefore available to agents over MCP.

## Interfaces

- **Offers:** commands such as `cmm.simulate_program@1` (I1), `cmm.get_simulation_report@1` (R0), `cmm.find_clean_simulation@1` (R0); data types SimulationReport, AnimationTimeline.
- **Consumes:** `HAL` I++ simulator (L1 fidelity); `CMM.PATH` compiled plan; `CMM.COLL` verdicts; `CMM.PROBE` configuration.
- **Journal:** `cmm.simulate_*`, `cmm.*_simulation*`.

## Decomposition hints

RUN (simulation driver over the I++ simulator), REPORT (report model and digest), GATE (clean-simulation query), TIMELINE (animation data), HITS (simulated observations).

## Open questions

- Whether the gate should also accept a clean report at a higher simulator fidelity level (L2) as equivalent to L1.

## Split rationale

The split follows the **two readers of a simulation**. RUN, HITS and REPORT produce the evidence: RUN drives the simulator and collects verdicts, HITS produces the `simulated` observations, and REPORT defines the digested artifact and the clean rule. GATE serves the Motion Executive (does a clean report exist for this exact tuple?), and TIME serves the programmer and viewer (poses and events over time). REPORT, GATE and TIME are small, pure and fail-closed, so they are leaves; RUN and HITS stay open because they depend on the HAL simulator and geometry queries.

Alternatives considered:
- **GATE inside REPORT.** Rejected: the gate is what the Motion Executive calls; keeping it a tiny separate query makes the trust path easy to review.
- **TIME inside RUN.** Rejected: the timeline is a read over a stored report and must not require re-simulation.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [CMM.SIM.RUN](run/NODE.md) | Simulation driver over the I++ simulator | 001 004 008 | PCD.PTH.04 |
| [CMM.SIM.REPORT](report/NODE.md) | Simulation report model, clean rule and digest | 002 005 | |
| [CMM.SIM.GATE](gate/NODE.md) | Clean-simulation query for a digest tuple | 006 008 | |
| [CMM.SIM.HITS](hits/NODE.md) | Simulated probe hits and scan data | 003 | |
| [CMM.SIM.TIME](time/NODE.md) | Animation timeline of tool poses | 007 008 | PCD.PTH.04 |
