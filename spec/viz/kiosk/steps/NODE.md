---
id: VIZ.KIOSK.STEPS
title: Operator-step renderers and lock-down
level: 3
parent: VIZ.KIOSK
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.KIOSK-002, REQ-VIZ.KIOSK-006]
covers: []
depends_on: [VIZ.KIOSK.SESSION, VIZ.PICK.RESOLVE, AUTO.PROG.OPER, AUTO.RUN.PLAY]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.KIOSK.STEPS: Operator-step renderers and lock-down

## Purpose

Render the operator steps a running program declares (prompt, input, pick, confirm) as large, glove-friendly screens, and guarantee that nothing else is reachable: the only commands the kiosk UI can issue are the step responses the current step accepts.

## Scope

**In:**
- Renderers for prompt, input (typed value with unit and limits), pick (on a simplified 3D view or image) and confirm steps
- Step-scoped command allow-list derived from the program
- Large-target layout in the shop-floor theme
- Untrusted program text rendered as plain text

**Out** (and which node owns it instead):
- Step semantics and the interpreter (`AUTO.PROG`, `AUTO.RUN`)
- Status display (`VIZ.KIOSK.STATUS`)
- Error handling and escalation (`VIZ.KIOSK.CYCLE`)

## Requirements

- **REQ-VIZ.KIOSK.STEPS-001** While a step is active, the kiosk UI shall offer only the responses that step declares, and any other command attempted from the UI shall be refused.
- **REQ-VIZ.KIOSK.STEPS-002** All interactive targets in kiosk steps shall be at least 15 mm on the reference kiosk display [unverified].
- **REQ-VIZ.KIOSK.STEPS-003** Input steps shall enforce the unit and limits the program declares and shall pass values as exact decimal strings with their unit.
- **REQ-VIZ.KIOSK.STEPS-004** Prompt texts and images from the program shall be rendered as untrusted content, text as plain text.
- **REQ-VIZ.KIOSK.STEPS-005** Each step response shall be committed as a command before the next step is shown.

## Interfaces

- **Offers:** step renderers within the kiosk surface.
- **Consumes:** AUTO.PROG operator-step definitions; AUTO.RUN step state and response commands; shell shop-floor theme.

## Decomposition hints

PROMPT, INPUT, PICK, CONFIRM, ALLOW (step-scoped allow-list).

## Open questions

- Resolved: pick steps reuse `VIZ.PICK.RESOLVE` directly (`VIZ.KIOSK.STEPS.PICK`).
- The UI emit filter in `VIZ.KIOSK.STEPS.LOCK` mirrors the engine-side operator session allow-list of `AUTO.RUN.PLAY.SESSION`; that list currently omits `view.get_kiosk_status@1` and the kiosk escalation command [cross-domain].
- Split: the host and its allow-list (LOCK), the descriptor field renderers (FORM) and the 3D element pick (PICK) have different inputs and tests.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.KIOSK.STEPS.LOCK | Kiosk step host, response allow-list and step sequencing (leaf) | 001 002 005 | none |
| VIZ.KIOSK.STEPS.FORM | Operator prompt and field renderers (leaf) | 002 003 004 | none |
| VIZ.KIOSK.STEPS.PICK | Element pick step on a simplified 3D view (leaf) | 002 004 | none |
