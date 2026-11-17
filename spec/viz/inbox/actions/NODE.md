---
id: VIZ.INBOX.ACTIONS
title: Action chips, plan preview and batch apply
level: 3
parent: VIZ.INBOX
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX-003, REQ-VIZ.INBOX-005, REQ-VIZ.INBOX-008]
covers: []
depends_on: [VIZ.INBOX.DIFF, VIZ.INBOX.STALE, VIZ.INBOX.GATES, CORE.JRN.BR, CORE.BUS.POL.PROJ]
cli: [fid view reject-proposal]
mcp: [view_reject_proposal]
journal: [view.reject_proposal@*]
updated: 2026-11-17
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

- CORE has no branch-close command, so rejection is the human-only W2 command `view.reject_proposal@1` owned by `VIZ.INBOX.ACTIONS.REJECT`; `VIZ.INBOX.LIST` must read its closed state from it.
- Pending requests filed by `CORE.BUS.POL.PROJ` (for example agent requests to start a recording or send an egress) are reviewed in `VIZ.INBOX.ACTIONS.REQS`.
- Split: single-proposal controls (CHIPS), multi-span batches (BATCH), rejection (REJECT, the only owned command) and pending requests (REQS). The gate dependency now points one way: ACTIONS uses `VIZ.INBOX.GATES.PLAN`; GATES no longer depends on ACTIONS.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.INBOX.ACTIONS.CHIPS | Action chips, apply through promotion, edit and explain (leaf) | 001 002 005 | none |
| VIZ.INBOX.ACTIONS.BATCH | Plan preview and batch apply by span (leaf) | 001 003 005 | none |
| VIZ.INBOX.ACTIONS.REJECT | Reject a proposal and keep it in history (leaf) | 004 | none |
| VIZ.INBOX.ACTIONS.REQS | Pending request review, approval and rejection (leaf) | 001 002 | none |
