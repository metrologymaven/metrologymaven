---
id: OUTR.COMM.SURVEY.AGG
title: Survey aggregation, cell suppression and deletion record
level: 4
parent: OUTR.COMM.SURVEY
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM.SURVEY-003, REQ-OUTR.COMM.SURVEY-004]
covers: []
depends_on: [OUTR.COMM.SURVEY.QSET]
uses_artifacts: []
side_effect: [W1, R0]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.SURVEY.AGG: Survey aggregation, cell suppression and deletion record

## Purpose

Turn a survey's raw response export into publishable aggregates, suppressing or merging every cell with fewer than the minimum number of respondents, and keep the raw data's life short: a deletion record per survey and a check that fails once the retention period has passed without one.

## Scope

**In:** the `survey-agg` command (raw export to aggregates file); cell suppression and merge rules; the deletion record format; the `survey-retention check` command.
**Out:** survey definitions and reportable groupings (`OUTR.COMM.SURVEY.QSET`); the tool and its export (`OUTR.COMM.SURVEY.TOOL`); showing aggregates in the report (`OUTR.COMM.REPORT`).

## Requirements

- **REQ-OUTR.COMM.SURVEY.AGG-001** `survey-agg` shall count respondents per reportable option of each reportable question, and shall never output a free-text answer or any response-level field.
- **REQ-OUTR.COMM.SURVEY.AGG-002** If a cell has fewer than 10 respondents [unverified threshold], then `survey-agg` shall merge it with the neighbouring option the definition names in `merge_with`, or else suppress it and show `<10`, and shall also suppress a second cell when that is needed to stop the suppressed value being derived from the question total.
- **REQ-OUTR.COMM.SURVEY.AGG-003** If the raw export path lies inside a public repository checkout, then `survey-agg` shall fail with `raw_in_repo` and read nothing.
- **REQ-OUTR.COMM.SURVEY.AGG-004** Each survey shall have a deletion record stating the survey ID, the survey close date, the planned deletion date (close date plus `retention_days`), and, once done, the actual deletion date and who deleted the raw data.
- **REQ-OUTR.COMM.SURVEY.AGG-005** If `--today` is after a survey's planned deletion date and its deletion record has no actual deletion date, then `survey-retention check` shall fail with `deletion_overdue` naming the survey.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/survey/agg/ok/` (a definition and a raw CSV export of 60 responses, all cells 10 or more), when `fid-outreach survey-agg --survey survey.toml --raw <tmp>/export.csv --out aggregates.json` runs, then the aggregates hold counts per reportable option and no free text.
- **AC-2** Given `small-cell/` (one option with 4 respondents and `merge_with` set), when aggregation runs, then that option is merged into its neighbour; without `merge_with`, it shows `<10` and one more cell of the same question is suppressed so the total cannot reveal it.
- **AC-3** (error path) Given `--raw` inside the repository checkout, when aggregation runs, then it exits 2 with `raw_in_repo`.
- **AC-4** (error path) Given `deletions.toml` with a planned deletion date of 2027-06-30 and no actual date, when `fid-outreach survey-retention check --deletions deletions.toml --today 2027-07-01` runs, then it exits 1 with `deletion_overdue`.
- **AC-5** (determinism) Given the same inputs, when aggregation runs twice, then the outputs are byte-identical.

## Design

- **Commands:** `fid-outreach survey-agg --survey <file> --raw <file> --out <file> [--min-cell <n>] [--repo-root <dir>]` (nearest class W1) and `fid-outreach survey-retention check --deletions <file> --today <date> [--json]` (nearest class R0). Deleting raw data is a human act in the survey tool; the record is updated by pull request.
- **Raw export:** CSV with one row per response and one column per question ID; bounded parser (100 MB cap).
- **Suppression:** primary suppression below `--min-cell` (default 10); complementary suppression of the next-smallest cell in the same question when exactly one cell is suppressed and the total is published.
- **Aggregates:** `{ survey_id, respondents, questions: { <id>: { cells: { <option>: n | "<10" } } } }`, RFC 8785 JSON.
- **Errors:** `raw_in_repo`, `export_parse`, `question_unknown`, `deletion_overdue`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `survey::agg`.

## Test fixtures

Synthetic CSV exports of invented responses and definitions under `fixtures/outr/comm/survey/agg/` (`ok`, `small-cell`), and `deletions.toml`; copied to a temp folder outside the repository by tests; CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-outreach`, add module `survey::agg` with `fid-outreach survey-agg --survey <file> --raw <file> --out <file> [--min-cell <n>] [--repo-root <dir>]` and `fid-outreach survey-retention check --deletions <file> --today <date> [--json]`. It reads survey definitions in the format of `survey::def` (questions with `reportable` options and optional `merge_with`).
>
> Aggregation: refuse (`raw_in_repo`, exit 2) a raw export inside `--repo-root` (default git top level); parse the CSV (one row per response, 100 MB cap); count respondents per reportable option of reportable questions; never output free text or response-level data. Cells below `--min-cell` (default 10) are merged per `merge_with` or shown as `<10`; when exactly one cell of a question is suppressed, also suppress the next-smallest so the total does not reveal it. Write RFC 8785 JSON.
>
> Retention: deletion records (`[[survey]] id, closed_on, planned_deletion, deleted_on?, deleted_by?`); `deletion_overdue <id>` when `--today` is after `planned_deletion` with no `deleted_on`.
>
> Tests with fixtures under `fixtures/outr/comm/survey/agg/` (copied outside the repo): normal aggregation with no free text, merge and suppression with complementary suppression, raw export inside the repo, deletion overdue, byte-identical output.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; no clock reads; BTreeMap rather than HashMap.

## Open questions

- The 10-respondent threshold is a proposal (carried from OUTR.COMM).

## Children

_Leaf: none._
