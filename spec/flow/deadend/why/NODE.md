---
id: FLOW.DEADEND.WHY
title: Why? and fix-reachability coverage check over golden journals
level: 3
parent: FLOW.DEADEND
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.DEADEND-005, REQ-FLOW.DEADEND-006, REQ-FLOW.DEADEND-007]
covers: [JOB.USE.05, JOB.USE.09]
depends_on: [FLOW.DEADEND.CATALOG, AUTO.CTRL.UIOPS, VIZ.SHELL.A11Y, VIZ.SHELL.TREE, CORE.RPL.EXEC]
uses_artifacts: [FLOW.RUNNER.PKG]
side_effect: R1
determinism: D1
cli: [fid accept check-why]
mcp: [accept_check_why]
journal: [accept.check_why@1]
updated: 2026-11-17
---

# FLOW.DEADEND.WHY: Why? and fix-reachability coverage check over golden journals

## Purpose

Checks the GUI side of "no dead ends" on real jobs. It replays each golden journal of the acceptance suite in a headless app session, walks the accessibility tree at each step, and reports every displayed value of record or NOT_DECIDABLE result that does not open a "Why?" explanation, every catalogued state with no fix action one interaction away, and every standards-based explanation without a structured citation.

## Scope

**In:**
- `accept.check_why@1` (R1) over the registered acceptance-job packages
- Replay of each package in a headless app with the control channel, and accessibility-tree inspection after each step
- Three finding kinds: missing "Why?", unreachable fix, uncited or uncatalogued standards explanation

**Out:**
- The "Why?" panel and its content (owned by `VIZ.SHELL.TREE`, `UNC.UEC.EXPL`, `UNC.DEC.DEFAULT`, `MIG.EXPLAIN`)
- The accessibility tree and UI inspection (owned by `VIZ.SHELL.A11Y`, `AUTO.CTRL.UIOPS`)
- Golden journals and the package format (owned by the job-family nodes and `FLOW.RUNNER.PKG`)

## Requirements

- **REQ-FLOW.DEADEND.WHY-001** When `fid accept check-why` runs, the system shall replay every registered acceptance-job package in a headless app session and inspect the accessibility tree after each replayed step.
- **REQ-FLOW.DEADEND.WHY-002** For each node whose role marks a displayed value of record or a NOT_DECIDABLE outcome, the check shall report `WHY-MISSING <job> <step> <entity>` if the node exposes no "Why?" action.
- **REQ-FLOW.DEADEND.WHY-003** For each displayed state carrying a catalogued reason code, the check shall report `FIX-UNREACHABLE <job> <step> <reason>` unless an action naming one of that reason's fix commands is reachable from the state's node by at most one activation.
- **REQ-FLOW.DEADEND.WHY-004** For each "Why?" explanation of a standards-based result, the check shall report `WHY-UNCITED` if the explanation lacks a structured citation of designator, edition and clause, and `WHY-UNCATALOGUED-TEXT` if any explanation string is not drawn from a project string table.
- **REQ-FLOW.DEADEND.WHY-005** The command shall return per-job counts of inspected displays and findings, fail with exit code 1 when any finding exists, and list findings sorted by job, step and entity.

## Acceptance criteria

- **AC-1** Given fixture package `fixtures/flow/why/ok-pkg` (a 5-step journal on a 3-characteristic synthetic part where every value has a Why? action), when `fid accept check-why --json` runs, then it exits 0 with 0 findings and an inspected-display count of at least 3.
- **AC-2** (error path) Given `fixtures/flow/why/nowhy-pkg` (a test build with the Why? action removed from one result row), then it exits 1 with exactly one `WHY-MISSING`.
- **AC-3** (error path) Given `fixtures/flow/why/rulemissing-pkg` (an ASME result NOT_DECIDABLE with `decide.rule_missing` and the fix action hidden behind two menus), then it reports `FIX-UNREACHABLE`.
- **AC-4** (error path) Given an explanation fixture citing only free text, then it reports `WHY-UNCITED`.
- **AC-5** Running AC-2 twice on the same platform gives identical findings.

## Design

- **Registry command(s):** `accept.check_why@1`, R1; optional parameter `job` (filter by job ID). Result `{packages:[{job, variant, displays, findings:[...]}]}`.
- **Algorithm:** for each package: start a headless app on a scratch copy, replay step by step through `CORE.RPL.EXEC`'s step cursor, query the accessibility tree through the control channel, classify nodes by role (`value_of_record`, `not_decidable`, `catalogued_state` with `reason_code` attribute), search one activation deep for actions whose `command` attribute matches a catalogue fix, inspect explanation nodes for a `standards_ref` attribute and string-table IDs.
- **Errors:** `WHY-PKG` (package invalid), `WHY-APP` (headless app failed to start); both fail the command.
- **Determinism:** D1 (accessibility-tree order depends on the platform toolkit; findings are sorted).
- **Performance budget:** under 10 minutes for the launch-1 suite on a CI runner [unverified].
- **Crate:** `fid-accept`.

## Test fixtures

Small synthetic packages under `fixtures/flow/why/` built with the package format of `FLOW.RUNNER.PKG` (project-authored, CC0 geometry).

## Build prompt

> In crate `fid-accept`, add `accept.check_why@1` (R1; CLI `fid accept check-why [--job <id>]`; MCP `accept_check_why`). For each registered acceptance-job package (format from FLOW.RUNNER.PKG), start a headless app session on a scratch copy, replay the golden journal step by step via the replay engine's step cursor, and after each step read the accessibility tree via the control channel. Report `WHY-MISSING` (value-of-record or NOT_DECIDABLE node without a Why? action), `FIX-UNREACHABLE` (catalogued reason state with no fix-command action within one activation; fix commands come from the `fid-fixes` catalogue), `WHY-UNCITED` and `WHY-UNCATALOGUED-TEXT` (standards explanation without designator/edition/clause attributes, or with strings outside project string tables). Sorted findings, per-job counts, exit 1 on any finding. Tests AC-1..AC-5 of spec node FLOW.DEADEND.WHY. Never store or emit standards text. CONSTITUTION §8 rules apply.

## Open questions

- The check needs the accessibility tree to expose roles `value_of_record`, `not_decidable` and `catalogued_state`, a `reason_code` attribute, a `command` attribute on actions and a `standards_ref` attribute on explanations. These are requests to `VIZ.SHELL.A11Y` and `VIZ.SHELL.TREE`.

## Children

_Leaf: none._
