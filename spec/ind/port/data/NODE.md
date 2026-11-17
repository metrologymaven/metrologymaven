---
id: IND.PORT.DATA
title: Portable routine simulator runs and reference datasets
level: 3
parent: IND.PORT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PORT-007, REQ-IND.PORT-008]
covers: []
depends_on: [IND.CONTRACT.VERIFY, IND.PORT.GUIDE, HAL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.PORT.DATA: Portable routine simulator runs and reference datasets

## Purpose

Proves the portable pack end to end without hardware: example routines run against HAL simulators for an articulated arm and a laser tracker, recorded transcripts shipped as reference datasets with expected feature results, and the pack's manifest declarations.

## Scope

**In:**
- Manifest entries: routine method IDs and maintainer
- Example routines for one arm and one tracker
- Recorded HAL simulator transcripts and expected feature results in `fid.pack.expected@1`
- An end-to-end test that runs each routine against the simulator

**Out** (and which node owns it instead):
- Simulators and transcript format (owned by `HAL`)
- The harness (owned by `IND.CONTRACT.VERIFY`)

## Requirements

- **REQ-IND.PORT.DATA-001** The pack manifest shall declare the routines' method IDs and shall name at least one maintainer.
- **REQ-IND.PORT.DATA-002** The pack shall run every shipped example routine end to end against a HAL simulator for arms and for trackers in CI.
- **REQ-IND.PORT.DATA-003** The pack shall ship recorded simulator transcripts for at least one arm routine and one tracker routine, with expected feature results and tolerances.

## Interfaces

- **Offers:** example routines, transcripts and expected results.
- **Consumes:** HAL simulators and transcript format; `IND.CONTRACT.VERIFY`.

## Decomposition hints

ARM and TRACKER example sets.

## Open questions

- Depends on HAL having arm and tracker simulators by P3.

## Children

_None yet: status `open`._
