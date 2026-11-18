---
id: ECO.COMM.TIMELINE
title: PR-timeline evaluator (author-blocked clock)
level: 3
parent: ECO.COMM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-timeline]
built_pr: 77
satisfies: [REQ-ECO.COMM-005]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.COMM.TIMELINE: PR-timeline evaluator

## Purpose

Implements the Craft-style PR timeline: a reminder after 14 days and a proposed closure after 30 days, where the clock runs only while the **author** is the blocker and bot activity never moves it. The evaluator is a pure function from a PR's event history to the action due now; a thin CI workflow posts the comment or label it returns.

## Scope

**In:**
- Event model for a PR timeline and the rules for who is the blocker
- The author-blocked clock and the actions `none`, `remind`, `propose-close`
- Idempotence: an action already taken is not repeated

**Out:**
- Posting comments and labels (thin workflow in `ECO.CI.WF`)
- Which accounts are bots: read from a config list maintained with `ECO.CI.BOTS`
- Closing PRs: a maintainer does that; the tool only proposes

## Requirements

- **REQ-ECO.COMM.TIMELINE-001** The author shall become the blocker when a maintainer submits a "changes requested" review or applies the `waiting-on-author` label, and shall stop being the blocker when the author pushes a commit, comments, or re-requests review, or the label is removed.
- **REQ-ECO.COMM.TIMELINE-002** Events by accounts in the bot list shall not change the blocker state.
- **REQ-ECO.COMM.TIMELINE-003** The author-blocked clock shall be the sum of all intervals in which the author was the blocker, up to `--now`.
- **REQ-ECO.COMM.TIMELINE-004** When the clock reaches 14 days and no reminder has been posted since the author last stopped being the blocker, the evaluator shall return `remind`; when it reaches 30 days and no closure proposal exists, it shall return `propose-close`; otherwise `none`.
- **REQ-ECO.COMM.TIMELINE-005** The thresholds (14 and 30 days) shall be read from a config file, and the evaluator shall use only the Python standard library.

## Acceptance criteria

- **AC-1** Given `fixtures/timeline/active.json` (changes requested on day 0, author push on day 5, changes requested on day 6), when `python tools/timeline.py --events fixtures/timeline/active.json --now <day 12> --json` runs, then it returns `{"clock_days": 11, "action": "none"}`.
- **AC-2** Given `fixtures/timeline/stalled.json` (changes requested on day 0, nothing after), when run at day 15, then the action is `remind`; given the same with a posted reminder at day 14, at day 20 the action is `none`; at day 31 it is `propose-close`.
- **AC-3** Given `fixtures/timeline/bots.json` (as stalled, plus bot comments on days 3 and 10 by an account in `fixtures/timeline/bots.toml`), when run at day 15, then the action is `remind`.
- **AC-4** Given `fixtures/timeline/maintainer-blocked.json` (review requested, no maintainer response for 40 days), when run at day 40, then the clock is 0 and the action is `none`.
- **AC-5** (error path) Given malformed JSON, when run, then the tool exits 1 with `TIMELINE-INPUT` and returns no action.

## Design

- **Registry command(s):** none; repository tool `python tools/timeline.py --events <json> --now <ISO8601> [--config <toml-or-json>] [--json]`.
- **Data structures:** events `[{at, actor, kind}]` with kinds `opened`, `review_changes_requested`, `review_approved`, `review_commented`, `label_added`, `label_removed`, `commit_pushed`, `comment`, `review_requested`, `bot_reminder_posted`, `bot_close_proposed`; config `remind_days`, `close_days`, `maintainers[]`, `bots[]`, `author`. Config parsed as JSON (stdlib) or TOML via `tomllib` where available (Python 3.11+).
- **Algorithm:** sort events by (time, original index); state machine over blocker ∈ {author, maintainers}; accumulate author-blocked seconds; apply thresholds and idempotence markers.
- **Errors:** `TIMELINE-INPUT`, `TIMELINE-CONFIG`.
- **Determinism:** D0 given `--now`.
- **Performance budget:** under 100 ms per PR with 1 000 events [unverified].
- **Crate:** not Rust; `tools/timeline.py`, standard library only.

## Test fixtures

Hand-written event JSON files with relative day offsets resolved from a fixed epoch, under `tools/tests/fixtures/timeline/`.

## Build prompt

> In the spec repository (the tool is shared with code repositories later), write `tools/timeline.py`, Python 3.9+ stdlib only. Input: an events JSON list (`at` ISO8601, `actor`, `kind` as listed in spec node ECO.COMM.TIMELINE), a config (JSON; TOML only if `tomllib` exists) with `author`, `maintainers`, `bots`, `remind_days` (14), `close_days` (30), and `--now`. The author becomes blocker on a maintainer's changes-requested review or `waiting-on-author` label; stops on author push, author comment, review re-request or label removal; bot events are ignored. Sum author-blocked time; return `remind` at ≥ remind_days unless a reminder was posted since the author last stopped being blocker, `propose-close` at ≥ close_days unless already proposed, else `none`. Output `{"clock_days": <int floor>, "action": ...}`. Exit 1 with `TIMELINE-INPUT` on malformed input. Write `unittest` tests for AC-1..AC-5.

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** an independent agent reviewer's approval counts as the one approving review for L3+ (D-037 P2).
- The 7-day first-response target for maintainers (research/artcraft-design-lessons.md item 22) is not in REQ-ECO.COMM-005; a maintainer-side report could be added later under ECO.COMM.

## Children

_Leaf: none._
