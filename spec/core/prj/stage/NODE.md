---
id: CORE.PRJ.STAGE
title: Stages: several parts or measurements in one project
level: 3
parent: CORE.PRJ
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ-004, REQ-CORE.PRJ-005, REQ-CORE.PRJ-011]
covers: [GOM.PRJ.02, PW.PRJ.02]
depends_on: [CORE.BUS, CORE.PRJ.CONT, CORE.PRJ.TREE]
cli: [fid project add-stage, fid project remove-stage, fid project set-active-stage, fid project list-stages]
mcp: [project_add_stage, project_remove_stage, project_set_active_stage, project_list_stages]
journal: [project.add_stage@1, project.remove_stage@1, project.set_active_stage@1]
updated: 2026-11-16
---

# CORE.PRJ.STAGE: Stages

## Purpose

Lets one project hold many measured instances (parts, serials or repeated measurements) against one shared plan. Each stage has its own measured data and per-element values; the element definitions are shared. Switching the active stage shows that stage's values without recomputing anything whose inputs for that stage are unchanged. Stages are the basis for trend analysis in RPT.

## Scope

**In:**
- Stage entity (ID, label, order, serial and user metadata) under a Run or Plan
- Per-stage data slots for every element; shared definitions
- Add, remove (soft, recorded), reorder and list stages
- Active stage as document state (`project.set_active_stage@1` W2)
- Per-stage value cache so switching does not recompute

**Out** (and which node owns it instead):
- Trend, SPC and multi-stage statistics (owned by `RPT`)
- Recompute when a stage's inputs change (owned by `CORE.DEP`)
- Importing a batch of scans into many stages (owned by `IO` and `AUTO`)

## Requirements

- **REQ-CORE.PRJ.STAGE-001** The project model shall support at least 1,000 stages in one project, each holding its own measured data against the shared plan [unverified that 1,000 suffices for incumbent parity].
- **REQ-CORE.PRJ.STAGE-002** When the active stage changes, the project model shall present every element's value for that stage without recomputing elements whose inputs for that stage are unchanged.
- **REQ-CORE.PRJ.STAGE-003** When a stage is added, every existing element definition shall gain an empty slot for that stage in state `not_computed`.
- **REQ-CORE.PRJ.STAGE-004** When a stage is removed, the project model shall keep its data in the journal and hide it from the active stage list, and the removal shall be undoable.
- **REQ-CORE.PRJ.STAGE-005** Switching the active stage of a 100,000-element project shall complete in under 200 ms when no recompute is needed [unverified until measured].
- **REQ-CORE.PRJ.STAGE-006** Adding, removing, reordering and activating stages shall be registry commands of side-effect class W2 executed through the bus.

## Interfaces

- **Offers:** `StageContext` (active stage, per-stage slot lookup), stage entity type.
- **Commands:** `project.add_stage@1` (W2), `project.remove_stage@1` (W2), `project.reorder_stages@1` (W2), `project.set_active_stage@1` (W2), `project.list_stages@1` (R0).
- **Consumes:** `CORE.BUS`, `CORE.PRJ.CONT`, `CORE.PRJ.TREE`.

## Decomposition hints

MODEL (stage entity and per-stage slots), CMD (stage commands), SWITCH (active stage and cache), SCALE (1,000-stage benchmark).

## Open questions

- Is the active stage document state or view state? This child keeps it as document state (W2) because results and reports depend on it; confirm with VIZ.
- PolyWorks multi-piece projects may differ from GOM stages [unverified]; confirm with seed verification before the MODEL leaf freezes.

## Children

_None yet: status `open`._
