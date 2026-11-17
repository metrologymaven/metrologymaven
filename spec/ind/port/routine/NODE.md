---
id: IND.PORT.ROUTINE
title: Guided routine model and authoring
level: 3
parent: IND.PORT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PORT-001, REQ-IND.PORT-002]
covers: [PCD.IND.03]
depends_on: [IND.CONTRACT.MANIFEST]
cli: [fid pack create-portable-routine, fid pack edit-portable-routine]
mcp: [pack_create_portable_routine, pack_edit_portable_routine]
journal: [pack.create_portable_routine@1, pack.edit_portable_routine@1]
updated: 2026-11-16
---

# IND.PORT.ROUTINE: Guided routine model and authoring

## Purpose

The `GuidedRoutine` document object: a versioned, ordered list of steps, each with an operator prompt, an optional image, an expected feature type, a minimum point count and an acceptance check. Owns creating and editing routines; running them belongs to GUIDE and REPLAY.

## Scope

**In:**
- Element type `GuidedRoutine` registered through the portable pack manifest
- Step fields: prompt (untrusted text, CONSTITUTION C6), optional image asset, expected feature type, minimum point count, acceptance check (for example form residual limit), optional target characteristic
- Routine versioning: every edit produces a new version; runs reference a version
- Create and edit commands (W2; agents write to an agent branch)
- Validation of steps

**Out** (and which node owns it instead):
- Running steps and operator prompts (owned by `IND.PORT.GUIDE`)
- Re-running on a new part (owned by `IND.PORT.REPLAY`)
- Feature-type recognition (owned by `IND.PORT.RECOG`)

## Requirements

- **REQ-IND.PORT.ROUTINE-001** The `GuidedRoutine` element type and its commands shall be registered only through the portable-device workflow pack manifest.
- **REQ-IND.PORT.ROUTINE-002** Each step shall declare an operator prompt, an expected feature type, a minimum point count of at least the type's mathematical minimum, and an acceptance check, and the pack shall refuse a routine with any step missing these.
- **REQ-IND.PORT.ROUTINE-003** Every edit to a routine shall produce a new routine version with a content digest, and earlier versions shall stay readable.
- **REQ-IND.PORT.ROUTINE-004** Step prompts and image captions shall be stored and handled as untrusted text and shall never be interpreted as commands (CONSTITUTION C6).
- **REQ-IND.PORT.ROUTINE-005** When an agent creates or edits a routine, the change shall be written to an agent branch and provenance-tagged, and an agent-authored routine shall need human adoption before it is run for released results (CONSTITUTION C5).

## Interfaces

- **Offers:** element type `GuidedRoutine`; create and edit commands (W2); `RoutineStep` type for GUIDE and REPLAY.
- **Consumes:** CORE document model and asset storage; `IND.CONTRACT.MANIFEST`.

## Decomposition hints

MODEL (types and validation), EDIT (commands and versioning).

## Open questions

- Whether routines may branch (conditional steps) in the first release; assumed linear only.

## Children

_None yet: status `open`._
