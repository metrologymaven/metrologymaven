---
id: OUTR.DOCS.QA
title: Docs quality gates and link checks
level: 3
parent: OUTR.DOCS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 445
satisfies: [REQ-OUTR.DOCS-008, REQ-OUTR.DOCS-009, REQ-OUTR.DOCS-010]
covers: []
depends_on: [OUTR.SITE.QA, OUTR.BRAND.LINT]
uses_artifacts: [OUTR.DOCS.FRAME]
side_effect: [R0, X1]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.QA: Docs quality gates and link checks

## Purpose

The checks every docs edition must pass before it is published: the reusable OUTR.SITE.QA gates (zero off-origin requests, accessibility baseline) run with a docs configuration, the OUTR.BRAND claim lint on hand-written sources, and an internal link and anchor check that fails the build; plus a scheduled external link check that keeps one tracking issue up to date. Only the configuration and the link checks are new; the gates themselves are reused.

## Scope

**In:** `docs-gates.toml` (the docs configuration of the SITE.QA gates); the `link-check` subcommand (internal and external modes); the claim-lint source list (hand-written sources only); the CI wiring for the build gate and the weekly external run.
**Out:** the gate implementations (`OUTR.SITE.QA`); the lint rules (`OUTR.BRAND.LINT`); search (`OUTR.DOCS.SEARCH`); the reference drift check (`OUTR.DOCS.REF`); building editions (`OUTR.DOCS.FRAME`).

## Requirements

- **REQ-OUTR.DOCS.QA-001** When an edition is built, the OUTR.SITE.QA off-origin crawl shall run with `docs-gates.toml` on every page of the edition and fail the docs build on any off-origin request.
- **REQ-OUTR.DOCS.QA-002** When an edition is built, the OUTR.SITE.QA accessibility gate shall run with `docs-gates.toml` on every page and fail the docs build on any violation at the VIZ baseline's required level.
- **REQ-OUTR.DOCS.QA-003** If any internal link or fragment in an edition does not resolve to a file or an element `id` of the same edition, then `link-check` shall exit 1 and name the source page, line and link.
- **REQ-OUTR.DOCS.QA-004** When `link-check --external` runs, it shall send one HEAD request (falling back to GET) to each distinct external URL of the latest edition with a 10 s timeout and at most 4 concurrent requests, and write a report and an issue body listing every URL that failed with its status or error and the pages that link it.
- **REQ-OUTR.DOCS.QA-005** A scheduled CI job shall run the external check at least weekly [unverified cadence] and create or update exactly one tracking issue labelled `docs-external-links` with the issue body, closing it when no link fails.
- **REQ-OUTR.DOCS.QA-006** When an edition is built, the OUTR.BRAND claim lint shall run on the edition's hand-written Markdown sources (book, contributor guide, registered guides) and not on generated reference pages, and fail the build on any error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/docs/qa/ok` (a built edition with valid internal links and fragments), when `fid-outreach link-check --edition ok --json` runs, then it exits 0.
- **AC-2** Given fixture `qa/broken` (one link to `book/ghost.html` and one to `book/units.html#nope`), when the check runs, then it exits 1 with two findings `link_target_missing` and `fragment_missing`, each with page and line.
- **AC-3** Given fixture `qa/external` and a local test server that answers 200 for one URL, 404 for one and never answers for a third, when `link-check --edition external --external --json` runs against it, then the report lists the 404 and the timeout with their linking pages, and `issue.md` contains both.
- **AC-4** Given fixture `qa/tracker` (a page with a third-party script), when the docs gates run with `docs-gates.toml`, then the off-origin gate fails naming the page and URL.
- **AC-5** Given fixture `qa/lint` (a book page saying "certified" and a reference page containing the same word inside a generated help text), when the docs gates run, then exactly one claim-lint error is reported, for the book page.
- **AC-6** (determinism) Given AC-2 run twice, then the reports are byte-identical.

## Design

- **Commands:** `fid-outreach link-check --edition <dir> [--json]` (R0) and `fid-outreach link-check --edition <dir> --external [--out <dir>] [--json]` (X1: contacts external hosts; it sends only the URL, no project data). The gates run as the OUTR.SITE.QA command with `--config docs-gates.toml` [command name fixed by OUTR.SITE.QA].
- **`docs-gates.toml`:** `origin` (docs portal origin), `root` (edition dir), `pages` (all `*.html` of the edition), `a11y_level` (from the VIZ baseline), `size_budget_kb` (docs pages use the same budget as the site unless set), `lint_sources` (globs of hand-written sources; reference pages excluded).
- **Internal check:** parse every HTML page with a bounded tokenizer; collect `href`/`src` that are relative; resolve against the page path; check the file exists inside the edition and, for fragments, that an element with that `id` exists.
- **External check:** distinct absolute `http(s)` URLs, sorted; requests with a fixed user agent naming the project tooling generically; results sorted by URL; `issue.md` is Markdown.
- **CI wiring:** the build job runs gates, lint and the internal check on every edition; a weekly scheduled job runs the external check on the latest edition and uses the repository's issue tooling to create, update or close the one issue labelled `docs-external-links`.
- **Errors:** findings `link_target_missing`, `fragment_missing`, `link_escapes_edition`, `external_failed`; fatal `EditionUnreadable`.
- **Determinism:** D0 for the internal check and gates; the external check is observed (class N) by nature and only reports.
- **Performance budget:** internal check of a 2,000-page edition in under 10 s [unverified until measured].
- **Crate:** `fid-outreach`, module `link_check`; configuration under `docs/qa/` in the docs repository.

## Test fixtures

Synthetic editions under `fixtures/outr/docs/qa/` (`ok`, `broken`, `external`, `tracker`, `lint`) and a local HTTP test server in the test harness for external cases. CC-BY-4.0.

## Build prompt

> In the Rust outreach tooling crate `fid-outreach`, build the `link-check` subcommand and the docs configuration of the existing static-site gates.
>
> `fid-outreach link-check --edition <dir> [--json]`: parse every `*.html` with a bounded tokenizer; for each relative `href`/`src`, resolve against the page, require the file inside the edition (`link_target_missing`, `link_escapes_edition`) and, for `#fragment`, an element with that `id` in the target (`fragment_missing`). Report page, line and link; exit 0/1/2; RFC 8785 JSON sorted by page, line.
>
> `--external [--out <dir>]`: for each distinct absolute `http(s)` URL (sorted), send HEAD (GET if HEAD is refused), 10 s timeout, at most 4 at a time, generic user agent; write `external.json` and `issue.md` listing failures with status or error and linking pages. This is the only mode that uses the network.
>
> Also write `docs/qa/docs-gates.toml` for the site gates (`origin`, `root`, `pages`, `a11y_level`, `size_budget_kb`, `lint_sources` with hand-written sources only), and CI wiring: build job runs gates, claim lint and internal check on every edition and fails on any error; a weekly scheduled job runs the external check and creates, updates or closes exactly one issue labelled `docs-external-links`.
>
> Acceptance tests under `fixtures/outr/docs/qa/` as listed in the spec (clean edition, broken link and fragment, external 404 and timeout against a local test server, third-party script caught by the gate, lint only on hand-written sources, byte-identical reruns).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; BTreeMap only; allowlisted dependency licences.

## Open questions

- The weekly external-link cadence is a proposal carried from OUTR.DOCS.
- OUTR.SITE.QA is still open; its gate command name and configuration keys may differ from `docs-gates.toml` above, in which case only the configuration file changes.
- **Deferred in PR #445: CI wiring.** The edition build gate (running internal `link-check`, the OUTR.SITE.QA off-origin and accessibility gates and the claim lint when an edition is built; REQ-001, REQ-002, REQ-003, REQ-006) and the weekly scheduled external-link job that maintains the single `docs-external-links` issue (REQ-005) are not wired into `.github/workflows/` yet. The commands and `docs/qa/docs-gates.toml` exist. The workflow comes in a follow-up once OUTR.DOCS.FRAME builds editions in CI and OUTR.SITE.QA.CRAWL lands. A scheduled job that writes issues needs a P1 human review.
- **Stand-in for OUTR.SITE.QA (PR #445).** OUTR.SITE.QA.CRAWL is not built, so AC-4 is tested with `link_check::check_off_origin`, a static off-origin scan of `src`/`href` resource URLs against `docs-gates.toml` `origin`. It is not the real crawl. AC-5 runs the real `fid-brand-lint claim-lint` binary with its shipped `claim-rules.toml`.
- **`--external` requires `curl`** on `PATH`. When it cannot be run, `link-check --external` exits 2 with `tool_unavailable` and writes no report.

## Children

_Leaf: none._
