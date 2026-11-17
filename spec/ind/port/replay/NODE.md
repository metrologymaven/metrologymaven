---
id: IND.PORT.REPLAY
title: Re-running a routine on a new part
level: 3
parent: IND.PORT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PORT-005]
covers: [PCD.IND.03]
depends_on: [IND.PORT.GUIDE, CORE]
cli: [fid pack start-portable-run]
mcp: [pack_start_portable_run]
journal: [pack.start_portable_run@1]
updated: 2026-11-16
---

# IND.PORT.REPLAY: Re-running a routine on a new part

## Purpose

Starts a new run of a recorded routine version on a new part: the same steps in the same order with the same prompts, as a re-targeted replay (CONSTITUTION §4) whose new points come from the live session, linked to the routine version and the new part's run in the project.

## Scope

**In:**
- `pack.start_portable_run@1` (W2): create a run bound to a routine version and a part
- Re-targeted replay semantics: recorded steps and settings, new data
- Comparing a run's step sequence with the routine version (skipped or reordered steps are refused)

**Out** (and which node owns it instead):
- Executing each step (owned by `IND.PORT.GUIDE`)
- Replay engine and outcomes (owned by `CORE`)

## Requirements

- **REQ-IND.PORT.REPLAY-001** When a recorded routine is run on a new part, the pack shall present the steps of the selected routine version in their recorded order with the same prompts and checks.
- **REQ-IND.PORT.REPLAY-002** A routine run shall be recorded as a re-targeted replay under CONSTITUTION §4, referencing the routine version digest and the new run in the project container.
- **REQ-IND.PORT.REPLAY-003** If an operator attempts to skip or reorder a step, then the pack shall refuse unless the routine version marks that step optional.
- **REQ-IND.PORT.REPLAY-004** If the routine version is agent-authored and not adopted by a human, then the pack shall mark the run's results as not releasable.

## Interfaces

- **Offers:** `pack.start_portable_run@1` (W2).
- **Consumes:** `IND.PORT.ROUTINE` versions; `IND.PORT.GUIDE` step execution; CORE re-targeted replay and project runs.

## Decomposition hints

Likely one leaf once CORE's re-targeted replay interface is fixed.

## Open questions

- None.

## Children

_None yet: status `open`._
