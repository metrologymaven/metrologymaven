---
id: OUTR.CONTENT.TUT.REPORT
title: Report app tutorial: a transcription inspection report
level: 4
parent: OUTR.CONTENT.TUT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.TUT-001, REQ-OUTR.CONTENT.TUT-003, REQ-OUTR.CONTENT.TUT-004, REQ-OUTR.CONTENT.TUT-005]
covers: []
depends_on: [OUTR.CONTENT.TUT.CHECK, OUTR.CONTENT.PUB]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, ECO.APPS.LIST]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.TUT.REPORT: Report app tutorial: a transcription inspection report

## Purpose

The first tutorial for the Report app: take a ballooned drawing and an existing measurement report, create the characteristics, transcribe the measured values, review each decision, and produce and export a first-article report. It follows the transcription acceptance job of launch-1 so that a reader reproduces the same result the release was tested on.

## Scope

**In:** the tutorial text; its example project (a public or CC-licensed ballooned drawing and a measurement report) and journal; expected outputs of each CLI step; screenshots from our own capture commands.
**Out:** other Report workflows such as SPC (later tutorials or the user guide `OUTR.DOCS.BOOK.REPORT`); FAI form layouts (D-015, owned by RPT); check tooling (`OUTR.CONTENT.TUT.CHECK`).

## Requirements

- **REQ-OUTR.CONTENT.TUT.REPORT-001** The tutorial shall declare `app: report` and follow the steps of acceptance job JOB.ACC.02 (transcription of an existing measurement report against a ballooned drawing) in the seed journeys inventory, in our own words.
- **REQ-OUTR.CONTENT.TUT.REPORT-002** The tutorial shall have sections for creating characteristics from the ballooned drawing, transcribing measured values from the existing report, reviewing decisions, and exporting the report to an approved local export root.
- **REQ-OUTR.CONTENT.TUT.REPORT-003** Every command step shall be a `sh step` block with its `text expected` block, and `tut-check --run` shall pass on the targeted release.
- **REQ-OUTR.CONTENT.TUT.REPORT-004** The tutorial shall state `duration_min` and its prerequisites, and every image in it shall be produced by our `capture.*` commands.

## Acceptance criteria

- **AC-1** Given the tutorial and its example project, when `fid-outreach tut-check ... --run` runs on the targeted release, then it reports no finding for this tutorial.
- **AC-2** Given the tutorial, when `fid-outreach content-check` runs on it, then it exits 0.
- **AC-3** Given the tutorial's journal, when `content-replay` runs on the targeted release, then its outcome is `IDENTICAL`.
- **AC-4** Given the tutorial, when a test lists its headings, then it finds the four sections of REQ-002.

## Design

- **Registry command(s):** none; a tutorial checked by `content-check`, `tut-check` and `content-replay`.
- **Files:** `content/tutorials/report/tutorial.md` with the OUTR.CONTENT.PUB manifest plus `app: report`, `topics`, `duration_min` and `prerequisites` (release, example project, hardware profile `minimum` per D-014).
- **Workflow source:** JOB.ACC.02 in `seeds/journeys.md`; commands from the targeted release's reference pages.
- **Determinism:** D1: expected outputs come from the release binary on one platform and are compared after normalisation.
- **Crate:** none (content); checks run from `fid-outreach`.

## Test fixtures

The tutorial's example project and journal, contributed through ECO's example-project process (ECO.DATA.EX) under CC-BY-4.0 or the source dataset's licence; the release binaries of the targeted release.

## Build prompt

> Write `content/tutorials/report/tutorial.md`, a Markdown tutorial for the Report app that follows acceptance job JOB.ACC.02 (transcription of an existing measurement report against a ballooned drawing) in your own words. Front matter: the content manifest plus `app: report`, `topics`, `duration_min`, `prerequisites`. Sections: create characteristics from the ballooned drawing; transcribe measured values from the existing report; review decisions; export the report to an approved local export root. CLI steps are `sh step` blocks with `text expected` blocks taken from the targeted release; GUI steps have screenshots made with `capture.take_screenshot`. Use a drawing and report that are public or CC-licensed. The form layout follows D-015 (the general report designer, not a copied form). Done when `fid-outreach content-check` and `fid-outreach tut-check --run` exit 0 on the tutorial, and its journal replays `IDENTICAL` on the targeted release. Write in your own words; use only screenshots made with our `capture.*` commands; no "certified", "compliant" or comparative claims.

## Open questions

- The Report app is planned for P1a (ECO.APPS.LIST proposal [unverified]); the tutorial is written against its first alpha.

## Children

_Leaf: none._
