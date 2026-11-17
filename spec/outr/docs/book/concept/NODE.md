---
id: OUTR.DOCS.BOOK.CONCEPT
title: Concept chapters
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
depends_on: [OUTR.DOCS.BOOK.CHECK]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, TRUST.VERIFY.CLI]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.BOOK.CONCEPT: Concept chapters

## Purpose

The cross-cutting chapters of the user guide that every app chapter builds on: the project model (part, revision, plan, run), the command registry and its surfaces (GUI, CLI, MCP, scripts), journals and replay, units and how decimals are kept, uncertainty status and decisions, and result verification with `fid-verify`. They explain how the product keeps every value of record deterministic and checkable.

## Scope

**In:** six chapters under `book/concepts/`; the apps overview chapter and stub chapters for apps without their own chapter; runnable examples on a small P0 example project; the chapter list with owners.
**Out:** app chapters (`OUTR.DOCS.BOOK.REPORT`, `OUTR.DOCS.BOOK.INSPECT`, `OUTR.DOCS.BOOK.PROBE`, `OUTR.DOCS.BOOK.NETWORK`); tutorials (`OUTR.CONTENT.TUT`); the contributor guide (`OUTR.DOCS.CONTRIB`); the check (`OUTR.DOCS.BOOK.CHECK`).

## Requirements

- **REQ-OUTR.DOCS.BOOK.CONCEPT-001** The guide shall have chapters with `topic: journals-replay`, `topic: units` and `topic: verification`, plus chapters on the project model, the registry and its surfaces, and uncertainty status and decisions.
- **REQ-OUTR.DOCS.BOOK.CONCEPT-002** The journals-and-replay chapter shall explain the four replay outcomes and the replay modes named in CONSTITUTION §4, with one runnable example of an exact replay.
- **REQ-OUTR.DOCS.BOOK.CONCEPT-003** The units chapter shall explain that the kernel works in SI metres and that each authored decimal keeps its value and unit with one journaled rounding per conversion, with one runnable example.
- **REQ-OUTR.DOCS.BOOK.CONCEPT-004** The verification chapter shall show `fid-verify` on a result package with one runnable example, and state plainly which report sections the documented release marks `not_available`.
- **REQ-OUTR.DOCS.BOOK.CONCEPT-005** The uncertainty chapter shall explain uncertainty status UEC-0 to UEC-3 and the decision outcomes, and state that UEC-0 means not evaluated and is never filled with an invented value.
- **REQ-OUTR.DOCS.BOOK.CONCEPT-006** The guide shall have an apps overview chapter that lists every app in the app list with its status, and, for each app whose own chapter is not yet written, a stub chapter with `app: <id>` that says the app is not yet available in this release and links the roadmap.

## Acceptance criteria

- **AC-1** Given the six chapters and the release binaries, when `fid-outreach book-check --src book --apps apps.toml --parity parity.json --rules claim-rules.toml --fid <fid> --examples ex --run` runs, then it reports no finding for these chapters.
- **AC-2** Given the chapters, when a test reads their front matter, then the topics `journals-replay`, `units` and `verification` are present.
- **AC-3** Given the journals-and-replay chapter, when a test lists its headings, then it finds one section per replay outcome and per replay mode.
- **AC-4** Given an app list with `probe` and `network` and no chapters for them other than the stubs, when `book-check` runs, then it reports no `app_without_chapter` finding.

## Design

- **Files:** `book/concepts/{project-model,registry,journals-replay,units,uncertainty,verification}.md`, front matter `title`, `topic`, `example_project` where steps exist.
- **Sources of truth:** CONSTITUTION §1 (C1, C3, C4), §4 and §5 for meaning; the release's reference pages for command names.
- **Determinism:** D1: example outputs come from the release binary on one platform and are compared after normalisation.
- **Crate:** none (text); checked by `fid-outreach book-check`.

## Test fixtures

The example projects named in the chapters' front matter (ECO example-project set, CC-BY-4.0 or the source dataset's licence) and the release binaries of the documented release.

## Build prompt

> Write six Markdown chapters for a user guide under `book/concepts/`: project model (part, revision, plan, run), registry and command surfaces, journals and replay (`topic: journals-replay`; the outcomes IDENTICAL, EQUIVALENT, DIVERGENT, NOT_REPLAYABLE and the replay modes exact, re-targeted, verifier, dry-run, cross-version), units (`topic: units`; SI metres inside the kernel, authored decimals keep value and unit, one journaled rounding per conversion), uncertainty status and decisions (UEC-0 to UEC-3, UEC-0 means not evaluated and is never invented; outcomes CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE), and verification (`topic: verification`; `fid-verify` on a result package; say plainly which sections the release reports as `not_available`). Also write `book/apps/index.md`, an overview listing every app in `apps.toml` with its status, and for each app without its own chapter a stub `book/apps/<id>.md` with `app: <id>` stating that the app is not yet available in this release and linking the roadmap page. Runnable examples are `sh step` blocks with one `fid` or `fid-verify` command and a following `text expected` block captured from the documented release on a small example project. Done when `fid-outreach book-check --run` exits 0 for the documented release. Write in your own words, cite standards by designator, edition and clause only (never quote them), use no "certified" or "compliant" wording, and tag every capability claim with `<!-- cap: <seed ID> -->` or, if it is not live, `<!-- cap-planned: <seed ID> -->` plus the label "Not yet available in this release."

## Open questions

- The capability tags need seed IDs; concept chapters rarely claim a seed capability, so most have none.

## Children

_Leaf: none._
