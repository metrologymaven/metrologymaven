---
id: ECO.COMM.RFC
title: RFC comment-period tooling
level: 3
parent: ECO.COMM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.COMM-006]
covers: []
depends_on: [ECO.COMM.SCOPE]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.COMM.RFC: RFC comment-period tooling

## Purpose

Implements the GOVERNANCE.md RFC timings (D-003) on decision records: a maintainer starts the comment period, the tool records its start and length (3 days, or 7 for adding or removing a domain or app or changing the process), reports when it has closed, and blocks approval by the record's author or before the period ends.

## Scope

**In:**
- `tools/rfc.py start <D-NNN>`: write `comment_period_start`, `comment_period_days` and `rfc_kind` into the record's frontmatter
- `tools/rfc.py status`: list records with open and closed periods
- `tools/rfc.py approve-check <D-NNN>`: validate an approval set exported from the forge

**Out:**
- The decision template (`templates/DECISION.md`) itself; adding the new fields to it is a tooling PR outside this tree's node files
- Who may accept a record (GOVERNANCE.md, D-003)
- The app-list RFC rule (owned by `ECO.APPS.LIST`, which reads these fields)

## Requirements

- **REQ-ECO.COMM.RFC-001** When `start` runs on a record, the tool shall set `comment_period_start` to `--now`, and `comment_period_days` to 7 if `rfc_kind` is `domain`, `app` or `process` and to 3 otherwise, and shall refuse if a period is already recorded.
- **REQ-ECO.COMM.RFC-002** `status` shall report each record with a recorded period as `open` or `closed` at `--now`, with its close time.
- **REQ-ECO.COMM.RFC-003** If the approvals for a record include no maintainer other than the record's `raised_by` author, then `approve-check` shall exit 1 with `RFC-SELF-APPROVAL`.
- **REQ-ECO.COMM.RFC-004** If `approve-check` runs before the period has closed, then it shall exit 1 with `RFC-PERIOD-OPEN`.
- **REQ-ECO.COMM.RFC-005** The 3-day and 7-day lengths and the kinds that get 7 days shall be read from a config file, so a governance change needs no code change.
- **REQ-ECO.COMM.RFC-006** `start` shall change only the three frontmatter fields and leave every other byte of the record unchanged.

## Acceptance criteria

- **AC-1** Given `fixtures/rfc/D-100-app.md` with `rfc_kind: app`, when `python tools/rfc.py start D-100 --dir fixtures/rfc --now 2026-12-01T09:00:00Z` runs on a copy, then the frontmatter gains `comment_period_start: 2026-12-01T09:00:00Z` and `comment_period_days: 7`, and a byte diff shows no other change.
- **AC-2** (error path) Running AC-1 twice exits 1 on the second run with `RFC-ALREADY-STARTED` and leaves the file unchanged.
- **AC-3** Given the AC-1 result, `status --now 2026-12-05T00:00:00Z` reports D-100 `open`; at `2026-12-08T09:00:01Z` it reports `closed`.
- **AC-4** (error path) Given `fixtures/rfc/approvals-self.json` (only the author approved) after the close, `approve-check D-100` exits 1 with `RFC-SELF-APPROVAL`; with `approvals-other.json` it exits 0.
- **AC-5** (error path) Given `approvals-other.json` at `2026-12-03T00:00:00Z`, `approve-check` exits 1 with `RFC-PERIOD-OPEN`.

## Design

- **Registry command(s):** none; repository tool `python tools/rfc.py start|status|approve-check`. `side_effect: W1` because `start` edits a record in the working tree; it never commits.
- **Data structures:** decision frontmatter fields `rfc_kind` (`domain|app|process|other`), `comment_period_start` (ISO8601 UTC), `comment_period_days` (int); config `short_days = 3`, `long_days = 7`, `long_kinds = [domain, app, process]`, `maintainers = [...]`; approvals JSON `[{actor, state, at}]`.
- **Algorithm:** line-level edit of the frontmatter block only (insert or refuse), preserving line endings; time arithmetic in UTC.
- **Errors:** `RFC-ALREADY-STARTED`, `RFC-NO-RECORD`, `RFC-SELF-APPROVAL`, `RFC-PERIOD-OPEN`, `RFC-CONFIG`.
- **Determinism:** D0 given `--now`.
- **Performance budget:** trivial.
- **Crate:** not Rust; `tools/rfc.py`, standard library only.

## Test fixtures

Hand-written decision records and approval JSON under `tools/tests/fixtures/rfc/`.

## Build prompt

> In the spec repository, write `tools/rfc.py`, Python 3.9+ stdlib only, with subcommands `start <D-NNN>`, `status`, `approve-check <D-NNN>`, options `--dir` (default `decisions/`), `--now`, `--config`, `--approvals <json>`. `start` inserts `comment_period_start` and `comment_period_days` (7 for `rfc_kind` in `long_kinds`, else 3) into the frontmatter, changing no other bytes, and refuses if already present. `status` lists records as open/closed. `approve-check` fails with `RFC-PERIOD-OPEN` before close and `RFC-SELF-APPROVAL` unless a maintainer other than `raised_by` approved. Config: `short_days`, `long_days`, `long_kinds`, `maintainers` (JSON). Write `unittest` tests for AC-1..AC-5 of spec node ECO.COMM.RFC.

## Open questions

- `templates/DECISION.md` needs the three new fields; that is a separate tooling PR.
- Mapping forge user names to `raised_by` values (which may say "founder" or a node ID) needs a small alias table in the config.

## Children

_Leaf: none._
