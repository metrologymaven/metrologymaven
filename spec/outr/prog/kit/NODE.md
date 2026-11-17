---
id: OUTR.PROG.KIT
title: University teaching kits
level: 3
parent: OUTR.PROG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.PROG-005, REQ-OUTR.PROG-007]
covers: []
depends_on: [OUTR.CONTENT.REPRO, HAL.SIM, PLAT.HW]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.PROG.KIT: University teaching kits

## Purpose

Teaching kits for metrology courses: lesson goals, exercises with expected results, and example projects whose journals replay `IDENTICAL` on the release the kit names, all runnable offline on minimum hardware with simulators in place of instruments. Kits may reuse OUTR.CONTENT tutorials.

## Scope

**In:**
- Kit structure: lesson goals, exercises, expected results, example projects and journals, instructor notes
- Replay check of kit journals on the named release (reusing OUTR.CONTENT.REPRO)
- Offline, minimum-hardware, simulator-only operation

**Out** (and which node owns it instead):
- Datasets offered with kits (owned by `OUTR.PROG.DATA`)
- The replay harness (owned by `OUTR.CONTENT.REPRO`)
- Simulators (owned by `HAL.SIM`)

## Requirements

- **REQ-OUTR.PROG.KIT-001** Each teaching kit shall contain lesson goals, exercises with expected results, instructor notes, and at least one example project with its journal.
- **REQ-OUTR.PROG.KIT-002** Each kit's example journals shall replay with outcome `IDENTICAL` on the release the kit names, checked by the OUTR.CONTENT.REPRO harness.
- **REQ-OUTR.PROG.KIT-003** Each kit shall run with networking disabled and no account on the PLAT minimum hardware profile.
- **REQ-OUTR.PROG.KIT-004** Where an exercise involves an instrument, the kit shall use the HAL simulator for that device class and shall never require motion-capable hardware.
- **REQ-OUTR.PROG.KIT-005** Each expected result in an exercise shall match the value produced by replaying the kit's journal, and a check shall fail on any mismatch.

## Interfaces

- **Offers:** teaching kits.
- **Consumes:** OUTR.CONTENT tutorials and REPRO harness; HAL.SIM; PLAT.HW minimum profile.

## Decomposition hints

One child per course topic (for example FIT basics, GD&T evaluation, CMM programming with the simulator).

## Open questions

- Course topics should be chosen with partner instructors.

## Children

_None yet: status `open`._
