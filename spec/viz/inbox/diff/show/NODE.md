---
id: VIZ.INBOX.DIFF.SHOW
title: Show-proposal command and command diff layer
level: 4
parent: VIZ.INBOX.DIFF
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.DIFF-001, REQ-VIZ.INBOX.DIFF-003]
covers: []
depends_on: [VIZ.INBOX.LIST, VIZ.INBOX.STALE, VIZ.INBOX.DIFF.SEMRES, VIZ.INBOX.DIFF.VISUAL, VIZ.INBOX.DIFF.HEADER, CORE.JRN.BR.DIFF, CORE.REG.QRY]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: [fid view show-proposal]
mcp: [view_show_proposal]
journal: [view.show_proposal@1]
updated: 2026-11-17
---

# VIZ.INBOX.DIFF.SHOW: Show-proposal command and command diff layer

## Purpose

`view.show_proposal@1` returns everything about one proposal in one read: header (brief, verdict, confidence word), command layer, semantic layer, result layer, visual descriptor and stale state, so the GUI panel, the CLI and MCP agents all see the same facts. The command layer lists each committed entry on the proposal since its fork point with command ID, side-effect class, localised title and its arguments, where agent-supplied string arguments stay typed as untrusted text.

## Scope

**In:** `view.show_proposal@1` (R0); the command layer (entries in order, arguments from entry payloads, per-entry class); assembling the layers from HEADER, SEMRES and VISUAL; the diff panel layout with tabs per layer.
**Out:** the layers' content (`VIZ.INBOX.DIFF.SEMRES`, `VIZ.INBOX.DIFF.VISUAL`, `VIZ.INBOX.DIFF.HEADER`); stale predicate (`VIZ.INBOX.STALE`); actions (`VIZ.INBOX.ACTIONS`).

## Requirements

- **REQ-VIZ.INBOX.DIFF.SHOW-001** `view.show_proposal@1` shall return, for one proposal ID, the header, command layer, semantic layer, result layer, visual descriptor and stale flag.
- **REQ-VIZ.INBOX.DIFF.SHOW-002** The command layer shall list every entry of the proposal's span or branch after its fork point, in sequence order, with command ID and major, side-effect class, localised title and arguments.
- **REQ-VIZ.INBOX.DIFF.SHOW-003** Every argument whose schema marks it untrusted shall be returned as untrusted text and rendered as quoted plain text.
- **REQ-VIZ.INBOX.DIFF.SHOW-004** If an entry's payload has been removed, then the command layer shall show the entry with `payload_removed` and no arguments.
- **REQ-VIZ.INBOX.DIFF.SHOW-005** The result shall be byte-identical in canonical form for the same journal state.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-resdiff`, when `fid view show-proposal --id <p> --json` runs, then the result holds all six parts, the command layer lists the branch's 7 entries in order with classes, and the semantic and result layers equal SEMRES's output for the same heads.
- **AC-2** Given an agent string argument containing markup, then it is returned in an untrusted field and rendered literally.
- **AC-3** Given one entry with its payload removed, then that row shows `payload_removed`.
- **AC-4** Given the same state twice, then outputs are byte-identical; given an MCP call, then the tool `view_show_proposal` returns the same JSON.
- **AC-5** (error path) Given an unknown proposal ID, then the command fails with `UnknownProposal`.

## Design

- **Registry command(s):** `view.show_proposal@1`, R0; params `{ id: ProposalId }`; result `ProposalDetail { summary, stale, header, commands: [CommandRow], semantic, result, visual }`.
- **Data structures:** `CommandRow { seq, entry_hash, command, class, title, args: Option<JsonWithUntrusted>, payload_removed }`.
- **Algorithm:** resolve proposal (LIST); compute stale (STALE); read entries since fork; call SEMRES, VISUAL descriptor and HEADER; assemble.
- **Errors:** `UnknownProposal`, `JournalUnavailable`.
- **Determinism:** D0.
- **Performance budget:** < 500 ms for a proposal of 200 entries, excluding the visual scene build [unverified].
- **Crate:** `fid-inbox` (module `diff::show`).

## Test fixtures

`fixtures/viz/inbox-resdiff` (shared).

## Build prompt

> In crate `fid-inbox`, add module `diff::show` with registry command `view.show_proposal@1` (R0; CLI `fid view show-proposal`, MCP `view_show_proposal`). Return `ProposalDetail`: summary and stale flag (from the list and stale modules), header (`diff::header`), command layer (every entry after the fork point in order: command ID and major, side-effect class, localised title, arguments with untrusted string fields kept untrusted; `payload_removed` when a payload is gone), semantic and result layers (`diff::semres`) and the visual descriptor (`diff::visual`). Render the panel with one tab per layer. Tests AC-1..AC-5. Rules: R0 only, no state change; untrusted text is data (C6); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
