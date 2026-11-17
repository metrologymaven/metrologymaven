---
id: VIZ.INBOX.ACTIONS
title: Action chips, plan preview and batch apply
level: 3
parent: VIZ.INBOX
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX-003, REQ-VIZ.INBOX-005, REQ-VIZ.INBOX-008]
covers: []
depends_on: [VIZ.INBOX.DIFF, VIZ.INBOX.STALE, CORE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.INBOX.ACTIONS: Action chips, plan preview and batch apply

## Purpose

The human's controls on a proposal: apply, apply and next, edit, reject, explain; each chip showing side-effect class icons and command count. Plan preview for multi-step proposals (a grouped checklist applied all at once, span by span, or edited). Batch apply that requires confirming the total count and lists every class. Applying is always CORE branch promotion by a human principal; rejecting closes the branch and keeps it in history.

## Scope

**In:**
- Action chips and their enabled state (from the apply gate and principal)
- Plan preview grouped by span
- Batch apply with count confirmation and class listing
- Edit (arguments changed on the branch, then re-evaluated)
- Reject (branch closed, kept in the journal)
- Explain (request for the agent's explanation, shown as untrusted text)

**Out** (and which node owns it instead):
- Promotion and branch closing (`CORE`)
- Governance gates such as e-signature (`VIZ.INBOX.GATES`)
- Staleness (`VIZ.INBOX.STALE`)

## Requirements

- **REQ-VIZ.INBOX.ACTIONS-001** Apply shall invoke CORE branch promotion as the human principal operating the GUI, and the module shall expose no command or tool by which an agent principal applies a proposal.
- **REQ-VIZ.INBOX.ACTIONS-002** Each action chip shall show the side-effect classes and command count it would affect.
- **REQ-VIZ.INBOX.ACTIONS-003** Batch apply shall require the human to type or confirm the total command count and shall list every side-effect class in the batch.
- **REQ-VIZ.INBOX.ACTIONS-004** Reject shall close the branch through CORE so that it remains readable in journal history.
- **REQ-VIZ.INBOX.ACTIONS-005** While the apply gate is disabled, apply and batch apply shall be disabled with the gate's reason shown.

## Interfaces

- **Offers:** action chips and plan preview within the Inbox panel.
- **Consumes:** CORE promotion, branch close and edit; `VIZ.INBOX.STALE` apply gate; `VIZ.INBOX.GATES` gate hooks (through CORE promotion).

## Decomposition hints

CHIPS, PLANPREV, BATCH, EDIT, REJECT.

## Open questions

- None beyond the CORE promotion API.

## Children

_None yet: status `open`._
