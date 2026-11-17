---
id: UX.RESEARCH.SUS
title: SUS scoring, history and release research reports
level: 3
parent: UX.RESEARCH
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.RESEARCH-004, REQ-UX.RESEARCH-006]
covers: []
depends_on: [UX.RESEARCH.USAB]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.RESEARCH.SUS: SUS scoring, history and release research reports

## Purpose

Turns a release's usability sessions into the published release research report: SUS mean, participant count and 95 % confidence interval, the top issues ranked by severity, and a regression flag when the SUS mean drops more than 5 points from the previous release. It keeps the SUS history across releases.

## Scope

**In:** `fid-design sus-report`; `design/research/sus-history.json` (`sus-history@1`); the report `design/research/release-<version>/report.md` and `report.json`.
**Out:** session records and issues (`UX.RESEARCH.USAB`); GA gating on research results (FLOW and ECO; see Open questions).

## Requirements

- **REQ-UX.RESEARCH.SUS-001** The report shall score each participant's SUS by the standard method (odd items score minus 1, even items 5 minus score, sum times 2.5) and give the mean, participant count and a two-sided 95 % confidence interval using Student's t with n − 1 degrees of freedom.
- **REQ-UX.RESEARCH.SUS-002** When the release mean is more than 5 points [unverified threshold] below the previous release's mean in the history, the report shall set `regression = true` and state it in the first paragraph.
- **REQ-UX.RESEARCH.SUS-003** The report shall list the top issues sorted by severity then by number of participants affected, each with its node ID and tracker reference.
- **REQ-UX.RESEARCH.SUS-004** The report shall record its release date and the session dates, and the publication check shall warn when publication is more than 30 days after the release date.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/research/sus/release-a/` (5 participants with known responses) and empty history, when `fid-design sus-report --sessions release-a --history h.json --json` runs, then the mean equals the hand-computed value to 0.01, the 95 % CI equals the reference values in `expected.json` to 0.01, and `regression = false`.
- **AC-2** Given `release-b/` whose mean is 6.5 below release A in the history, when the report runs, then `regression = true`.
- **AC-3** (error path) Given `bad-item/` (a response of 6), when the report runs, then it exits 1 with `SUS-ITEM-RANGE`.
- **AC-4** Given the same inputs, when the report runs twice, then the outputs are byte-identical.

## Design

- **Command:** `fid-design sus-report --sessions <dir> --history <file> [--issues <file>] [--out <dir>] [--json]`. Tooling, nearest class R0. It updates the history file only with `--write-history`.
- **Statistics:** t quantiles from a built-in table for 1 to 200 degrees of freedom (normal approximation above) to avoid transcendental-function differences; values rounded to 0.01.
- **Errors:** `SUS-ITEM-RANGE`, `SUS-ITEMS-COUNT`, `SUS-NO-SESSIONS`, `SUS-HISTORY-SCHEMA`; warning `SUS-LATE`.
- **Determinism:** D0 (table lookup, fixed-order sums, rounding).
- **Crate:** `fid-design`, module `sus`.

## Test fixtures

`fixtures/ux/research/sus/`: `release-a/`, `release-b/`, `bad-item/`, `expected.json` (hand-computed); synthetic, CC-BY-4.0.

## Build prompt

> In the Rust tooling crate `fid-design`, add `fid-design sus-report --sessions <dir> --history <file> [--issues <file>] [--out <dir>] [--write-history] [--json]`: read `usab-session@1` records, score SUS per participant (odd items r − 1, even items 5 − r, sum × 2.5), compute mean, n and a two-sided 95 % CI with Student's t (built-in quantile table for 1..200 df), compare with the previous release in `sus-history@1` and set `regression` when the drop exceeds 5 points, rank top issues by severity then participant count, and write `report.md` and `report.json`. Warn `SUS-LATE` when the publication date is more than 30 days after the release date. Errors `SUS-ITEM-RANGE|SUS-ITEMS-COUNT|SUS-NO-SESSIONS|SUS-HISTORY-SCHEMA`. Tests AC-1..AC-4 of spec node UX.RESEARCH.SUS.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; fixed-order arithmetic; no network.

## Open questions

- REQ-UX-005 does not say whether failing usability results block GA. FLOW owns GA gates (REQ-FLOW-003); linking SUS or the regression flag to a GA gate needs a maintainer decision and is not set here.

## Children

_Leaf: none._
