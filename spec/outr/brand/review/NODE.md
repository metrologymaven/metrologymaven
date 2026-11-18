---
id: OUTR.BRAND.REVIEW
title: Human trade-dress review gate
level: 3
parent: OUTR.BRAND
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 106
satisfies: [REQ-OUTR.BRAND-008]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BRAND.REVIEW: Human trade-dress review gate

## Purpose

The human check that no outreach release imitates an incumbent's trade dress: a short review checklist, a review record per outreach release, and a gate that refuses publication when the record is missing, incomplete or signed off by the release's own author. Automation cannot judge look and feel, so this node makes sure a person does, and that the fact is recorded. It is a project process with a small checker, not a product command.

## Scope

**In:** the checklist file; the review-record format; the `review-gate` check run before a site, docs or launch-material publication.
**Out:** the rules the reviewer applies (`OUTR.BRAND.GUIDE` trade-dress distinctness list); automated wording checks (`OUTR.BRAND.LINT`); publication pipelines themselves (`OUTR.SITE.GEN`, `OUTR.DOCS.FRAME`, `OUTR.LAUNCH`).

## Requirements

- **REQ-OUTR.BRAND.REVIEW-001** The repository shall contain a trade-dress review checklist with at least these items: colours, icon style, layout, screenshots, typography, and copied text, each answered pass or fail with an optional note.
- **REQ-OUTR.BRAND.REVIEW-002** Each outreach release shall have a review record file naming the release identifier, the reviewer, the date, and an answer for every checklist item.
- **REQ-OUTR.BRAND.REVIEW-003** If the review record for a release is missing, lacks an answer for any checklist item, or has any item answered fail, then the review gate shall exit non-zero and name the release and the item.
- **REQ-OUTR.BRAND.REVIEW-004** If the reviewer named in the record is the same person as the release author given to the gate, then the review gate shall exit non-zero with error `self_review`.
- **REQ-OUTR.BRAND.REVIEW-005** The review gate shall accept only reviewers listed in the repository's maintainer or reviewer roster file.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/review/ok` (a checklist, a roster with `alice` and `bob`, and a record for release `site-2027.01` by `bob` with all six items pass), when `fid-outreach review-gate --record records/site-2027.01.toml --author alice --roster roster.txt --checklist checklist.toml` runs, then it exits 0.
- **AC-2** Given fixture `fixtures/outr/review/missing-item` (the record has no answer for screenshots), when the gate runs, then it exits 1 and names `screenshots`.
- **AC-3** Given fixture `fixtures/outr/review/fail-item` (layout answered fail), when the gate runs, then it exits 1 and names `layout`.
- **AC-4** Given fixture `fixtures/outr/review/self` (reviewer `alice`, author `alice`), when the gate runs, then it exits 1 with error `self_review`.
- **AC-5** Given fixture `fixtures/outr/review/not-on-roster` (reviewer `mallory`), when the gate runs, then it exits 1 with error `unknown_reviewer`.
- **AC-6** (error path) Given a missing record file, when the gate runs, then it exits 2 and names the path.

## Design

- **Command:** `fid-outreach review-gate --record <file> --author <id> --roster <file> --checklist <file> [--json]`. Project tooling; nearest class R0.
- **Data structures:** checklist TOML (`version`, `items = [{ id, question }]`); record TOML (`release`, `reviewer`, `date`, `answers = { <item id> = { result = "pass"|"fail", note } }`); roster as one handle per line.
- **Algorithm:** parse the three files with bounded readers; check reviewer is on the roster; check reviewer differs from author (case-insensitive); check every checklist item has an answer and none is fail; report findings sorted by item ID.
- **Errors:** `RecordMissing`, `ParseError(path)`, `SelfReview`, `UnknownReviewer`, `MissingAnswer(item)`, `FailedItem(item)`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `review_gate`; the checklist file lives in the outreach repositories under `brand/`.

## Test fixtures

Synthetic TOML and text fixtures under `fixtures/outr/review/` (`ok`, `missing-item`, `fail-item`, `self`, `not-on-roster`), hand-written, CC-BY-4.0. Handles are fictional.

## Build prompt

> Build the `review-gate` subcommand of the Rust crate `fid-outreach`: `fid-outreach review-gate --record <file> --author <id> --roster <file> --checklist <file> [--json]`. Also write the checklist file `brand/trade-dress-checklist.toml`.
>
> Purpose: before a website, docs or launch-material release is published, a human other than its author must confirm it does not imitate any competitor's trade dress. The checklist has items `colours`, `icon_style`, `layout`, `screenshots`, `typography`, `copied_text`, each with a question in your own words. A review record (TOML) has `release`, `reviewer`, `date` and `answers.<item> = { result = "pass" | "fail", note = "..." }`. The roster is a text file with one reviewer handle per line.
>
> Behaviour: exit 0 when the reviewer is on the roster, differs from `--author` (case-insensitive), and every checklist item has a `pass` answer. Exit 1 with `unknown_reviewer`, `self_review`, `missing_answer:<item>` or `failed_item:<item>` otherwise (report all findings, sorted). Exit 2 if a file is missing or does not parse.
>
> Acceptance tests with fixtures under `fixtures/outr/review/`: `ok` exits 0; `missing-item` exits 1 naming `screenshots`; `fail-item` exits 1 naming `layout`; `self` exits 1 with `self_review`; `not-on-roster` exits 1 with `unknown_reviewer`; a missing record exits 2.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded file reads; no network; deterministic, sorted output; allowlisted dependency licences only.

## Open questions

- Review records are not authenticated: the gate trusts the `reviewer` named in the committed record. It should eventually verify that the commit or pull request adding the record was approved by that reviewer (forge review data, signed commits), so that nobody can name another person as reviewer. Until then the pull-request review that adds a record is the control.
- Which publications count as an "outreach release" (each site deploy, or only tagged site releases) is for maintainers to fix in the site and launch pipelines; the gate works per record either way.

## Children

_Leaf: none._
