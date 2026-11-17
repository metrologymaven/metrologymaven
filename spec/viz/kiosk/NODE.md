---
id: VIZ.KIOSK
title: Kiosk and operator mode
level: 2
parent: VIZ
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ-008]
covers: []
depends_on: [AUTO, CORE, VIZ.SHELL, VIZ.OVERLAY]
cli: [fid view get-kiosk-status]
mcp: [view_get_kiosk_status]
journal: [view.enter_kiosk@*, view.exit_kiosk@*]
updated: 2026-11-16
---

# VIZ.KIOSK: Kiosk and operator mode

## Purpose

The locked-down front end for the shop-floor operator: choose a released Command Program, scan or type the serial, follow only the operator steps the program declares, see a large live conformance status, and get labels, reports and exports at the end of each cycle. No editing is possible. It is separate because its safety and lock-down rules (what the operator cannot do) are the opposite of the full shell's.

## Scope

**In:**
- Kiosk session lifecycle: enter and exit (admin-authorised), program choice from released programs only
- Serial and barcode entry
- Rendering of the program's declared operator steps (prompt, input, pick, confirm)
- Large live status display with non-colour cues, for glove and touch use
- End-of-cycle actions declared by the program (label print, report, export)
- Pallet and multi-part loops as declared by the program
- Error handling by the program's declared operator decision, or escalation to a supervisor

**Out** (and which node owns it instead):
- Command Program format, interpreter and operator-step semantics (`AUTO`)
- Motion approval and run tickets (`CMM`)
- Report generation (`RPT`)
- Kiosk hardware and touchscreen support (`PLAT`)

## Requirements

- **REQ-VIZ.KIOSK-001** While in kiosk mode, the system shall offer only programs whose lifecycle state is released.
- **REQ-VIZ.KIOSK-002** While in kiosk mode, the system shall present only the steps and prompts the running program declares, and no other command shall be reachable from the UI.
- **REQ-VIZ.KIOSK-003** Entering and leaving kiosk mode shall require an admin principal and shall be journaled; neither shall be available to agents.
- **REQ-VIZ.KIOSK-004** If a program step fails, then the system shall follow the operator decision the program declares for that step, or escalate to a supervisor if none is declared.
- **REQ-VIZ.KIOSK-005** The live status display shall show the conformance outcome as text and icon at a minimum glyph height of 20 mm on the reference kiosk display [unverified].
- **REQ-VIZ.KIOSK-006** All interactive targets in kiosk mode shall be at least 15 mm on the reference kiosk display, for gloved touch use [unverified].
- **REQ-VIZ.KIOSK-007** If the application restarts during a kiosk cycle, then it shall resume in kiosk mode at the last committed step of that cycle.

## Interfaces

- **Offers:** kiosk mode; `view.get_kiosk_status@1` (R0); `view.enter_kiosk@1` and `view.exit_kiosk@1` (A0, never on MCP).
- **Consumes:** AUTO program interpreter and operator steps; CORE journal and principals; VIZ.SHELL theme (shop-floor) and panel host; VIZ.OVERLAY status markers.

## Decomposition hints

SESSION (enter, exit, resume), PROGPICK, STEPS (operator-step renderers), STATUS (big status), CYCLE (end-of-cycle and loops), ESCAL (errors and supervisor escalation).

## Split rationale

The dividing principle is **the operator's cycle in time order**: the admin sets the station up (SESSION), the operator picks a program and serial (PROGPICK), works through declared steps (STEPS) while watching the big status (STATUS), and the cycle ends or escalates (CYCLE). Each child carries one lock-down rule: admin-only entry, released-only programs, declared-steps-only, never-colour-only status, declared-decisions-only.

Alternatives considered:
- **Split by permission** (admin, operator, supervisor surfaces). Rejected: the supervisor appears only in escalation, which would leave a one-requirement child.
- **STATUS folded into STEPS.** Rejected: the status display is a small finishable leaf reused across steps and the summary.

## Open questions

- Phase: the concept places scan-program kiosk in P1b and CNC kiosk in P2; the parent is P1a. A phase change would need a decision; this child is tagged P1b as the earliest it can be useful [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.KIOSK.SESSION | Kiosk session enter, exit and resume (leaf) | 003 007 | none |
| VIZ.KIOSK.PROGPICK | Released-program chooser and serial entry (leaf) | 001 | none |
| VIZ.KIOSK.STEPS | Operator-step renderers and lock-down | 002 006 | none |
| VIZ.KIOSK.STATUS | Large live status display (leaf) | 005 | none |
| VIZ.KIOSK.CYCLE | End-of-cycle actions, loops and escalation | 004 | none |
