---
id: CMM
title: CMM programming and execution
level: 1
parent: FID
kind: domain
status: split
phase: P2
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-002, REQ-FID-011, REQ-FID-021]
covers: [PCD.PRB.*, PCD.FEA.*, PCD.SCN.*, PCD.PTH.*, PCD.CAD.03, PW.CMM.*]
depends_on: [HAL, KRN, GDT, GEO, AUTO]
cli: [fid cmm *]
mcp: [cmm_*]
journal: [cmm.*]
updated: 2026-11-16
---

# CMM: CMM programming and execution

## Purpose

Replaces CMM programming suites:
- probe and sensor configuration and qualification;
- measurement strategies for features and scans;
- path planning with clearance and collision checks;
- offline simulation;
- programs authored as Command Programs (from CAD, PMI, templates or imported DMIS);
- safe execution through the Motion Executive and the Approval Station.

## Scope

**In:**
- Probe and sensor configuration: tips, stars, articulating heads, changers, scanning and non-contact sensors
- Probe qualification
- Auto-feature strategies (hits, depth, sampling)
- Scan strategies: linear, patch, perimeter, analog
- Feature generation from CAD and PMI
- Move points and clearance planes
- Path optimisation
- Collision detection against machine, fixture and part models
- Offline simulation
- DMIS-to-Command-Program conversion
- The Motion Executive, Approval Station, run tickets and setup or batch scopes
- Rotary tables
- Temperature compensation inputs

**Out:**
- Device drivers and the I++ client (HAL)
- Program language features: variables, loops, subroutines (AUTO)
- Feature evaluation (KRN, GDT)
- DMIS parsing (IO)

## Requirements

- **REQ-CMM-001** The system shall model probe and sensor configurations and qualify them against a reference sphere, journaling every qualification result.
- **REQ-CMM-002** The system shall generate measurement strategies for each feature type, with configurable sampling, and validate them against minimum-point requirements of the fit method.
- **REQ-CMM-003** The system shall plan paths with clearance planes and move points, and detect collisions against machine, fixture, part and probe models before execution.
- **REQ-CMM-004** The system shall simulate a program offline, showing motion, probe hits and collisions, without hardware.
- **REQ-CMM-005** The system shall create programs from CAD features, from PMI, from templates and from imported DMIS, as Command Programs.
- **REQ-CMM-006** No motion shall be started, continued or resumed through MCP. Motion shall run only from a released program, under a run ticket verified by the Motion Executive and approved at the Approval Station by a human (CONSTITUTION §3, Design Baseline §8).
- **REQ-CMM-007** The first run of a program shall be limited to a setup scope with reduced velocity, and the operator shall be present.
- **REQ-CMM-008** The Motion Executive shall be positioned as machine protection and process assurance, not a safety function, and the documentation shall say so.

## Interfaces

- **Offers:** `cmm.*` commands (planning and simulation: R1, W2; execution requests: M1, request only); probe model; path model.
- **Consumes:** HAL (I++ client, simulator), KRN, GDT, GEO (collision geometry), AUTO (Command Programs).

## Decomposition hints

PROBE (configuration and qualification), STRAT (feature and scan strategies), GEN (program generation from CAD, PMI, templates, DMIS), PATH (paths, clearance, optimisation), COLL (collision models and checks), SIM (offline simulation), EXEC (Motion Executive, Approval Station, tickets).

## Split rationale

The split follows the **pipeline from probe to motion**: what is mounted (PROBE), where to touch (STRAT), what to measure in what order (GEN), how to move (PATH), what could be hit (COLL), proving it offline (SIM) and the only gate to real motion (EXEC). Each stage has one owner and hands a digested artifact to the next. The digests (probe configuration, plan, models, simulation report) are exactly what a run ticket binds, so the split also mirrors the trust chain the Motion Executive verifies.

Two cuts are deliberate:
- **COLL is separate from PATH and SIM**, because the same deterministic collision verdict must be produced in planning, in simulation and in the Motion Executive's validators.
- **EXEC is isolated**, because it is the motion trust boundary (review zone Z3) and must stay small, with no dependency on agent-reachable components.

Alternatives considered:
- **By source of program** (CAD, PMI, DMIS, manual). Rejected: every source needs the same strategy, path, collision and execution machinery, so it would duplicate them.
- **Planning vs execution only** (two children). Rejected: the planning half would be too large to own and would mix the probe model, strategies and geometry checks.
- **Merging SIM into COLL.** Rejected: simulation also produces simulated hits, timing and the gate record, which are not collision questions.

## Open questions

- The released-by-other rule for agent-authored programs (baseline §8) is pending a founder decision. It is carried as an open question in `CMM.EXEC`.
- Rotary tables, articulating-head kinematics and auto-path from PMI are in this domain's scope, but the Design Baseline places parts of them in P3. Children may need phase decisions when they are expanded.
- Temperature compensation inputs are placed in `CMM.EXEC` (run context); the uncertainty contribution stays in `UNC`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [CMM.PROBE](probe/NODE.md) | Probe and sensor configuration and qualification | 001 | PCD.PRB.* |
| [CMM.STRAT](strat/NODE.md) | Feature and scan measurement strategies | 002 | PCD.FEA.*, PCD.SCN.* |
| [CMM.GEN](gen/NODE.md) | Program generation from CAD, PMI, templates and DMIS | 005 | PCD.CAD.03, PW.CMM.02 |
| [CMM.PATH](path/NODE.md) | Path planning, clearance and optimisation | 003 | PCD.PTH.01, PCD.PTH.02, PCD.PTH.05 |
| [CMM.COLL](coll/NODE.md) | Machine, fixture and collision models | 003 | PCD.PTH.03, PCD.PTH.06 |
| [CMM.SIM](sim/NODE.md) | Offline program simulation | 004 006 | PCD.PTH.04 |
| [CMM.EXEC](exec/NODE.md) | Motion Executive, Approval Station and run tickets | 006 007 008 | PW.CMM.01 |
