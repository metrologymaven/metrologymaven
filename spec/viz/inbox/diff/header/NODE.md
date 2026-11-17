---
id: VIZ.INBOX.DIFF.HEADER
title: Proposal header with brief, verdict and confidence words
level: 4
parent: VIZ.INBOX.DIFF
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.DIFF-003, REQ-VIZ.INBOX.DIFF-004]
covers: []
depends_on: [AGENT.PROPOSE.SHOW, VIZ.SHELL.DOCK]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.DIFF.HEADER: Proposal header with brief, verdict and confidence words

## Purpose

Renders the top of a proposal: the agent's brief (goal, rationale steps, assumptions, confidence word and its reasons, client and model) from `agent.show_brief@1`, the system verdict (`complete`, `brief_missing`, `stale`, `fields_missing`) and the claim mismatches. Every agent-authored string is shown as quoted plain text. Confidence is only ever the word `low`, `medium` or `high` with reasons, and this header is the only place in the product that shows it.

## Scope

**In:** `ProposalHeader` model built from `agent.show_brief@1`; plain-text rendering of untrusted fields (no Markdown, no links, no images, control characters shown as escapes); verdict and mismatch badges; the "Explain" disclosure that expands the full rationale.
**Out:** the brief schema and verdict rules (`AGENT.PROPOSE.BRIEF`, `AGENT.PROPOSE.SHOW`); result values (`VIZ.INBOX.DIFF.SEMRES`); other surfaces that must never show confidence (reports: `RPT`; result tables: `VIZ.SHELL.TREE`).

## Requirements

- **REQ-VIZ.INBOX.DIFF.HEADER-001** Every agent-authored field shall be rendered as quoted plain text: markup, URLs and Markdown shall appear as literal characters and no element shall be clickable or fetch anything.
- **REQ-VIZ.INBOX.DIFF.HEADER-002** Confidence shall be rendered only as one of the words `low`, `medium` or `high` followed by its reasons, and the header model shall contain no numeric confidence field.
- **REQ-VIZ.INBOX.DIFF.HEADER-003** The header shall show the verdict of `agent.show_brief@1` and every claim mismatch, with system-computed text visibly distinct from agent text.
- **REQ-VIZ.INBOX.DIFF.HEADER-004** If the brief is missing or stale, then the header shall say so in system text above any agent text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-brief-hostile.json` (a brief whose goal holds `<a href=...>`, a Markdown link and a zero-width joiner sequence), when rendered, then a UI snapshot shows the literal characters and the accessibility tree contains no link role.
- **AC-2** Given a brief with confidence `high`, then the header shows "high" and its reasons, and a schema check of `ProposalHeader` finds no numeric confidence field.
- **AC-3** Given a stale brief, then the stale notice is the first line, in system style.
- **AC-4** Given two mismatches, then both appear with their kinds.

## Design

- **Registry command(s):** none; data comes from `agent.show_brief@1` (R0).
- **Data structures:** `ProposalHeader { verdict, mismatches, brief: Option<BriefView { goal, rationale, assumptions, confidence: Low|Medium|High, confidence_reasons, client, model }> }`, all strings `Untrusted<String>`.
- **Algorithm:** map fields; escape control and bidi characters to visible escapes; render with the toolkit's plain-text label only.
- **Errors:** passed through from `agent.show_brief@1`.
- **Determinism:** D0.
- **Performance budget:** < 5 ms per header [unverified].
- **Crate:** `fid-inbox` (module `diff::header`).

## Test fixtures

`fixtures/viz/inbox-brief-hostile.json`, hand-written; CC-BY-4.0.

## Build prompt

> In crate `fid-inbox`, add module `diff::header`: build `ProposalHeader` from `agent.show_brief@1` (brief, verdict, mismatches) and render it in the Inbox panel. Agent strings are `Untrusted<String>` shown as quoted plain text only (no markup, links, images; control and bidi characters shown as visible escapes). Confidence appears only as `low|medium|high` plus reasons; no numeric field exists. Missing or stale briefs are announced first in system text. Tests AC-1..AC-4. Rules: untrusted text is data (C6); confidence never leaves the Inbox; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
