---
id: IO.DMIS.PROG.PLAN
title: DMIS sensors, measurement blocks and outputs to QIF resources and plan items
level: 4
parent: IO.DMIS.PROG
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.PROG-004]
covers: []
depends_on: [IO.DMIS.PROG.TOL, IO.QIF.GEN, IO.QIF.IDS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PROG.PLAN: Sensors, measurement blocks and outputs to QIF

## Purpose

Maps the parts of a DMIS program that say what is measured and with what: sensor and probe definitions to QIF measurement resources, measurement blocks to QIF plan items that measure a feature (with the planned points kept as nominal hit points), and output requests to plan items that evaluate a characteristic. Motion, alignment and control flow stay in the program model for `CMM.GEN.DMIS`; this leaf only produces the QIF plan view.

## Scope

**In:**
- `SensorDef` → QIF measurement resource (probe tip diameter and type as authored) [unverified: QIF resource types used]
- `MeasBlock` → plan item "measure feature" with planned point list (positions and approach vectors as lexemes), linked to the QIF feature for its label
- `Output` requests → plan items "evaluate characteristic" for the referenced tolerance labels
- Plan item order = program order; every item references its program line
- Plan items that depend on control flow (inside `IF` or `DO`) flagged `conditional` with the enclosing construct's line

**Out:**
- Motion paths, alignment and run-time logic (CMM, AUTO)
- Features and characteristics (`.FEAT`, `.TOL`)

## Requirements

- **REQ-IO.DMIS.PROG.PLAN-001** Each measurement block shall create one plan item linked to the QIF feature of its label, carrying the planned points and the program line.
- **REQ-IO.DMIS.PROG.PLAN-002** Each output request shall create one plan item per referenced characteristic, carrying the program line.
- **REQ-IO.DMIS.PROG.PLAN-003** A plan item created inside a control-flow construct shall be flagged `conditional` with the line of the enclosing construct.
- **REQ-IO.DMIS.PROG.PLAN-004** Each sensor definition shall create one QIF measurement resource with its authored parameters and program line.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/parse/gexec/program.dmi`, when mapped after FEAT and TOL, then one resource, one measure item with 4 planned points and one evaluate item exist in program order, each with its line.
- **AC-2** Given `fixtures/io/dmis/prog/plan/loop_meas.dmi` (a measurement block inside `DO`), then its plan item is `conditional` with the `DO` line.
- **AC-3** Given an output request naming an undefined tolerance label, then no item is created and `label_unresolved` is reported with the line.

## Design

- **Registry commands:** none (library).
- **Data structures:** `PlanMapOut { resources, items: Vec<(PlanItem, SourceRef, Option<Conditional>)>, issues }`.
- **Algorithm:** walk model nodes in order with a control-flow stack.
- **Errors:** none fatal.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-dmis`.

## Test fixtures

Project-written programs under `fixtures/io/dmis/prog/plan/` (CC-BY-4.0) plus GEXEC fixtures.

## Build prompt

> In crate `fid-io-dmis`, map `SensorDef`, `MeasBlock` and `Output` nodes of a `DmisProgram` to `fid-qif-model` measurement resources and plan items in program order: measure items linked to features with planned points as lexemes, evaluate items per referenced characteristic, every item with its program line, items inside IF/DO flagged `conditional` with the construct's line, undefined labels → `label_unresolved`. Leave motion, alignment and flow in the model. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-3.

## Open questions

- Which QIF plan and resource types represent DMIS sensors and measurement blocks best [unverified]; to confirm against the QIF 3.0 schema when `IO.QIF.GEN` lands.

## Children

_Leaf: none._
