---
id: VIZ.KIOSK.CYCLE
title: End-of-cycle actions, loops and escalation
level: 3
parent: VIZ.KIOSK
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK-004]
covers: []
depends_on: [VIZ.KIOSK.SESSION, VIZ.KIOSK.STATUS, AUTO.RUN.PLAY, AUTO.RUN.BIND, CORE.BUS.PRIN]
cli: [fid view resolve-kiosk-escalation]
mcp: [view_resolve_kiosk_escalation]
journal: [view.resolve_kiosk_escalation@*]
updated: 2026-11-17
---

# VIZ.KIOSK.CYCLE: End-of-cycle actions, loops and escalation

## Purpose

What happens around the steps: end-of-cycle actions the program declares (label print, report, export), pallet and multi-part loops, and error handling by the program's declared operator decision for a failed step, or escalation to a supervisor when none is declared.

## Scope

**In:**
- Running declared end-of-cycle actions and showing their outcome
- Pallet and multi-part loop navigation (next part, skip, repeat as declared)
- Failed-step handling: declared operator decisions (retry, skip, abort) or supervisor escalation with supervisor sign-in
- Cycle summary screen

**Out** (and which node owns it instead):
- Report generation and export (`RPT`, `IO`)
- Interpreter semantics of loops and decisions (`AUTO.RUN`)
- Machine motion approval (`CMM`)

## Requirements

- **REQ-VIZ.KIOSK.CYCLE-001** If a program step fails, then the kiosk shall offer only the operator decisions the program declares for that step.
- **REQ-VIZ.KIOSK.CYCLE-002** If a failed step declares no operator decision, then the kiosk shall block the cycle and require a supervisor principal to resolve it.
- **REQ-VIZ.KIOSK.CYCLE-003** End-of-cycle actions shall run only as declared by the program and their outcome shall be shown and journaled.
- **REQ-VIZ.KIOSK.CYCLE-004** Loops shall follow only the iteration rules the program declares.

## Interfaces

- **Offers:** cycle summary and escalation screens.
- **Consumes:** AUTO.RUN cycle state and decisions; RPT generation and export commands; CORE principals.

## Decomposition hints

ENDACT, LOOP, ERRDEC (declared decisions), ESCAL (supervisor escalation), SUMMARY.

## Open questions

- RPT generation and export are consumed as program steps run by the interpreter, not as a `depends_on` (RPT depends on VIZ, so the reverse edge would form a cycle). End-of-cycle actions are therefore declared program steps (`VIZ.KIOSK.CYCLE.SUMMARY`).
- AUTO's program format has no per-step operator decisions on failure yet (`on_error` is noted in `AUTO.PROG.INTERP.CALL` as a possible additive change); until it exists, every failure escalates to a supervisor (`VIZ.KIOSK.CYCLE.ESCAL`) [cross-domain].
- Supervisors are listed in the admin setting `kiosk.supervisors` and confirmed by OS user presence, since CORE.BUS defines no roles.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.KIOSK.CYCLE.ESCAL | Failed-step decisions and supervisor escalation (leaf) | 001 002 | none |
| VIZ.KIOSK.CYCLE.SUMMARY | Cycle summary, loop progress and end-of-cycle outcomes (leaf) | 003 004 | none |
