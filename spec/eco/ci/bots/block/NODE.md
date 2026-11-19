---
id: ECO.CI.BOTS.BLOCK
title: Maintainer-marked blocking findings check
level: 4
parent: ECO.CI.BOTS
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
built_pr: 171
satisfies: [REQ-ECO.CI.BOTS-002]
covers: []
depends_on: []
uses_artifacts: [ECO.CI.BOTS.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.CI.BOTS.BLOCK: Maintainer-marked blocking findings check

## Purpose

Bot findings are advisory until a maintainer says otherwise. A maintainer marks a bot's review thread blocking by replying `/blocking` (and can undo it with `/advisory`); while any such thread is unresolved, the required check `bot-blocking` fails. The evaluator is a pure function over an exported list of review threads, the bot register and the maintainer list, so it is testable offline; a thin workflow exports the threads and reports the check result.

## Scope

**In:**
- The `/blocking` and `/advisory` reply convention and who may use it
- `cargo xtask bot-blocking`: evaluate threads, print the blocking ones, exit status
- The `bot-blocking` check name in the required-check set (declared in ECO.CI.WF.REQSET's file)

**Out:**
- Producing findings (owned by `ECO.CI.BOTS.REVIEW` and any registered hosted bot)
- Exporting threads from the forge and posting the check status (thin workflow in `ECO.CI.WF`)
- Who is a maintainer (GOVERNANCE.md; supplied as an input list)

## Requirements

- **REQ-ECO.CI.BOTS.BLOCK-001** A thread shall count as blocking when its root comment is by an account in the bot register and the latest `/blocking` or `/advisory` reply by an account in the maintainer list is `/blocking`.
- **REQ-ECO.CI.BOTS.BLOCK-002** When at least one blocking thread is unresolved, the check shall exit 1 and list each thread's ID, path, line and the maintainer who marked it; otherwise it shall exit 0.
- **REQ-ECO.CI.BOTS.BLOCK-003** Replies of `/blocking` or `/advisory` by accounts not in the maintainer list shall be ignored and listed as `ignored-mark` notes.
- **REQ-ECO.CI.BOTS.BLOCK-004** Threads whose root comment is not by a registered bot account shall never be blocking under this check.
- **REQ-ECO.CI.BOTS.BLOCK-005** If the thread export is missing, malformed or larger than 10 MiB, then the check shall exit 1 with `BLOCK-INPUT` (fail closed).

## Acceptance criteria

- **AC-1** Given `fixtures/block/threads-advisory.json` (two bot threads, no marks), `fixtures/block/bots.toml` and `fixtures/block/maintainers.txt`, when `cargo xtask bot-blocking --threads fixtures/block/threads-advisory.json --register fixtures/block/bots.toml --maintainers fixtures/block/maintainers.txt` runs, then it exits 0.
- **AC-2** (error path) Given `threads-blocking.json` (a maintainer replied `/blocking`, thread unresolved), then it exits 1 and lists that thread and maintainer.
- **AC-3** Given `threads-unmarked.json` (the same maintainer later replied `/advisory`), then it exits 0; given `threads-resolved.json` (blocking but resolved), then it exits 0.
- **AC-4** Given `threads-non-maintainer.json` (a non-maintainer replied `/blocking`), then it exits 0 and prints one `ignored-mark` note.
- **AC-5** (error path) Given a truncated JSON file, then it exits 1 with `BLOCK-INPUT`.

## Design

- **Registry command(s):** none; repository tool `cargo xtask bot-blocking --threads <json> --register <bots.toml> --maintainers <txt>`.
- **Data structures:** thread export `[{id, path, line, resolved, comments: [{author, at, body}]}]`; output lines `blocking <id> <path>:<line> by <maintainer>`.
- **Algorithm:** for each thread whose root author is a register `account`, scan replies in `at` order and keep the last maintainer mark (a body whose first token is `/blocking` or `/advisory`); blocking and not resolved → report.
- **Errors:** `BLOCK-INPUT`.
- **Determinism:** D0; output sorted by thread ID.
- **Performance budget:** under 1 s for 2 000 threads [unverified until measured].
- **Crate:** `fid-xtask` (module `bots`).

## Test fixtures

Hand-written thread exports, register and maintainer list under `fixtures/block/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `bot-blocking --threads <json> --register <bots.toml> --maintainers <txt>`. Threads: `[{id, path, line, resolved, comments: [{author, at, body}]}]` (max 10 MiB; else exit 1 `BLOCK-INPUT`). A thread is blocking when its root comment's author is an `account` in the bot register and the last reply by a listed maintainer whose body starts with `/blocking` or `/advisory` is `/blocking`; marks by non-maintainers are ignored and printed as `ignored-mark` notes. Exit 1 if any blocking thread is unresolved, listing `blocking <id> <path>:<line> by <maintainer>` sorted by id; else exit 0. Tests for AC-1..AC-5 of spec node ECO.CI.BOTS.BLOCK. No clock reads; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- The `/blocking` comment itself is a human reply, so it does not affect the PR-timeline clock rule for bots (ECO.COMM.TIMELINE); confirm with that leaf's event kinds.

## Children

_Leaf: none._
