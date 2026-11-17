---
id: OUTR.COMM.SURVEY.QSET
title: Survey definitions, privacy statement and announcements
level: 4
parent: OUTR.COMM.SURVEY
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM.SURVEY-001, REQ-OUTR.COMM.SURVEY-002, REQ-OUTR.COMM.SURVEY-005]
covers: []
depends_on: [OUTR.BRAND.LINT]
uses_artifacts: [OUTR.COMM.CHAN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.COMM.SURVEY.QSET: Survey definitions, privacy statement and announcements

## Purpose

Each survey as one reviewed definition file: its purpose, retention period, who may read raw responses, its questions and where it will be announced. A check makes sure the privacy statement comes before the first question, that no question asks for identifying data without being optional and justified, and that announcements go only to official channels and the website, never into the product.

## Scope

**In:** the survey definition format (`community/surveys/<id>/survey.toml`); the `survey-def check` command; the rendered introduction page with the privacy statement; the first question set (adoption and workflow questions).
**Out:** tool criteria (`OUTR.COMM.SURVEY.TOOL`); aggregation and deletion (`OUTR.COMM.SURVEY.AGG`); the channel inventory (`OUTR.COMM.CHAN`).

## Requirements

- **REQ-OUTR.COMM.SURVEY.QSET-001** A survey definition shall state `purpose`, `retention_days`, `raw_readers` (roles), `aggregate_publication` (where results appear) and at least one question, and the rendered survey shall show purpose, retention and readers before the first question.
- **REQ-OUTR.COMM.SURVEY.QSET-002** If a question's `kind` is `email`, `name`, `organisation` or free text flagged `may_identify`, then the check shall fail unless the question is optional and has a `why` note.
- **REQ-OUTR.COMM.SURVEY.QSET-003** Each entry in `announce_on` shall be either `website` or a channel ID present in the channel inventory, and if any entry names the product (`in_product`, `app`, or an unknown ID), then the check shall fail with `announce_target_invalid`.
- **REQ-OUTR.COMM.SURVEY.QSET-004** The rendered introduction and questions shall pass the outreach claim lint with zero errors.
- **REQ-OUTR.COMM.SURVEY.QSET-005** Each question shall declare `reportable` groupings (the answer options that may be published as cells), and free-text answers shall never be reportable.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/comm/survey/qset/ok/survey.toml` and a channel inventory with `forum`, when `fid-outreach survey-def check --survey survey.toml --channels channels.toml --json` runs, then it exits 0, and `survey-def render` writes a page whose first block states purpose, retention and readers.
- **AC-2** (error path) Given `email-required.toml` (a required email question), when the check runs, then it exits 1 with `identifying_question_required`.
- **AC-3** (error path) Given `in-product.toml` (`announce_on = ["website", "in_product"]`), when the check runs, then it exits 1 with `announce_target_invalid in_product`.
- **AC-4** (error path) Given `freetext-reportable.toml`, when the check runs, then it exits 1 with `free_text_reportable`.
- **AC-5** Given the committed first survey, when the check and the claim lint run, then both exit 0.

## Design

- **Commands:** `fid-outreach survey-def check --survey <file> --channels <file> [--json]` and `fid-outreach survey-def render --survey <file> --out <file>`. Project tooling; nearest class R0.
- **Definition:** `id`, `purpose`, `retention_days`, `raw_readers`, `aggregate_publication`, `announce_on`, `[[question]] id, text, kind (single | multi | scale | number | free_text | email | name | organisation), options?, required, may_identify?, why?, reportable`.
- **Errors:** `field_missing`, `identifying_question_required`, `announce_target_invalid`, `free_text_reportable`, `parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `survey::def`; definitions in the outreach repository.

## Test fixtures

Hand-written definitions and a small channel inventory under `fixtures/outr/comm/survey/qset/` (`ok`, `email-required`, `in-product`, `freetext-reportable`), CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-outreach`, add module `survey::def` with `fid-outreach survey-def check --survey <file> --channels <file> [--json]` and `fid-outreach survey-def render --survey <file> --out <file>`.
>
> Survey TOML: `id`, `purpose`, `retention_days`, `raw_readers`, `aggregate_publication`, `announce_on` (list of `website` or channel IDs from the channel inventory TOML `[[channel]] id`), and `[[question]]` with `id`, `text`, `kind` (single, multi, scale, number, free_text, email, name, organisation), `options`, `required`, `may_identify`, `why`, `reportable`. Findings: `field_missing`; `identifying_question_required` when an email, name, organisation or `may_identify` question is required or lacks `why`; `announce_target_invalid` for any target that is not `website` or a known channel; `free_text_reportable`. Render Markdown whose first block states purpose, retention and who reads raw responses, then the questions; run the existing `claim_lint` on it.
>
> Also write the first survey under `community/surveys/adoption-2027/survey.toml` (adoption, use case and workflow questions; no required identifying questions), in your own words.
>
> Tests with fixtures under `fixtures/outr/comm/survey/qset/` for each case above and the committed survey passing.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; never add any survey prompt to the product.

## Open questions

- Whether a survey must also be reviewed by a maintainer other than its author (as for channel changes, D-003) is assumed through normal pull-request review.

## Children

_Leaf: none._
