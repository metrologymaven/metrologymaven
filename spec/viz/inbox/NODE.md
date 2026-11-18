---
id: VIZ.INBOX
title: Proposal Inbox
level: 2
parent: VIZ
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ-007]
covers: []
depends_on: [CORE, VIZ.SHELL, VIZ.VIEW]
cli: [fid view list-proposals, fid view show-proposal]
mcp: [view_list_proposals, view_show_proposal]
journal: []
updated: 2026-11-16
---

# VIZ.INBOX: Proposal Inbox

## Purpose

The human review surface for agent work. Every agent branch (or tagged span of one) appears as a proposed change set with a four-layer diff against the working document: command arguments, semantic entities, kernel result changes, and a visual before/after. The human applies, edits, or rejects it; applying is branch promotion through CORE. It is separate because it is the visible enforcement point of C5 ("humans adopt agent work") and has its own safety rules about batching, staleness and untrusted text.

## Scope

**In:**
- Listing open proposals (agent branches and spans) with author client, declared model (as untrusted text), command count and side-effect classes
- Four-layer diff: command, semantic, result (kernel-computed), visual (ghosts, toggle, split view)
- Plan preview for multi-step proposals: grouped checklist, apply all, span by span, or edit
- Action chips: apply, apply and next, edit, reject, explain; each shows side-effect class and count
- Stale detection when the target head moved, and re-execution on the new head
- Rendering agent rationale as quoted untrusted data
- Agent confidence shown in words only, never as a number, and never outside the Inbox
- Governance-profile gates (step-confirm, e-signature) as UI gates on CORE promotion

**Out** (and which node owns it instead):
- Branches, promotion and dual-principal journaling (`CORE`)
- MCP tools that agents use to create work and request review (`AUTO`)
- Signing and e-signature meanings (`TRUST`)
- Visual ghost drawing (`VIZ.RENDER`, `VIZ.VIEW` highlight)

## Requirements

- **REQ-VIZ.INBOX-001** The Inbox shall list every open agent branch or tagged span as one proposal within 1 s of its creation [unverified].
- **REQ-VIZ.INBOX-002** For each proposal the Inbox shall show a command diff, a semantic diff, a result diff computed by the kernel, and a visual diff.
- **REQ-VIZ.INBOX-003** Applying a proposal shall be performed only by a human principal through CORE branch promotion, and the Inbox shall offer no path by which an agent principal applies a proposal.
- **REQ-VIZ.INBOX-004** If the target head has moved since a proposal was created, then the Inbox shall mark it stale and shall not allow apply until it is re-executed on the new head.
- **REQ-VIZ.INBOX-005** Batch apply shall require the human to confirm the total command count and shall list every side-effect class in the batch.
- **REQ-VIZ.INBOX-006** Agent-authored rationale and declared model names shall be rendered as quoted untrusted text, never as markup or links that execute.
- **REQ-VIZ.INBOX-007** Agent confidence shall be displayed as a level word with reasons, never as a number, and shall never appear in result tables or reports.
- **REQ-VIZ.INBOX-008** A rejected proposal shall remain readable in the journal history.

## Interfaces

- **Offers:** the Inbox panel; `view.list_proposals@1` (R0) and `view.show_proposal@1` (R0) returning the diff layers.
- **Consumes:** CORE journal branches, promote request and promotion, result diff; VIZ.SHELL panel host; VIZ.VIEW highlight and before/after split; TRUST signing hooks for protected targets.

## Decomposition hints

LIST, DIFF (four layers), PLANPREV, ACTIONS (chips and batch rules), STALE, GATES (governance-profile gates).

## Split rationale

The dividing principle is **the review sequence**: see the queue (LIST), understand one proposal (DIFF), check it is still valid (STALE), act on it (ACTIONS), pass the governance gates (GATES). Each step has its own safety rule: untrusted text in LIST and DIFF, staleness in STALE, human-only promotion in ACTIONS, signing and two-person rules in GATES.

Alternatives considered:
- **One child per diff layer.** Rejected at this level: the four layers share one command (`view.show_proposal`) and one panel; they are DIFF's decomposition hints instead.
- **GATES folded into ACTIONS.** Rejected: gates depend on TRUST and differ per governance profile, while ACTIONS is the same everywhere.

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** the Inbox is the UI for the H2 batched release-and-adoption queue and the H1 preview; one action per batch.
- The result-diff computation is assumed to be a CORE/kernel service; if it is not, it needs an owner.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.INBOX.LIST | Proposal list (leaf) | 001 006 | none |
| VIZ.INBOX.DIFF | Four-layer proposal diff | 002 006 007 | none |
| VIZ.INBOX.STALE | Stale proposal detection and apply gate (leaf) | 004 | none |
| VIZ.INBOX.ACTIONS | Action chips, plan preview and batch apply | 003 005 008 | none |
| VIZ.INBOX.GATES | Governance-profile gates on promotion | 003 | none |
