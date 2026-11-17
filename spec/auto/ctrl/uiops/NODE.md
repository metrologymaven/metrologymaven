---
id: AUTO.CTRL.UIOPS
title: UI inspection, input driving and screenshots
level: 3
parent: AUTO.CTRL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.CTRL-005, REQ-AUTO.CTRL-009]
covers: []
depends_on: [AUTO.CTRL.EXEC]
cli: []
mcp: []
journal: [capture.take_screenshot]
updated: 2026-11-16
---

# AUTO.CTRL.UIOPS: UI inspection, input driving and screenshots

## Purpose

The UI-level operations of the channel: read the UI tree (panels, widgets, labels, enabled state, bounds), drive pointer and keyboard input, and take screenshots. Screenshots are the VIZ capture registry command (`capture.take_screenshot@1`), so they are journaled like any other; input driving only causes what the same clicks by a person would cause, so every state change still goes through registry commands.

## Scope

**In:**
- Ops `ui.inspect`, `ui.input` (pointer, keyboard, text), `ui.screenshot`
- Stable widget identifiers for inspection and targeting
- Mapping of `ui.screenshot` to `capture.take_screenshot@1` and returning the image within the 16 MiB bound
- Input rate limits and focus rules

**Out:**
- UI-side hooks for the tree, input injection and capture (`VIZ.SHELL`, `VIZ.CAPTURE`)
- Action traces (`AUTO.CTRL.TRACE`)

## Requirements

- **REQ-AUTO.CTRL.UIOPS-001** `ui.inspect` shall return the UI tree with a stable widget ID, role, label, enabled flag and bounds for every widget, without changing UI or document state.
- **REQ-AUTO.CTRL.UIOPS-002** `ui.screenshot` shall be executed as `capture.take_screenshot@1` through the bus, and the reply shall carry the journal entry hash.
- **REQ-AUTO.CTRL.UIOPS-003** `ui.input` shall inject input only into the app's own windows, and any state change it causes shall happen only through the registry commands the UI invokes in response.
- **REQ-AUTO.CTRL.UIOPS-004** Text typed through `ui.input` into a field shall be marked untrusted when it reaches a command (C6).

## Interfaces

- **Offers:** the three UI ops.
- **Consumes:** VIZ UI tree, input injection and capture command.

## Decomposition hints

INSPECT (tree model and widget IDs), INPUT (injection and focus), SHOT (capture mapping). Waits on `VIZ.SHELL` for the widget-tree hook.

## Open questions

- Stable widget IDs under egui's immediate mode (D-005) need a VIZ convention; open until `VIZ.SHELL` defines it.

## Children

_None yet: status `open`._
