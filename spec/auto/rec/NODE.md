---
id: AUTO.REC
title: Record to program and the release lifecycle
level: 2
parent: AUTO
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO-006, REQ-AUTO-007]
covers: [GOM.AUT.02, PW.AUT.01]
depends_on: [CORE, AUTO.PROG]
cli: [fid program record-*, fid program from-journal, fid program register, fid program release, fid program deprecate, fid script register, fid script release, fid script deprecate]
mcp: [program_record_*, program_from_journal, program_register, program_release, program_deprecate, script_register, script_release, script_deprecate]
journal: [program.record_*, program.from_journal, program.register, program.release, program.deprecate, script.register, script.release, script.deprecate]
updated: 2026-11-16
---

# AUTO.REC: Record to program and the release lifecycle

## Purpose

The "record a session, edit it, release it" path. It turns a span of the semantic journal (or a live macro recording) into a draft Command Program a human can edit, lifts session-specific inputs into program parameters, and moves programs and scripts through the lifecycle draft → registered → released → deprecated. Release is a controlled-record change (W3) that only a human performs; a released program is immutable and identified by content.

## Scope

**In:**
- Macro recording: start, pause, stop; recording a journal span into a draft program
- Converting an existing journal range into a draft program
- Parameter lifting: replacing session-specific inputs (data handles, selections, nominal values) with program parameters
- Selection translation: recorded index-set selections into semantic queries where one exists, flagged when none does
- Lifecycle states and transitions for Command Programs and scripts
- Content-addressed identity and version of registered and released artifacts
- Provenance of agent-authored drafts (C5) through the lifecycle

**Out** (and which node owns it instead):
- Journal storage and spans (`CORE`)
- Program format and editing (`AUTO.PROG`)
- Re-targeted runs and run comparison (`AUTO.RUN`)
- Semantic-query definitions for selections (`GEO`)
- Signing a released artifact (`TRUST`)
- UI-level action traces (`AUTO.CTRL`)

## Requirements

- **REQ-AUTO.REC-001** When a recording stops, the system shall produce a draft Command Program containing one step per W1 or W2 command journaled during the recording, in journal order.
- **REQ-AUTO.REC-002** Given any contiguous range of a journal branch, the system shall produce a draft Command Program that, replayed in exact mode against the same starting state, reproduces that range with outcome `IDENTICAL`.
- **REQ-AUTO.REC-003** When converting, the system shall turn each input data handle of the recorded session into a named program parameter, and shall list every value it could not lift.
- **REQ-AUTO.REC-004** If a recorded selection is an index set with no equivalent semantic query, then the draft shall keep the index set and mark that step as not re-targetable.
- **REQ-AUTO.REC-005** Programs and scripts shall move only through the transitions draft → registered → released → deprecated, plus registered → draft, and any other transition shall be refused.
- **REQ-AUTO.REC-006** Release shall be a W3 command, so agents may only request it and only a human may perform it.
- **REQ-AUTO.REC-007** A released program or script shall be immutable and identified by the digest of its canonical form, and any change shall produce a new draft with a new version.
- **REQ-AUTO.REC-008** If a draft contains agent-authored steps, then release shall be refused until a human has adopted each agent-authored step (C5).
- **REQ-AUTO.REC-009** When a program or script is deprecated, new runs of it shall be refused unless a human overrides, and its earlier run records shall remain verifiable.

## Interfaces

- **Offers:** `program.record_start@1`, `program.record_stop@1`, `program.from_journal@1`, lifecycle commands for programs and scripts; lifecycle state queries used by `AUTO.RUN` and `AUTO.SCRIPTED`.
- **Consumes:** CORE journal and replay; `AUTO.PROG` format and checker; GEO semantic queries.

## Decomposition hints

MACRO (live recording), CONV (journal range to draft), LIFT (parameter lifting and selection translation), LIFE (lifecycle state machine and identities), ADOPT (agent provenance gate on release).

## Open questions

- Should a "registered" artifact (usable by others, not released) require human action, or can an agent register its own draft? This node allows agent registration (W2) and restricts only release.

## Children

_None yet: status `open`._
