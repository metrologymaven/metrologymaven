---
id: UX.PATTERN
title: Shared interaction and guidance patterns
level: 2
parent: UX
kind: subsystem
status: split
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
updated: 2026-11-17
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

## Split rationale

The dividing principle is **one user situation per child, plus a shared format**: SCHEMA (the `pattern-spec@1` format and check enforcing REQ-UX.PATTERN-001), SELECT (selection, pick and lasso), CMDFLOW (palette flow and undo, the two halves of running a command), PROPOSE (proposal review and agent provenance), OPERATE (operator prompts and controlled-action confirmation), ERROR (dead ends, progress and cancel), GUIDE (onboarding and guidance). This follows the decomposition hints; the palette flow joins undo because both present the command bus's commit unit.

Alternatives considered:
- **Confirmation inside PROPOSE.** Rejected: confirmation of controlled actions applies to human-initiated commands too, and shares the floor-theme constraints of operator prompts.
- **No SCHEMA child (reuse the component check).** Rejected: patterns need journey and command links that component specs do not have.

FLOW is read as artifacts (journey maps, fix catalogue, setup journey) with `uses_artifacts`, not code edges, so UX stays upstream of VIZ even though FLOW depends on VIZ.

## Open questions

- CMM.EXEC.STATION's request for a UX review of the agent-authored flag is covered by UX.PATTERN.PROPOSE (REQ-005, AC-3).
- The parent's `depends_on: [UX.COMP, FLOW]` stays as written; children use `uses_artifacts` on specific FLOW nodes, which avoids the cycle risk noted in UX (FLOW depends on VIZ and RPT).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [UX.PATTERN.SCHEMA](schema/NODE.md) | Pattern-spec format and completeness check (leaf) | 001 | none |
| [UX.PATTERN.SELECT](select/NODE.md) | Selection, pick and lasso pattern (leaf) | 001 002 | none |
| [UX.PATTERN.CMDFLOW](cmdflow/NODE.md) | Command palette flow and undo presentation (leaf) | 001 003 | none |
| [UX.PATTERN.PROPOSE](propose/NODE.md) | Proposal review and agent provenance pattern (leaf) | 001 004 005 006 | none |
| [UX.PATTERN.OPERATE](operate/NODE.md) | Operator prompts and controlled-action confirmation (leaf) | 001 008 009 | none |
| [UX.PATTERN.ERROR](error/NODE.md) | Error, unresolved and not-decidable states; progress and cancel (leaf) | 001 007 011 | none |
| [UX.PATTERN.GUIDE](guide/NODE.md) | Onboarding and in-app guidance pattern (leaf) | 001 010 | none |
