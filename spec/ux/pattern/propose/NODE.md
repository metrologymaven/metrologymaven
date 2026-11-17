---
id: UX.PATTERN.PROPOSE
title: Proposal review and agent provenance pattern
level: 3
parent: UX.PATTERN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.PATTERN-001, REQ-UX.PATTERN-004, REQ-UX.PATTERN-005, REQ-UX.PATTERN-006]
covers: []
depends_on: [UX.PATTERN.SCHEMA, UX.COMP.CMD, UX.ICONS.CLASS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.PATTERN.PROPOSE: Proposal review and agent provenance pattern

## Purpose

How a person reviews work an agent proposes: the diff display, approval chips with class icon and command count, batch confirmation that requires confirming the count, rejection with a reason, and the provenance cue that marks agent-authored content wherever it appears. Agents never adopt their own proposals (CONSTITUTION C5), and the pattern offers no way to approve a W3, M1, S1 or A0 action from a proposal.

## Scope

**In:** `patterns/propose.md` (`pat.propose`), `patterns/provenance.md` (`pat.agent-cue`).
**Out:** proposal content and agent behaviour (AGENT); the Proposal Inbox implementation (VIZ.INBOX); approval-chip component (`UX.COMP.CMD`).

## Requirements

- **REQ-UX.PATTERN.PROPOSE-001** Every approval control shall show the side-effect class icon of the highest class in the proposal and the command count as text.
- **REQ-UX.PATTERN.PROPOSE-002** Batch approval shall require the person to confirm the command count, by typing it or selecting it, before the approval commits.
- **REQ-UX.PATTERN.PROPOSE-003** If a proposal contains a W3, M1, S1 or A0 command, then the pattern shall show no approve control for it and shall show text naming the separate human route.
- **REQ-UX.PATTERN.PROPOSE-004** The diff display shall show added, changed and removed entities with icon and text cues besides colour, and shall link each change to its command.
- **REQ-UX.PATTERN.PROPOSE-005** Agent-authored content shall carry the `pat.agent-cue` (icon plus the text "agent-authored" or its translation) in every view where it appears, including tree, table, viewport labels, reports in draft and the Approval Station, until a human adopts it.
- **REQ-UX.PATTERN.PROPOSE-006** Both specs shall pass `fid-design pattern-check`.

## Acceptance criteria

- **AC-1** Given the two specs, when `fid-design pattern-check` runs, then it exits 0.
- **AC-2** Given `pat.propose`, then the count confirmation, the absence of approve controls for W3, M1, S1 and A0, and the diff cues are present (design-review checklist items).
- **AC-3** Given `pat.agent-cue`, then the list of views of REQ-005 is complete (review item, including the CMM.EXEC.STATION accessibility note).

## Design

- **Deliverable:** two Markdown specs in `pattern-spec@1`.
- **Determinism:** D0.
- **Crate:** none (package content).

## Test fixtures

The shipped specs; CC-BY-4.0.

## Build prompt

> Write `patterns/propose.md` and `patterns/provenance.md` in `pattern-spec@1`: approval controls with the highest class icon and the command count as text; batch approval needs the count confirmed; no approve control for W3, M1, S1 or A0 commands, with text naming the human route; diff with added, changed, removed cues by icon and text, each linked to its command; reject with reason; the agent-authored cue (icon plus text) in tree, table, viewport labels, draft reports and the Approval Station until adopted. Original wording. Run `fid-design pattern-check` until it exits 0, then open a design-review record.

## Open questions

- CMM.EXEC.STATION asks for a UX review of the agent-authored flag's accessibility; REQ-UX.PATTERN.PROPOSE-005 covers it.

## Children

_Leaf: none._
