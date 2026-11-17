---
id: AUTO.CTRL.TRACE
title: UI action-trace record and playback
level: 3
parent: AUTO.CTRL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.CTRL-005, REQ-AUTO.CTRL-011]
covers: []
depends_on: [AUTO.CTRL.UIOPS]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CTRL.TRACE: UI action-trace record and playback

## Purpose

Records UI-level input (pointer, keys, widget targets, timing) as an action trace and plays it back, for GUI tests and demos. A trace is not a program: playback only re-sends UI input, so the document changes only through the registry commands that input triggers, and the semantic journal (not the trace) remains the record.

## Scope

**In:**
- Ops `trace.start`, `trace.stop`, `trace.play`
- Trace format: widget-targeted events with relative timing, versioned
- Playback synchronisation (wait for widget, timeouts) and divergence reporting

**Out:**
- Semantic recording into Command Programs (`AUTO.REC`)
- Input injection (`AUTO.CTRL.UIOPS`)

## Requirements

- **REQ-AUTO.CTRL.TRACE-001** A trace shall contain only UI input events targeted by widget ID and relative time, and no document state.
- **REQ-AUTO.CTRL.TRACE-002** Playback shall reach the document only through registry commands triggered by the replayed input.
- **REQ-AUTO.CTRL.TRACE-003** If a target widget is absent or disabled during playback, then playback shall stop at that event and report its index and the reason.
- **REQ-AUTO.CTRL.TRACE-004** Traces shall be read and written only through root handles.

## Interfaces

- **Offers:** the trace ops and trace format.
- **Consumes:** `AUTO.CTRL.UIOPS`; `AUTO.SURF.ROOTS`.

## Decomposition hints

FMT (trace format), REC (capture), PLAY (playback and synchronisation).

## Open questions

- Whether traces should be captured as commands (journaled) like screenshots, since starting a trace records the user's input. Proposed: no; traces are test artefacts written to a write root.

## Children

_None yet: status `open`._
