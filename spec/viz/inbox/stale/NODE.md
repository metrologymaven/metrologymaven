---
id: VIZ.INBOX.STALE
title: Stale proposal detection and apply gate
level: 3
parent: VIZ.INBOX
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX-004]
covers: []
depends_on: [VIZ.INBOX.LIST, CORE.JRN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.INBOX.STALE: Stale proposal detection and apply gate

## Purpose

Mark a proposal stale when the target branch head has moved since the proposal's base head, block apply while it is stale, and offer re-execution on the new head through CORE's re-execute command. The rule is a pure function of the proposal and the current target head.

## Scope

**In:** the staleness predicate, the apply gate state exposed to the action chips, the stale badge, wiring of the "re-execute" action to CORE.
**Out:** re-execution itself (`CORE`); action chips (`VIZ.INBOX.ACTIONS`).

## Requirements

- **REQ-VIZ.INBOX.STALE-001** A proposal shall be stale when its base head differs from the current head of its target branch.
- **REQ-VIZ.INBOX.STALE-002** While a proposal is stale, the apply gate shall report apply as disabled with the reason "target moved".
- **REQ-VIZ.INBOX.STALE-003** When the target head moves, every affected proposal's stale state shall update within 1 s [unverified].
- **REQ-VIZ.INBOX.STALE-004** The stale state shall be included in `view.list_proposals@1` and `view.show_proposal@1` results.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-3branches`, when a human commit moves `main` after branch 1 was created, then branch 1 is stale and branches created after the move are not.
- **AC-2** Given a stale proposal, then the apply gate returns `Disabled { reason: TargetMoved }`.
- **AC-3** Given re-execution recorded on the new head (proposal base head updated), then the proposal is no longer stale.
- **AC-4** Given the same proposal and head, then the predicate returns the same answer (pure function).

## Design

- **Registry command(s):** none new; adds `stale: bool` to `ProposalSummary` and the proposal detail.
- **Data structures:** `ApplyGate { Enabled, Disabled { reason } }`.
- **Algorithm:** compare `base_head` to the target's current head; subscribe to head-moved events.
- **Errors:** none (predicate).
- **Determinism:** D0.
- **Performance budget:** under 1 ms per proposal.
- **Crate:** `fid-inbox` (module `stale`).

## Test fixtures

`fixtures/viz/inbox-3branches` (shared with VIZ.INBOX.LIST).

## Build prompt

> In crate `fid-inbox`, add module `stale`: a pure predicate `is_stale(proposal, target_head) -> bool` (base head differs from the target's current head) and an `ApplyGate` that is `Disabled { reason: TargetMoved }` while stale. Subscribe to journal head-moved events and update stale state within 1 s. Add `stale` to proposal summaries and details. Wire the "re-execute" action to CORE's re-execute command; never re-execute or apply from this module. Tests: head move marks older proposals stale only; gate disabled; re-execution clears stale; purity. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- The CORE command that re-executes a branch on a new head must exist; CORE.JRN branch model is assumed to provide it.

## Children

_Leaf: none._
