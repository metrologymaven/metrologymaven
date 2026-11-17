---
id: OUTR.DOCS.BOOK.INSPECT
title: Inspect app chapter
level: 4
parent: OUTR.DOCS.BOOK
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS.BOOK-001, REQ-OUTR.DOCS.BOOK-003, REQ-OUTR.DOCS.BOOK-004, REQ-OUTR.DOCS.BOOK-005]
covers: []
depends_on: [OUTR.DOCS.BOOK.CHECK]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, ECO.PARITY.GEN]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.BOOK.INSPECT: Inspect app chapter

## Purpose

The user-guide chapter for the Inspect app: importing CAD with PMI and scans, alignment, surface comparison, sections and thickness, GD&T evaluation in the ASME and ISO dialects, and reports, written for inspection engineers who work from scans.

## Scope

**In:** the chapter (one file or a short set with `app: inspect`); runnable CLI examples on the Inspect example project; capability tags for every workflow claim.
**Out:** the Inspect tutorial (`OUTR.CONTENT.TUT.INSPECT`); migration guides (`OUTR.CONTENT.MIG`); the concept chapters (`OUTR.DOCS.BOOK.CONCEPT`).

## Requirements

- **REQ-OUTR.DOCS.BOOK.INSPECT-001** The Inspect chapter shall carry `app: inspect` and sections on import, alignment, surface comparison, sections and thickness, GD&T evaluation, and reporting.
- **REQ-OUTR.DOCS.BOOK.INSPECT-002** Every workflow claim in the chapter shall carry a capability tag, and claims not live in the documented release shall use the planned tag and its label.
- **REQ-OUTR.DOCS.BOOK.INSPECT-003** The GD&T section shall cite standards by designator, edition and clause only and name which dialect each evaluation follows.
- **REQ-OUTR.DOCS.BOOK.INSPECT-004** Every runnable example shall pass `book-check --run` on the documented release.

## Acceptance criteria

- **AC-1** Given the chapter, when `fid-outreach book-check ... --run` runs on the documented release, then it reports no finding for the chapter.
- **AC-2** Given the chapter, when a test lists its headings, then it finds the six sections of REQ-001.
- **AC-3** Given the chapter and the release's parity report, when `cap-check` runs on it, then it reports nothing.

## Design

- **Files:** `book/apps/inspect.md` (or `book/apps/inspect/*.md`), front matter `app: inspect`, `example_project`.
- **Workflow source:** JOB.ACC.01 and JOB.ACC.07 (sections and thickness only in launch-1) in `seeds/journeys.md`, the Inspect app's manifest and the release's reference pages.
- **Determinism:** D1: example outputs come from the release binary on one platform and are compared after normalisation.
- **Crate:** none (text); checked by `fid-outreach book-check`.

## Test fixtures

The example projects named in the chapters' front matter (ECO example-project set, CC-BY-4.0 or the source dataset's licence) and the release binaries of the documented release.

## Build prompt

> Write the Inspect app chapter of the user guide, `book/apps/inspect.md`, front matter `app: inspect` and `example_project`. Sections: import of CAD with PMI and scans; alignment; surface comparison; sections and thickness; GD&T evaluation (name the dialect, cite standards by designator, edition and clause only); reporting. CLI examples are `sh step` blocks with `text expected` blocks from the documented release. Gap and flush is not part of launch-1; tag it as planned if mentioned. Done when `fid-outreach book-check --run` exits 0 for the documented release. Write in your own words, cite standards by designator, edition and clause only (never quote them), use no "certified" or "compliant" wording, and tag every capability claim with `<!-- cap: <seed ID> -->` or, if it is not live, `<!-- cap-planned: <seed ID> -->` plus the label "Not yet available in this release."

## Open questions

- Inspect is a P1b app; the chapter is written against its first alpha.

## Children

_Leaf: none._
