---
id: VIZ.INBOX.DIFF
title: Four-layer proposal diff
level: 3
parent: VIZ.INBOX
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX-002, REQ-VIZ.INBOX-006, REQ-VIZ.INBOX-007]
covers: []
depends_on: [VIZ.INBOX.LIST, VIZ.INBOX.STALE, CORE.JRN.BR, AGENT.PROPOSE, GDT.CHAR.RECORD, VIZ.VIEW]
cli: [fid view show-proposal]
mcp: [view_show_proposal]
journal: [view.show_proposal@*]
updated: 2026-11-17
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

- Resolved: the result diff is `AGENT.PROPOSE.IMPACT` (R1) over `CORE.JRN.BR.DIFF`, with values read from stored ResultRecords and formatted by the kernel formatter (`VIZ.INBOX.DIFF.SEMRES`). The brief, verdict and confidence word come from `AGENT.PROPOSE.SHOW` (`VIZ.INBOX.DIFF.HEADER`).
- Split rationale: one child per layer family plus the assembling command; the split view needs `VIZ.VIEW.MULTI` (P1b), so P1a may ship the toggle only.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.INBOX.DIFF.SHOW | Show-proposal command and command diff layer (leaf) | 001 003 | none |
| VIZ.INBOX.DIFF.SEMRES | Semantic and result diff layers from kernel records (leaf) | 002 | none |
| VIZ.INBOX.DIFF.VISUAL | Visual proposal diff with before/after toggle and split (leaf) | 005 | none |
| VIZ.INBOX.DIFF.HEADER | Proposal header with brief, verdict and confidence words (leaf) | 003 004 | none |
