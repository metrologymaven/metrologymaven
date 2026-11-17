---
id: AUTO.PY.DLG
title: Custom dialogs for scripts
level: 3
parent: AUTO.PY
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.PY-009]
covers: [GOM.AUT.08]
depends_on: [AUTO.PY.PROTO, AUTO.PROG.OPER]
cli: []
mcp: []
journal: [script.answer_dialog]
updated: 2026-11-16
---

# AUTO.PY.DLG: Custom dialogs for scripts

## Purpose

Lets a script ask the user for input through a dialog declared with the operator-interaction descriptors of `AUTO.PROG.OPER` (fields, types, units, validation). The host validates each answer against the descriptor and journals it as a command, so a script run's inputs are as reproducible as a program's operator responses.

## Scope

**In:**
- Python API to declare and show a dialog from descriptors
- Host-side validation of answers
- Journaling each submitted answer (a command such as `script.answer_dialog@1`, W2)
- Behaviour when no human is present (headless or agent-started runs)

**Out:**
- Descriptor schema (`AUTO.PROG.OPER`)
- Rendering the dialog in the GUI or kiosk (`VIZ`)

## Requirements

- **REQ-AUTO.PY.DLG-001** A script shall declare a dialog only with operator-interaction descriptors, and the host shall reject any other dialog definition.
- **REQ-AUTO.PY.DLG-002** When a dialog is submitted, the host shall validate every field against its descriptor and journal the answer as a command before returning it to the script.
- **REQ-AUTO.PY.DLG-003** If no human can answer (headless run or agent-started run), then showing a dialog shall fail with `no_operator` unless the run was given a pre-bound answer set.
- **REQ-AUTO.PY.DLG-004** Dialog text from the script shall be shown as script-authored text, and answers typed by the user shall be marked untrusted when passed to commands.

## Interfaces

- **Offers:** Python dialog API; `script.answer_dialog@1`.
- **Consumes:** `AUTO.PROG.OPER` descriptors; `AUTO.PY.PROTO` dialog messages; VIZ dialog renderer.

## Decomposition hints

API (Python declaration), VALID (validation and journaling), HEADLESS (pre-bound answers). One M leaf is likely once `AUTO.PROG.OPER` is built and `VIZ` names its dialog renderer.

## Open questions

- Should an agent-started run be allowed to show a dialog to a human (an agent asking a person a question mid-run)? Proposed no in this release (`no_operator`); the Proposal Inbox (`VIZ.INBOX`) is the channel for agent questions.

## Children

_None yet: status `open`._
