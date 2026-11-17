---
id: VIZ.SHELL.TREE.RESULTS
title: Results table with outcome filters and fly-to
level: 4
parent: VIZ.SHELL.TREE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.TREE-001, REQ-VIZ.SHELL.TREE-004, REQ-VIZ.SHELL.TREE-005]
covers: []
depends_on: [VIZ.SHELL.SPKTABLE, VIZ.SHELL.DOCK.HOST, VIZ.SHELL.A11Y.AXTREE, VIZ.OVERLAY.STATUS, VIZ.VIEW.CAMERA.SET]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.TREE.RESULTS: Results table with outcome filters and fly-to

## Purpose

The table metrology users live in: one row per ResultRecord with characteristic, nominal, tolerance, measured value, deviation, decision outcome, evaluation basis badge and UEC level, all as kernel-formatted strings; filters for NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, stale and not evaluated; sort; copy as TSV; click to fly to. Each row exposes the roles and attributes that acceptance checks read.

## Scope

**In:** results panel `viz.results`; virtual scrolling with frozen columns; filters and sorting on kernel-supplied fields; basis badge per row; fly-to via `view.fly_to@1`; copy as TSV of the displayed strings; accessibility roles `value_of_record` and `not_decidable`, attributes `basis`, `reason_code`, `entity_id`, and a Why? action per value.
**Out:** computing or reformatting values (kernel, `KRN.NUM.FMT`); report tables (`RPT`); Why? panel (`VIZ.SHELL.TREE.WHY`).

## Requirements

- **REQ-VIZ.SHELL.TREE.RESULTS-001** Each row shall display the kernel-formatted strings of the ResultRecord unchanged, and sorting by a numeric column shall use the kernel-supplied f64 or decimal value, never a parse of the displayed string.
- **REQ-VIZ.SHELL.TREE.RESULTS-002** The table shall filter by outcome CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE, stale and not evaluated, and show the count per outcome.
- **REQ-VIZ.SHELL.TREE.RESULTS-003** Each row shall show an evaluation-basis badge (`POINTS`, `FEATURES`, `RESULTS_ONLY`, `DIRECT_READING`) equal to the ResultRecord's `evaluation_basis`, exposed as the `basis` attribute.
- **REQ-VIZ.SHELL.TREE.RESULTS-004** A NOT_DECIDABLE row shall have role `not_decidable` with its `reason_code`, and every value cell shall have role `value_of_record` with a Why? action carrying its `command` attribute.
- **REQ-VIZ.SHELL.TREE.RESULTS-005** Activating a row shall invoke `view.fly_to@1` for its characteristic, and the table shall offer no edit of a value of record.
- **REQ-VIZ.SHELL.TREE.RESULTS-006** Characteristic names and notes shall be rendered as untrusted plain text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/results-500` (500 ResultRecords with all outcomes and bases), when the NONCONFORMS filter is applied, then the visible rows and counts equal the fixture's expectation.
- **AC-2** Given a row with `evaluation_basis = RESULTS_ONLY`, then its badge and `basis` attribute read `RESULTS_ONLY` (supports `FLOW.RUNNER.CHKUI` `basis_label`).
- **AC-3** Given a NOT_DECIDABLE row with `decide.rule_missing`, then its node has role `not_decidable`, `reason_code = decide.rule_missing`, and a fix action is reachable within one activation (supports `FLOW.DEADEND.WHY`).
- **AC-4** Given sort by deviation, then the order equals sorting the kernel values, including rows whose display strings round to the same text.
- **AC-5** Given row activation, then `view.fly_to@1` is submitted with that characteristic and nothing is journaled.

## Design

- **Registry command(s):** none of its own; submits `view.fly_to@1` and fix commands.
- **Data structures:** `ResultRow { entity_id, cells: Vec<KernelString>, sort_keys: Vec<SortKey>, outcome, basis, uec, reason_code }`.
- **Algorithm:** rows built from ResultRecords; sort keys from kernel values; fix actions from the fix catalogue (via `VIZ.SHELL.TREE.PROBLEMS` service).
- **Errors:** none specific.
- **Determinism:** D0 (rows and sort order).
- **Performance budget:** 10^5 rows: p95 frame time under 33 ms on H1 while scrolling [unverified].
- **Crate:** `fid-shell` (module `tree::results`).

## Test fixtures

`fixtures/viz/results-500` (synthetic ResultRecords, project-authored).

## Build prompt

> In crate `fid-shell`, add `tree::results`: a results table panel (`viz.results`) with one row per ResultRecord showing kernel-formatted strings unchanged (sorting by kernel values, never parsed strings), outcome filters with counts (including stale and not evaluated), an evaluation-basis badge per row equal to `evaluation_basis`, roles `value_of_record`/`not_decidable` with `basis`, `reason_code`, `entity_id` attributes and Why? and fix actions carrying `command`, row activation submitting `view.fly_to@1`, TSV copy of displayed strings, and no editing of values of record. Untrusted names. Tests AC-1..AC-5 with `fixtures/viz/results-500`. Rules: CONSTITUTION §5, §8; C1, C6.

## Open questions

- None.

## Children

_Leaf: none._
