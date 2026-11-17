---
id: VIZ.INBOX.LIST
title: Proposal list
level: 3
parent: VIZ.INBOX
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX-001, REQ-VIZ.INBOX-006]
covers: []
depends_on: [CORE.JRN, VIZ.SHELL.DOCK]
side_effect: R0
determinism: D0
cli: [fid view list-proposals]
mcp: [view_list_proposals]
journal: [view.list_proposals@1]
updated: 2026-11-16
---

# VIZ.INBOX.LIST: Proposal list

## Purpose

List every open agent branch or tagged span as one proposal, with author client, declared model (as untrusted text), command count, side-effect classes, base head and age, both as the Inbox panel's list and as an R0 command, so humans and tools see the same queue.

## Scope

**In:** `view.list_proposals@1`, the list panel, live update on branch creation, rendering of untrusted author fields.
**Out:** diff layers (`VIZ.INBOX.DIFF`); actions (`VIZ.INBOX.ACTIONS`); stale state (`VIZ.INBOX.STALE`); branch storage (`CORE.JRN`).

## Requirements

- **REQ-VIZ.INBOX.LIST-001** `view.list_proposals@1` shall return every open agent branch and tagged span as one proposal with ID, branch, span, author client ID, declared model, command count, set of side-effect classes, base head and creation time.
- **REQ-VIZ.INBOX.LIST-002** When an agent branch or span is created, the Inbox list shall show it within 1 s [unverified].
- **REQ-VIZ.INBOX.LIST-003** Declared model names and other agent-supplied strings shall be returned in typed untrusted-text fields and rendered as quoted plain text, never as markup or active links.
- **REQ-VIZ.INBOX.LIST-004** The list shall be ordered by creation time, then proposal ID, so that identical states give identical output.
- **REQ-VIZ.INBOX.LIST-005** Closed (applied or rejected) proposals shall not appear in the open list but shall remain retrievable with a `--include-closed` flag.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-3branches` (journal with three agent branches, one with two tagged spans), when `fid view list-proposals --json` runs, then it returns four proposals in creation order with correct command counts and class sets.
- **AC-2** Given a declared model string containing `<a href=...>` and Markdown, then the result carries it unchanged in an untrusted-text field and the panel renders it as literal text (UI snapshot test).
- **AC-3** Given a new branch committed while the panel is open, then the panel shows it within 1 s in the test harness.
- **AC-4** Given one proposal rejected in the fixture, then it is absent by default and present with `--include-closed`.
- **AC-5** Given the same journal state, then two runs return byte-identical canonical JSON.

## Design

- **Registry command(s):** `view.list_proposals@1`, R0. Parameters: `include_closed: bool` (default false). Result: `[ProposalSummary { id, branch, span?, author: { client_id, declared_model: UntrustedText }, command_count, classes: [SideEffect], base_head, created_at, state }]`.
- **Algorithm:** query CORE journal branches and spans; compute class sets from committed commands; sort; subscribe to branch-created events for the panel.
- **Errors:** `JournalUnavailable`.
- **Determinism:** D0.
- **Performance budget:** list of 500 proposals under 50 ms [unverified].
- **Crate:** `fid-inbox`.

## Test fixtures

`fixtures/viz/inbox-3branches`: generated journal with scripted agent commands; in-repo.

## Build prompt

> Create crate `fid-inbox` with registry command `view.list_proposals@1` (R0; CLI `fid view list-proposals`, MCP `view_list_proposals`) and the Inbox list panel hosted through the shell's panel host. Read agent branches and tagged spans from the journal; return id, branch, span, author client ID, declared model (typed untrusted text), command count, side-effect class set, base head, creation time and state, sorted by creation time then ID; `include_closed` adds applied and rejected ones. Render all agent-supplied strings as literal quoted text. Update the panel within 1 s of a new branch. Tests: fixture with three branches and two spans; untrusted markup shown literally; live update; closed filter; byte-identical repeated output. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; untrusted text is data (C6); agents cannot apply anything from this module (C5).

## Open questions

- None blocking.

## Children

_Leaf: none._
