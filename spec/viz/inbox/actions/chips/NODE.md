---
id: VIZ.INBOX.ACTIONS.CHIPS
title: Action chips, apply through promotion, edit and explain
level: 4
parent: VIZ.INBOX.ACTIONS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.ACTIONS-001, REQ-VIZ.INBOX.ACTIONS-002, REQ-VIZ.INBOX.ACTIONS-005]
covers: []
depends_on: [VIZ.INBOX.STALE, VIZ.INBOX.GATES.PLAN, VIZ.INBOX.DIFF.SHOW, CORE.JRN.BR.PROMO, CORE.JRN.BR.NAME]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.ACTIONS.CHIPS: Action chips, apply through promotion, edit and explain

## Purpose

The per-proposal controls: **Apply**, **Apply and next**, **Edit**, **Reject** (handled by `VIZ.INBOX.ACTIONS.REJECT`) and **Explain**. Each chip shows the side-effect classes and command count it would affect and is enabled only when the apply gate (stale check) and the gate plan allow. Apply runs the gate steps, then invokes CORE promotion (`journal.promote_branch@1`, or `journal.promote_protected_branch@1` with the ceremony's assertion) as the human operating the GUI. Edit creates a human branch at the proposal head for the reviewer to change; Explain opens the brief's full rationale in the header. This module owns no registry command, so there is no surface by which an agent could apply anything.

## Scope

**In:** chip model and enabled state; apply flow (gate runner → promotion with `source_head` pinned to the proposal's head); apply-and-next (move to the next open proposal in list order); edit (`journal.create_branch@1` at the proposal head named `review/<proposal-id>`, then switch the working view to it); explain (expand the header rationale); disabled reasons.
**Out:** reject (`VIZ.INBOX.ACTIONS.REJECT`); batch apply and plan preview (`VIZ.INBOX.ACTIONS.BATCH`); gate steps (`VIZ.INBOX.GATES`); promotion itself (`CORE.JRN.BR.PROMO`).

## Requirements

- **REQ-VIZ.INBOX.ACTIONS.CHIPS-001** Apply shall invoke `journal.promote_branch@1` (or `journal.promote_protected_branch@1` for a protected target) with the proposal's head as `source_head`, under the human principal of the interactive GUI session, only after the gate runner reports all steps passed.
- **REQ-VIZ.INBOX.ACTIONS.CHIPS-002** The module shall register no registry command, CLI command or MCP tool.
- **REQ-VIZ.INBOX.ACTIONS.CHIPS-003** Each chip shall show the set of side-effect classes and the number of commands it would affect, taken from the proposal summary.
- **REQ-VIZ.INBOX.ACTIONS.CHIPS-004** While the stale gate is disabled or the gate plan is `Unavailable`, Apply and Apply-and-next shall be disabled and show the reason.
- **REQ-VIZ.INBOX.ACTIONS.CHIPS-005** Edit shall create a branch named `review/<proposal-id>` at the proposal head through `journal.create_branch@1` and shall leave the proposal itself pending.
- **REQ-VIZ.INBOX.ACTIONS.CHIPS-006** If promotion returns an error, then the chip shall show it and the proposal shall stay listed as open.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-3branches` and a GUI human session, when Apply is clicked on branch 2 (Lean, unprotected), then the journal holds one promotion (fast-forward `head_move` or re-executed entries with `promoted_from`) with `source_head` equal to the proposal head, and the proposal is closed.
- **AC-2** Given the generated manifest, then no command, CLI or MCP entry belongs to this module.
- **AC-3** Given a stale proposal, then Apply is disabled with "target moved"; given a protected target with no ceremony provider, then disabled with "signature ceremony unavailable".
- **AC-4** Given Edit, then a `review/<id>` branch exists at the proposal head and the proposal is still open.
- **AC-5** (error path) Given promotion failing with `divergent_step`, then the chip shows the error and nothing changes.

## Design

- **Registry command(s):** none owned; invokes `journal.promote_branch@1`, `journal.promote_protected_branch@1`, `journal.create_branch@1`.
- **Data structures:** `Chip { kind, enabled: Result<(), DisabledReason>, classes, count }`.
- **Algorithm:** compute enabled state from `ApplyGate` and `GatePlan`; on click, run `GateRunner`; in its callback invoke promotion through the bus with the session's human principal.
- **Errors:** passed through from CORE (`policy`, `precondition`, `conflict`, `cancelled`).
- **Determinism:** D0 for enabled state.
- **Performance budget:** chip state < 1 ms per proposal.
- **Crate:** `fid-inbox` (module `actions::chips`).

## Test fixtures

`fixtures/viz/inbox-3branches` (shared with VIZ.INBOX.LIST).

## Build prompt

> In crate `fid-inbox`, add module `actions::chips`: chips Apply, Apply-and-next, Edit, Reject (delegated to `actions::reject`) and Explain, each showing side-effect classes and command count from the proposal summary. Enabled state comes from the stale `ApplyGate` and the `GatePlan`; show the reason when disabled. Apply: run `GateRunner`, then invoke `journal.promote_branch@1` (or `journal.promote_protected_branch@1` with the ceremony assertion) via the bus as the GUI session's human principal with `source_head` = proposal head. Edit: `journal.create_branch@1` named `review/<proposal-id>` at the proposal head; proposal stays pending. Register no registry command, CLI or MCP tool. Tests AC-1..AC-5. Rules: humans adopt agent work (C5); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- "Explain" shows the brief already attached; asking the agent for a new explanation is not possible without a channel back to the agent and is not offered.

## Children

_Leaf: none._
