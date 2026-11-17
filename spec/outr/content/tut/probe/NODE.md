---
id: OUTR.CONTENT.TUT.PROBE
title: Probe app tutorials
level: 4
parent: OUTR.CONTENT.TUT
kind: feature
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.TUT-001, REQ-OUTR.CONTENT.TUT-003, REQ-OUTR.CONTENT.TUT-004, REQ-OUTR.CONTENT.TUT-005]
covers: []
depends_on: [OUTR.CONTENT.TUT.CHECK, OUTR.CONTENT.PUB]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, ECO.APPS.LIST]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.TUT.PROBE: Probe app tutorials

## Purpose

The tutorials for the Probe app: writing a CMM measurement program, simulating it, releasing it, and running it through the approval station, each reproducible from an example project recorded against the CMM simulator. They start when Probe's core workflow exists (P2).

## Scope

**In:**
- At least one tutorial on programming and simulation, and one on release and execution with the approval station
- Example projects and journals recorded against the simulator only

**Out** (and which node owns it instead):
- The PC-DMIS migration guide (owned by `OUTR.CONTENT.MIG.PCD`)
- The simulator (owned by `HAL.SIM`; read as a release artifact)
- Check tooling (owned by `OUTR.CONTENT.TUT.CHECK`)

## Requirements

- **REQ-OUTR.CONTENT.TUT.PROBE-001** When the Probe app reaches alpha, at least one tutorial with `app: probe` shall pass `tut-check --run` on the targeted release.
- **REQ-OUTR.CONTENT.TUT.PROBE-002** Every Probe tutorial journal shall be recorded against the CMM simulator, so that it replays without hardware.
- **REQ-OUTR.CONTENT.TUT.PROBE-003** Any tutorial step that releases or runs a program shall describe the human approval steps of D-008 and shall not show an agent performing them.
- **REQ-OUTR.CONTENT.TUT.PROBE-004** Each Probe tutorial shall state `duration_min` and its prerequisites and pass `content-check`.

## Interfaces

- **Offers:** Probe tutorials (registered in OUTR.DOCS.FRAME guide slots).
- **Consumes:** `tut-check`, `content-check` (OUTR tooling); ECO example projects; release binaries including the simulator.

## Decomposition hints

PROG (programming and simulation), RUN (release and execution); each becomes a leaf like OUTR.CONTENT.TUT.INSPECT once Probe's commands are leaves.

## Open questions

- Not in launch-1 (Probe is excluded).

## Children

_None yet: status `open`._
