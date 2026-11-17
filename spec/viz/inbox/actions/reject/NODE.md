---
id: VIZ.INBOX.ACTIONS.REJECT
title: Reject a proposal and keep it in history
level: 4
parent: VIZ.INBOX.ACTIONS
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.ACTIONS-004]
covers: []
depends_on: [VIZ.INBOX.LIST, CORE.REG.DEF, CORE.JRN.BR.NAME]
uses_artifacts: []
side_effect: W2
determinism: D0
cli: [fid view reject-proposal]
mcp: [view_reject_proposal]
journal: [view.reject_proposal@1]
updated: 2026-11-17
---

# VIZ.INBOX.ACTIONS.REJECT: Reject a proposal and keep it in history

## Purpose

Closes a proposal without applying it. CORE has no branch-close command, so rejection is recorded by `view.reject_proposal@1`, a human-only W2 entry naming the proposal, its branch, span and head, and the reviewer's reason. Nothing is deleted: the agent branch and its entries stay readable, and the proposal list treats a proposal with a reject entry for its current head as closed.

## Scope

**In:** `view.reject_proposal@1` (W2, `human_only: true`); the closed-state rule (rejected at head H; a later entry on the branch reopens it as a new proposal state); the reject dialog.
**Out:** listing (`VIZ.INBOX.LIST`, which reads these entries); pending-request rejection (`VIZ.INBOX.ACTIONS.REQS`); branch storage (`CORE.JRN.BR`).

## Requirements

- **REQ-VIZ.INBOX.ACTIONS.REJECT-001** `view.reject_proposal@1` shall be declared `human_only: true`, so agent and script calls become pending requests and reject nothing.
- **REQ-VIZ.INBOX.ACTIONS.REJECT-002** The command shall journal one entry with proposal ID, branch, span, proposal head and a reason of 1 to 2,000 characters, and shall delete or rewrite no entry.
- **REQ-VIZ.INBOX.ACTIONS.REJECT-003** A proposal shall be closed as rejected while a reject entry names its current head, and every entry of the rejected branch shall remain readable through the journal.
- **REQ-VIZ.INBOX.ACTIONS.REJECT-004** If the named head is not the proposal's current head, then the command shall fail with `HeadMismatch` and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-3branches`, when a human runs `fid view reject-proposal --id <p> --head <h> --reason "wrong datum" --json`, then one entry is journaled, `fid view list-proposals` no longer shows it, `--include-closed` shows it as rejected, and `fid journal list-branches` still lists the branch.
- **AC-2** (agent) Given an MCP agent call, then the result is `PendingRequest` and the proposal stays open.
- **AC-3** (error path) Given a stale head, then `HeadMismatch` and nothing is journaled.
- **AC-4** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.reject_proposal@1`, W2, `human_only: true`, D0; params `{ id: ProposalId, head: EntryHash, reason: String (1..=2000) }`; result `{ entry }`.
- **Algorithm:** resolve the proposal; compare head; append the entry on the target branch.
- **Errors:** `UnknownProposal`, `HeadMismatch`, `ReasonLength`.
- **Determinism:** D0.
- **Performance budget:** < 10 ms excluding commit [unverified].
- **Crate:** `fid-inbox` (module `actions::reject`).

## Test fixtures

`fixtures/viz/inbox-3branches` (shared).

## Build prompt

> In crate `fid-inbox`, add module `actions::reject` with registry command `view.reject_proposal@1` (W2, `human_only: true`; CLI `fid view reject-proposal`, MCP `view_reject_proposal` request-only). Journal one entry with proposal ID, branch, span, head and reason (1–2,000 chars) on the target branch; fail with `HeadMismatch` if the head is not the proposal's current head. Delete nothing; expose `is_rejected(proposal)` for the list module (rejected while a reject entry names the current head). Tests AC-1..AC-4. Rules: humans adopt or reject agent work (C5); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- `VIZ.INBOX.LIST` (leaf) must read `is_rejected` for its closed state; if CORE later adds a branch-close command, this command should map onto it.

## Children

_Leaf: none._
