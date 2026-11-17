---
id: VIZ.INBOX.DIFF
title: Four-layer proposal diff
level: 3
parent: VIZ.INBOX
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX-002, REQ-VIZ.INBOX-006, REQ-VIZ.INBOX-007]
covers: []
depends_on: [VIZ.INBOX.LIST, CORE, VIZ.VIEW]
cli: [fid view show-proposal]
mcp: [view_show_proposal]
journal: [view.show_proposal@*]
updated: 2026-11-16
---

# VIZ.INBOX.DIFF: Four-layer proposal diff

## Purpose

Show what a proposal would change, in four layers: the command diff (arguments of each committed command), the semantic diff (entities added, changed, removed), the result diff (status and value changes with uncertainty, computed by the kernel) and the visual diff (ghosts, before/after toggle or split). It also renders the proposal header: rationale as quoted untrusted text and agent confidence as a word with reasons, never as a number.

## Scope

**In:**
- `view.show_proposal@1` (R0) returning all four layers and the header
- Command and semantic diff presentation
- Result diff presentation from CORE's kernel-computed result diff
- Visual diff via VIZ.VIEW highlight, toggle and split
- Rationale and confidence rendering rules

**Out** (and which node owns it instead):
- Computing the result diff (`CORE`, kernel)
- Ghost drawing (`VIZ.RENDER`), split views (`VIZ.VIEW.MULTI`)
- Applying or rejecting (`VIZ.INBOX.ACTIONS`)

## Requirements

- **REQ-VIZ.INBOX.DIFF-001** `view.show_proposal@1` shall return the command diff, semantic diff, kernel-computed result diff and a visual-diff descriptor for one proposal.
- **REQ-VIZ.INBOX.DIFF-002** Every value in the result diff shall come from the kernel result diff and be formatted by the kernel formatter, with decision changes highlighted.
- **REQ-VIZ.INBOX.DIFF-003** Agent rationale and standards references supplied by the agent shall be rendered as quoted untrusted text, never as markup or active links.
- **REQ-VIZ.INBOX.DIFF-004** Agent confidence shall be shown only as a level word (low, medium, high) with reasons, never as a number, and only inside the Inbox.
- **REQ-VIZ.INBOX.DIFF-005** The visual diff shall offer a before/after toggle and shall label proposed geometry as "proposal".

## Interfaces

- **Offers:** `view.show_proposal@1`; diff panel.
- **Consumes:** CORE branch contents and result diff; `VIZ.VIEW` highlight and split; `VIZ.INBOX.LIST` summaries.

## Decomposition hints

CMDDIFF, SEMDIFF, RESDIFF, VISDIFF, HEADER (rationale and confidence).

## Open questions

- The result diff is assumed to be a CORE/kernel service (parent open question); if CORE does not own it, an owner is needed before RESDIFF can be finished.

## Children

_None yet: status `open`._
