---
id: AUTO.PROG.EDIT
title: Editor model: command view, summary view and edit commands
level: 3
parent: AUTO.PROG
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-002, REQ-AUTO.PROG-014]
covers: [PCD.PRG.01, PW.AUT.02]
depends_on: [AUTO.PROG.FMT, AUTO.PROG.CHECK]
cli: [fid program edit-*, fid program show]
mcp: [program_edit_*, program_show]
journal: [program.edit_*]
updated: 2026-11-16
---

# AUTO.PROG.EDIT: Editor model: command view, summary view and edit commands

## Purpose

The model behind every program editor. The command view lists steps with their full arguments; the summary view groups steps into readable blocks (per feature, per alignment, per operator interaction). Both are projections of the one canonical program, and every edit, from either view or from an agent, is a journaled `program.edit_*` command that keeps the IDs of untouched steps.

## Scope

**In:**
- View models: command view rows, summary view blocks, mapping between them by step ID
- Edit commands: insert, delete, move, update arguments, wrap in loop or branch, extract subroutine, rename variable (`program.edit_*`, W1 on drafts)
- `program.show@1` (R0) rendering either view
- Undo through the journal; provenance tags on agent edits (C5)

**Out:**
- Widgets (`VIZ`); Python authoring view (D-026)
- Lifecycle (only drafts are editable; `AUTO.REC.LIFE`)

## Requirements

- **REQ-AUTO.PROG.EDIT-001** Every change to a program's content shall be a `program.edit_*` command, journaled, applied only to a program in the draft state.
- **REQ-AUTO.PROG.EDIT-002** An edit shall keep the step ID of every step it does not delete, and a new step shall get a new UUIDv7.
- **REQ-AUTO.PROG.EDIT-003** The command view and the summary view shall be computed from the canonical program alone, and both shall show the same set of step IDs.
- **REQ-AUTO.PROG.EDIT-004** Each step changed by an agent principal shall carry a provenance tag naming that agent until a human adopts it.

## Interfaces

- **Offers:** `program.edit_*` (W1), `program.show@1` (R0).
- **Consumes:** FMT, CHECK (diagnostics shown in the views), CORE journal.

## Decomposition hints

VIEWS (command and summary projections), OPS (edit commands), PROV (agent provenance on steps).

## Open questions

- Whether a third, Python-like text view is added is D-026 (recommendation there: not in P1b).
- Adoption of agent-authored steps is checked at release by `AUTO.REC.ADOPT`; which command a human uses to adopt steps (a generic CORE adoption command or `program.adopt_steps@1`) needs agreement with `CORE`.

## Children

_None yet: status `open`._
