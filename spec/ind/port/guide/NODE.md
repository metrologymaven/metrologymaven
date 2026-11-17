---
id: IND.PORT.GUIDE
title: In-routine guidance and retakes
level: 3
parent: IND.PORT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PORT-003, REQ-IND.PORT-004, REQ-IND.PORT-006]
covers: [PCD.IND.03]
depends_on: [IND.PORT.ROUTINE, IND.PORT.RECOG, HAL, KRN]
cli: [fid pack run-portable-step, fid pack resolve-portable-step]
mcp: [pack_run_portable_step, pack_resolve_portable_step]
journal: [pack.run_portable_step@1, pack.resolve_portable_step@1]
updated: 2026-11-16
---

# IND.PORT.GUIDE: In-routine guidance and retakes

## Purpose

Runs a guided routine step by step on a live HAL session: collects the step's points through HAL session commands, asks recognition for the feature type, prompts the operator to accept, override or retake when the type differs from the expected one, applies the step's acceptance check, prompts a retake on failure, and journals every attempt. Provides the progress data the UI shows.

## Scope

**In:**
- Step execution state machine (waiting for points, recognised, mismatch pending, check failed, accepted)
- Point acquisition only through HAL session commands
- Mismatch resolution: accept recognised type, override to expected type, or retake
- Acceptance checks (form residual, point count, size range) on the fitted feature
- Journaling of every attempt, failed and accepted
- Progress data (current step, attempts, remaining steps)

**Out** (and which node owns it instead):
- Device drivers and session control (owned by `HAL`)
- Feature-type recognition (owned by `IND.PORT.RECOG`)
- Feature fits (owned by `KRN`)
- UI rendering of prompts (owned by `VIZ`)

## Requirements

- **REQ-IND.PORT.GUIDE-001** When the operator completes a step's point collection, the pack shall request feature-type recognition, and if the recognised type differs from the expected type, then the pack shall require the operator to accept, override or retake before continuing.
- **REQ-IND.PORT.GUIDE-002** If a step's acceptance check fails, then the pack shall prompt a retake and shall journal the failed attempt with its check result before the retake.
- **REQ-IND.PORT.GUIDE-003** The pack shall journal every attempt of every step (points digest, recognised type, operator resolution, check result) so the full sequence is replayable.
- **REQ-IND.PORT.GUIDE-004** The pack shall acquire points only by invoking HAL session commands through the registry and shall never issue device I/O itself, so HAL's side-effect classes apply unchanged.
- **REQ-IND.PORT.GUIDE-005** If the HAL session ends or faults during a step, then the pack shall keep the accepted steps, mark the current step interrupted and allow resuming from it.

## Interfaces

- **Offers:** `pack.run_portable_step@1` and `pack.resolve_portable_step@1` (W2); `RoutineRunState` progress type.
- **Consumes:** `IND.PORT.ROUTINE` steps; `IND.PORT.RECOG` recognition; HAL session point commands; KRN fits.

## Decomposition hints

STATE (step state machine), CHECK (acceptance checks), RESUME (interruption and resume).

## Open questions

- Whether agents may drive `pack.run_portable_step@1` at all: it consumes points from a live session but issues no device I/O. Assumed allowed only as an R0 status read for agents; the W2 commands are for the operator [unverified against HAL's MCP policy].

## Children

_None yet: status `open`._
