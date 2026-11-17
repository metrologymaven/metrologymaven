---
id: OUTR.DOCS.BOOK.CHECK
title: User-guide structure, example and availability check
level: 4
parent: OUTR.DOCS.BOOK
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.BOOK-001, REQ-OUTR.DOCS.BOOK-002, REQ-OUTR.DOCS.BOOK-003, REQ-OUTR.DOCS.BOOK-004, REQ-OUTR.DOCS.BOOK-005]
covers: []
depends_on: [OUTR.SITE.CLAIM, OUTR.CONTENT.TUT.CHECK, OUTR.BRAND.LINT]
uses_artifacts: [ECO.APPS.LIST, ECO.PARITY.GEN, ECO.REL.PKG, ECO.DATA.EX]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.BOOK.CHECK: User-guide structure, example and availability check

## Purpose

The build gate for the user guide of one release: every app in the app list has a chapter, the three required concept chapters exist, every runnable example prints what the chapter shows when run with the release's own binary, every capability a chapter claims is live in the release's parity report (or is labelled as not yet available), and every chapter passes the claim lint. It composes checks that already exist in the outreach crate.

## Scope

**In:** the `book-check` subcommand; chapter front-matter fields `app` and `topic`; the planned-capability tag and its fixed label; reuse of the step runner, capability check and claim lint.
**Out:** chapter text (`OUTR.DOCS.BOOK.CONCEPT`, `OUTR.DOCS.BOOK.REPORT`, `OUTR.DOCS.BOOK.INSPECT`, `OUTR.DOCS.BOOK.PROBE`, `OUTR.DOCS.BOOK.NETWORK`); the step runner and normalisation (`OUTR.CONTENT.TUT.CHECK`, `OUTR.CONTENT.REPRO.RUN`); the capability-tag check (`OUTR.SITE.CLAIM`); lint rules (`OUTR.BRAND.LINT`).

## Requirements

- **REQ-OUTR.DOCS.BOOK.CHECK-001** If an app ID in the app list has no chapter whose front matter has `app:` equal to it, then the check shall report `app_without_chapter` naming the app.
- **REQ-OUTR.DOCS.BOOK.CHECK-002** If no chapter has `topic: journals-replay`, `topic: units` or `topic: verification`, then the check shall report `concept_chapter_missing` naming each missing topic.
- **REQ-OUTR.DOCS.BOOK.CHECK-003** When run with `--run`, the check shall execute every `sh step` block of the guide with the release's `fid` binary on a scratch copy of the chapter's example project and report `output_mismatch` when the normalised output differs from the following `text expected` block.
- **REQ-OUTR.DOCS.BOOK.CHECK-004** The check shall run the capability-tag check on every chapter against the release's parity report, and if a `<!-- cap-planned: <ID> -->` tag is not followed in the same paragraph by the fixed label "Not yet available in this release.", then it shall report `planned_label_missing`.
- **REQ-OUTR.DOCS.BOOK.CHECK-005** The check shall run the claim lint on every chapter and report its errors with rule IDs.
- **REQ-OUTR.DOCS.BOOK.CHECK-006** The check shall exit 0 with no findings, 1 with any finding, and 2 when an input cannot be read.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/docs/book/ok` (app list `report`, `inspect`; one chapter per app; three concept chapters; one runnable example; a parity report with the tagged capability live), when `fid-outreach book-check --src book --apps apps.toml --parity parity.json --rules claim-rules.toml --fid fid-stub --examples ex --run` runs, then it exits 0.
- **AC-2** Given fixture `book/no-inspect`, when the check runs, then it reports `app_without_chapter` for `inspect`.
- **AC-3** Given fixture `book/no-units`, when the check runs, then it reports `concept_chapter_missing` for `units`.
- **AC-4** Given fixture `book/not-live` (a `<!-- cap: GOM.ALN.04 -->` tag on a row not live), when the check runs, then it reports the capability check's `not_live` finding; with the tag changed to `cap-planned` and the fixed label added, it reports nothing.
- **AC-5** Given fixture `book/mismatch`, when the check runs with `--run`, then it reports `output_mismatch` with the block line.
- **AC-6** Given fixture `book/lint` ("fully compliant"), when the check runs, then it reports claim-lint rule CL001.

## Design

- **Command:** `fid-outreach book-check --src <dir> --apps <apps.toml> --parity <parity.json> --rules <claim-rules.toml> [--fid <binary> --examples <dir> --run] [--json]`. Project tooling; nearest class R0.
- **Chapter front matter:** `title`, optional `app` (app ID), optional `topic` (`journals-replay`, `units`, `verification`, or free), `example_project` when the chapter has steps.
- **Reuse:** step runner and `normalise` from the `tut_check` and `content_replay` modules; `cap_check` from OUTR.SITE.CLAIM; `claim_lint` from OUTR.BRAND.LINT.
- **Errors:** findings `app_without_chapter`, `concept_chapter_missing`, `output_mismatch`, `step_failed`, `planned_label_missing`, plus reused findings; fatal `InputUnreadable`.
- **Determinism:** D1 (example outputs come from the product binary per platform).
- **Crate:** `fid-outreach`, module `book_check`.

## Test fixtures

Synthetic chapter sets under `fixtures/outr/docs/book/` with a stub `fid` script, small app lists and parity files in the OUTR.SITE.CLAIM input format. CC-BY-4.0.

## Build prompt

> Build `fid-outreach book-check --src <dir> --apps <apps.toml> --parity <parity.json> --rules <claim-rules.toml> [--fid <binary> --examples <dir> --run] [--json]` in the Rust outreach tooling crate `fid-outreach`, reusing its `tut_check` step runner, `normalise`, `cap_check` and `claim_lint` modules. Rules: each `[[app]] id` in `apps.toml` needs a chapter with front matter `app: <id>` (`app_without_chapter`); chapters with `topic: journals-replay`, `units` and `verification` must exist (`concept_chapter_missing`); with `--run`, execute `sh step` blocks on a scratch copy of the chapter's `example_project` and compare normalised output with the next `text expected` block (`output_mismatch`, `step_failed`); run `cap_check` on all chapters; a `<!-- cap-planned: <ID> -->` tag needs the exact label "Not yet available in this release." in the same paragraph (`planned_label_missing`); run the claim lint. Exit 0/1/2; RFC 8785 JSON report sorted by file, line, code. Never modify inputs.
>
> Acceptance tests under `fixtures/outr/docs/book/` as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- REQ-OUTR.DOCS.BOOK-001 says "the ECO.APPS manifest"; the app list file of ECO.APPS.LIST is the source of app IDs here.

## Children

_Leaf: none._
