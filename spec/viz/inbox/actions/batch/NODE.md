---
id: VIZ.INBOX.ACTIONS.BATCH
title: Plan preview and batch apply by span
level: 4
parent: VIZ.INBOX.ACTIONS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.ACTIONS-001, REQ-VIZ.INBOX.ACTIONS-003, REQ-VIZ.INBOX.ACTIONS-005]
covers: []
depends_on: [VIZ.INBOX.ACTIONS.CHIPS, VIZ.INBOX.GATES.LEAN]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.ACTIONS.BATCH: Plan preview and batch apply by span

## Purpose

For a multi-step proposal (a branch with several tagged spans) and for a multi-selection of proposals, shows a plan preview as a checklist grouped by span, and lets the human apply all, apply span by span, or stop. Batch apply always asks for the total command count to be typed and lists every side-effect class first (`ConfirmCount`); each span is then promoted in order with its own end head as `source_head`, stopping at the first failure.

## Scope

**In:** plan-preview model (spans in order, each with command count, classes and titles); apply-all and span-by-span flows; forced `ConfirmCount` step for any batch of more than one span or proposal; stop-on-first-failure and the report of what was applied.
**Out:** single-proposal chips (`VIZ.INBOX.ACTIONS.CHIPS`); gate selection (`VIZ.INBOX.GATES.PLAN`); promotion (`CORE.JRN.BR.PROMO`).

## Requirements

- **REQ-VIZ.INBOX.ACTIONS.BATCH-001** The plan preview shall list the spans of the selection in journal order with each span's command count, side-effect classes and command titles.
- **REQ-VIZ.INBOX.ACTIONS.BATCH-002** Before a batch of two or more spans or proposals is applied, the human shall type the batch's total command count and see every side-effect class in it.
- **REQ-VIZ.INBOX.ACTIONS.BATCH-003** Batch apply shall promote spans in journal order, each with its end head as `source_head`, through the same promotion path as a single apply.
- **REQ-VIZ.INBOX.ACTIONS.BATCH-004** If promoting a span fails, then the batch shall stop, keep later spans pending and report which spans were applied.
- **REQ-VIZ.INBOX.ACTIONS.BATCH-005** While any span in the batch is stale or its gate plan is `Unavailable`, batch apply shall be disabled with the reason.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-3branches` (branch 3 has two spans of 3 and 4 commands), when the plan preview opens, then it shows two groups with 3 and 4 commands and their classes.
- **AC-2** Given apply-all, when the user types 6, then confirm stays disabled; typing 7 promotes span 1 then span 2 with their end heads.
- **AC-3** (error path) Given a fake promotion that fails on span 2, then span 1 is applied, span 2 stays pending and the report says so.
- **AC-4** Given a stale span, then batch apply is disabled with "target moved".

## Design

- **Registry command(s):** none owned.
- **Data structures:** `PlanPreview { groups: [SpanGroup { span, end_head, count, classes, titles }] }`; `BatchReport { applied: [span], failed: Option<(span, error)>, pending: [span] }`.
- **Algorithm:** build groups from proposal summaries; plan = `ConfirmCount` + span plans merged; run gates once; promote sequentially via the chips' promotion function.
- **Errors:** passed through from promotion.
- **Determinism:** D0 for preview and order.
- **Performance budget:** preview < 50 ms for 50 spans [unverified].
- **Crate:** `fid-inbox` (module `actions::batch`).

## Test fixtures

`fixtures/viz/inbox-3branches` (shared).

## Build prompt

> In crate `fid-inbox`, add module `actions::batch`: a plan preview grouping a proposal's (or a multi-selection's) spans in journal order with count, classes and titles; apply-all or span-by-span; for two or more spans always run `ConfirmCount` (typed total count, every class listed) before anything; promote spans in order with each span's end head as `source_head` via the chips' promotion function; stop at the first failure and report applied, failed and pending spans; disable with the reason while any span is stale or gated `Unavailable`. Tests AC-1..AC-4. Rules: human-only promotion (C5); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
