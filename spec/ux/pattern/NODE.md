---
id: UX.PATTERN
title: Shared interaction and guidance patterns
level: 2
parent: UX
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UX-001]
covers: []
depends_on: [UX.COMP, FLOW]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UX.PATTERN: Shared interaction and guidance patterns

## Purpose

The behaviours that span components and apps, specified once so every app behaves the same: selection, picking and lasso, undo, the command palette flow, proposal review, operator prompts, confirmation of controlled actions, errors with suggested fixes, progress and cancel, and in-app onboarding and guidance. It is separate from components because patterns are sequences across several components and are tied to FLOW's journeys and to the command bus rules (CONSTITUTION C3 and C5), not to one widget.

## Scope

**In:**
- Selection model across viewport, tree and table; pick and lasso gestures; selection as shared context for palette and agent
- Undo and redo presentation: undo groups labelled with command titles
- Command palette flow: search, argument entry, dry-run preview, execution feedback
- Proposal review: diff display, approval chips, batch confirmation, provenance cues for agent-authored work
- Operator prompts for kiosk and Approval Station steps
- Confirmation of controlled actions (W3 release, S1 signing hand-off, irreversible actions)
- Error, unresolved and not-decidable states with suggested fixes (REQ-FLOW-006)
- Progress, background jobs and cancel
- Onboarding and guidance: first-run flow around Shop Setup, empty states, contextual help, tours

**Out** (and which node owns it instead):
- Individual component specs (`UX.COMP`)
- Implementing picking and CPU resolution (`VIZ.PICK`), the Proposal Inbox (`VIZ.INBOX`) and kiosk mode (`VIZ.KIOSK`)
- The Shop Setup steps and the suggested-fix catalogue (`FLOW`)
- Tutorials and training content outside the app (`OUTR.CONTENT`)
- Motion authorization rules (D-008, `CMM.EXEC`); this node only designs how the UI presents them

## Requirements

- **REQ-UX.PATTERN-001** Each pattern spec shall define its trigger, steps, states, the command patterns it invokes, its keyboard path, and its cancel and failure paths.
- **REQ-UX.PATTERN-002** The selection pattern shall define one selection shared by viewport, tree and table, and a selection change in any of them shall be shown in all linked views.
- **REQ-UX.PATTERN-003** The undo pattern shall present each undo step as one undo group labelled with its command title, and an applied proposal shall undo as one group.
- **REQ-UX.PATTERN-004** The proposal-review pattern shall show the side-effect class icon and the command count on every approval control, and batch approval shall require confirming the count.
- **REQ-UX.PATTERN-005** The proposal-review pattern shall offer no control that approves a W3, M1, S1 or A0 action from an agent proposal.
- **REQ-UX.PATTERN-006** Agent-authored content shall carry a provenance cue that uses an icon and text, not colour alone, wherever it is shown.
- **REQ-UX.PATTERN-007** When a workflow reaches an error, unresolved or not-decidable state, the error pattern shall show the cause in plain words and at least one suggested fix that names a command.
- **REQ-UX.PATTERN-008** The confirmation pattern for controlled and irreversible actions shall name the action and its consequence in the control labels, and shall not use generic "OK" and "Cancel" labels.
- **REQ-UX.PATTERN-009** The operator-prompt pattern shall show one instruction per step in the shop-floor theme with body text at least 18 logical pixels high [unverified], and shall state which input the step expects.
- **REQ-UX.PATTERN-010** Onboarding and guidance shall be dismissible, recoverable from help, and shall never cover a control that the current step requires.
- **REQ-UX.PATTERN-011** The progress pattern shall show a cancel control for every cancellable job and shall state whether cancelling keeps or discards partial results.

## Interfaces

- **Offers:** pattern specs in the design-system package.
- **Consumes:** `UX.COMP`; `FLOW` (journeys, Shop Setup steps, suggested-fix rule). Interface notes (no `depends_on`): `VIZ.PICK`, `VIZ.INBOX`, `VIZ.KIOSK`, `VIZ.SHELL` and `CMM.EXEC.STATION` implement these patterns; `AGENT` defines the proposal content that the proposal-review pattern displays; CORE supplies undo groups and side-effect classes.

## Decomposition hints

SELECT (selection, pick and lasso), UNDO (undo and history presentation), PROPOSE (proposal review and agent provenance), OPERATE (operator prompts, controlled-action confirmation), ERROR (errors, suggested fixes, progress and cancel), GUIDE (onboarding and guidance).

## Open questions

- `CMM.EXEC.STATION` notes that accessibility of the agent-authored flag needs a UX review; REQ-UX.PATTERN-006 is intended to cover it.

## Children

_None yet: status `open`._
