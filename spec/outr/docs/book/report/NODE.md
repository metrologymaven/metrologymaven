---
id: OUTR.DOCS.BOOK.REPORT
title: Report app chapter
level: 4
parent: OUTR.DOCS.BOOK
kind: task
status: leaf
phase: P1a
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

# OUTR.DOCS.BOOK.REPORT: Report app chapter

## Purpose

The user-guide chapter for the Report app: characteristics and ballooning, transcription and manual readings, decisions, report layouts with the general designer, export, and basic SPC, written for quality engineers who build first-article and production reports.

## Scope

**In:** the chapter (one file or a short set of files with `app: report`); runnable CLI examples on the Report example project; capability tags for every workflow claim.
**Out:** the Report tutorial (`OUTR.CONTENT.TUT.REPORT`); FAI form layouts as such (D-015, owned by RPT); the concept chapters (`OUTR.DOCS.BOOK.CONCEPT`).

## Requirements

- **REQ-OUTR.DOCS.BOOK.REPORT-001** The Report chapter shall carry `app: report` and sections on characteristics, transcription and manual readings, decisions, report layout, export, and SPC.
- **REQ-OUTR.DOCS.BOOK.REPORT-002** Every workflow claim in the chapter shall carry a capability tag, and claims not live in the documented release shall use the planned tag and its label.
- **REQ-OUTR.DOCS.BOOK.REPORT-003** The chapter shall describe FAI forms only as layouts made with the general report designer, following D-015, and reproduce no standard's form.
- **REQ-OUTR.DOCS.BOOK.REPORT-004** Every runnable example shall pass `book-check --run` on the documented release.

## Acceptance criteria

- **AC-1** Given the chapter, when `fid-outreach book-check ... --run` runs on the documented release, then it reports no finding for the chapter.
- **AC-2** Given the chapter, when a test lists its headings, then it finds the six sections of REQ-001.
- **AC-3** Given the chapter and the release's parity report, when `cap-check` runs on it, then it reports nothing.

## Design

- **Files:** `book/apps/report.md` (or `book/apps/report/*.md`), front matter `app: report`, `example_project`.
- **Workflow source:** JOB.ACC.02 and JOB.ACC.06 in `seeds/journeys.md`, the Report app's manifest and the release's reference pages.
- **Determinism:** D1: example outputs come from the release binary on one platform and are compared after normalisation.
- **Crate:** none (text); checked by `fid-outreach book-check`.

## Test fixtures

The example projects named in the chapters' front matter (ECO example-project set, CC-BY-4.0 or the source dataset's licence) and the release binaries of the documented release.

## Build prompt

> Write the Report app chapter of the user guide, `book/apps/report.md`, front matter `app: report` and `example_project`. Sections: characteristics and ballooning; transcription and manual readings; decisions; report layout with the general designer (FAI forms are layouts users build, per D-015; never reproduce a standard's form); export to an approved local root; SPC basics. CLI examples are `sh step` blocks with `text expected` blocks from the documented release. Done when `fid-outreach book-check --run` exits 0 for the documented release. Write in your own words, cite standards by designator, edition and clause only (never quote them), use no "certified" or "compliant" wording, and tag every capability claim with `<!-- cap: <seed ID> -->` or, if it is not live, `<!-- cap-planned: <seed ID> -->` plus the label "Not yet available in this release."

## Open questions

- Report is planned for P1a (ECO.APPS.LIST proposal [unverified]).

## Children

_Leaf: none._
