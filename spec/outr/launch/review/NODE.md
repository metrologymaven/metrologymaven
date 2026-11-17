---
id: OUTR.LAUNCH.REVIEW
title: Post-launch reviews
level: 3
parent: OUTR.LAUNCH
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-008]
covers: []
depends_on: [OUTR.LAUNCH.PLAN.FMT, OUTR.BRAND.LINT]
uses_artifacts: []
side_effect: [R0, X1]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.REVIEW: Post-launch reviews

## Purpose

A short written review after each milestone announcement (what was announced, what went wrong, what changes in the next milestone's criteria), kept next to the launch plan, and a scheduled check that flags any announcement older than 30 days without its review. Review entries are also the recorded path through which an announced milestone's criteria may change (OUTR.LAUNCH.PLAN.FMT reads their `changes` list).

## Scope

**In:** the review template; review entries under `launch/reviews/`; the `review-due` subcommand (deadline, structure, issue links, claim lint); the scheduled CI job that keeps one tracking issue per overdue milestone.
**Out:** criteria and announcement dates (`OUTR.LAUNCH.PLAN.FMT`, `OUTR.LAUNCH.PLAN.DOC`; announcement dates are the `announced.date` entries of the criteria file); adoption numbers (`OUTR.COMM.REPORT`; a review may link them).

## Requirements

- **REQ-OUTR.LAUNCH.REVIEW-001** The repository shall contain `launch/reviews/TEMPLATE.md` with front matter `milestone`, `announced`, `changes` (list of criterion IDs) and the sections "What was announced", "Issues found" and "Changes proposed".
- **REQ-OUTR.LAUNCH.REVIEW-002** If a milestone in the criteria file has `announced.date` more than 30 days before the `--today` date and no review entry names that milestone, then `review-due` shall report `review_overdue` naming the milestone and the announcement date.
- **REQ-OUTR.LAUNCH.REVIEW-003** If a review entry lacks a template section or front-matter field, or its `announced` differs from the criteria file's `announced.date` for that milestone, then the check shall report an error naming the entry and the item.
- **REQ-OUTR.LAUNCH.REVIEW-004** If a review entry mentions an issue number (`#<n>`) that is not part of a Markdown link, then the check shall report `issue_unlinked` with line.
- **REQ-OUTR.LAUNCH.REVIEW-005** The check shall run the claim lint on every review entry and report its errors.
- **REQ-OUTR.LAUNCH.REVIEW-006** A scheduled CI job shall run `review-due` at least daily with the run date as `--today`, and for each `review_overdue` finding create or update one tracking issue labelled `launch-review` that names the milestone, closing it once the review exists.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/launch/review/ok` (criteria file with `report-alpha` announced 2027-01-10 and a complete review entry), when `fid-outreach review-due --criteria criteria.toml --reviews reviews --rules claim-rules.toml --today 2027-03-01` runs, then it exits 0.
- **AC-2** Given fixture `review/overdue` (announced 2027-01-10, no entry), when the check runs with `--today 2027-02-10`, then it reports `review_overdue`; with `--today 2027-02-09`, it reports nothing.
- **AC-3** Given fixture `review/no-section` (no "Issues found" section), when the check runs, then it reports `section_missing`.
- **AC-4** Given fixture `review/bare-issue` ("fixed in #42" as plain text), when the check runs, then it reports `issue_unlinked` with the line.
- **AC-5** Given fixture `review/lint` ("now fully compliant"), when the check runs, then it reports claim-lint rule CL001.

## Design

- **Commands:** `fid-outreach review-due --criteria <file> --reviews <dir> --rules <file> --today <YYYY-MM-DD> [--json]` (R0); the CI job's issue update through the repository's issue tooling (X1: posts to the hosted tracker).
- **Deadline:** overdue when `today - announced.date > 30` days (calendar days, proleptic Gregorian, no time zone).
- **Errors:** findings `review_overdue`, `section_missing`, `field_missing`, `announced_mismatch`, `issue_unlinked`, plus claim-lint findings; fatal `InputUnreadable`, `InputParse`, `BadDate`.
- **Determinism:** D0; the date is an input, never read from the clock.
- **Crate:** `fid-outreach`, module `launch_review` (reuses the `launch_plan` parser).

## Test fixtures

Synthetic criteria files and review entries under `fixtures/outr/launch/review/`. CC-BY-4.0.

## Build prompt

> (1) Write `launch/reviews/TEMPLATE.md`: front matter `milestone`, `announced` (YYYY-MM-DD), `changes` (list of criterion IDs whose change the review proposes); sections "What was announced", "Issues found" (every issue as a Markdown link), "Changes proposed".
>
> (2) Build `fid-outreach review-due --criteria <file> --reviews <dir> --rules <file> --today <YYYY-MM-DD> [--json]` in the Rust outreach tooling crate `fid-outreach`, reusing its `launch_plan` parser and `claim_lint`. Report `review_overdue` for each milestone whose `announced.date` is more than 30 calendar days before `--today` with no review entry naming it; `section_missing`, `field_missing`, `announced_mismatch` for malformed entries; `issue_unlinked` for `#<n>` outside a Markdown link; claim-lint findings. Never read the system clock. Exit 0/1/2; RFC 8785 JSON report.
>
> (3) A daily scheduled CI job that runs the check with the run date and creates, updates or closes one issue labelled `launch-review` per overdue milestone.
>
> Acceptance tests under `fixtures/outr/launch/review/` as listed in the spec (including the 30-day boundary).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; the tool itself opens no network connection; BTreeMap only; allowlisted dependency licences.

## Open questions

- None beyond the parent's.

## Children

_Leaf: none._
