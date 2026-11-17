---
id: OUTR.CONTENT.TUT.INSPECT
title: Inspect app tutorial: scan-to-CAD inspection
level: 4
parent: OUTR.CONTENT.TUT
kind: task
status: leaf
phase: P1b
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

# OUTR.CONTENT.TUT.INSPECT: Inspect app tutorial: scan-to-CAD inspection

## Purpose

The first tutorial for the Inspect app: import a CAD model with PMI and a scan, pre-align and best-fit, run a surface comparison, take sections, evaluate GD&T characteristics and produce a report. It follows the scan FAI acceptance job of launch-1, so a reader reproduces the result the release was tested on.

## Scope

**In:** the tutorial text; its example project (a public CAD-with-PMI and scan pair) and journal; expected outputs of each CLI step; screenshots from our own capture commands.
**Out:** migration guides for incumbent users (`OUTR.CONTENT.MIG.GOM`, `OUTR.CONTENT.MIG.PW`); videos (`OUTR.CONTENT.VIDEO`); the Inspect user-guide chapter (`OUTR.DOCS.BOOK.INSPECT`).

## Requirements

- **REQ-OUTR.CONTENT.TUT.INSPECT-001** The tutorial shall declare `app: inspect` and follow the steps of acceptance job JOB.ACC.01 (scan FAI from CAD and PMI) in the seed journeys inventory, in our own words.
- **REQ-OUTR.CONTENT.TUT.INSPECT-002** The tutorial shall have sections for import, alignment, surface comparison, sections, GD&T evaluation and reporting.
- **REQ-OUTR.CONTENT.TUT.INSPECT-003** Every command step shall be a `sh step` block with its `text expected` block, and `tut-check --run` shall pass on the targeted release.
- **REQ-OUTR.CONTENT.TUT.INSPECT-004** The tutorial shall state `duration_min` and its prerequisites (including the hardware profile needed for the scan size), and every image shall be produced by our `capture.*` commands.

## Acceptance criteria

- **AC-1** Given the tutorial and its example project, when `fid-outreach tut-check ... --run` runs on the targeted release, then it reports no finding for this tutorial.
- **AC-2** Given the tutorial, when `fid-outreach content-check` runs on it, then it exits 0.
- **AC-3** Given the tutorial's journal, when `content-replay` runs on the targeted release, then its outcome is `IDENTICAL`.
- **AC-4** Given the tutorial, when a test lists its headings, then it finds the six sections of REQ-002.

## Design

- **Registry command(s):** none; a tutorial checked by `content-check`, `tut-check` and `content-replay`.
- **Files:** `content/tutorials/inspect/tutorial.md` with the OUTR.CONTENT.PUB manifest plus `app: inspect`, `topics`, `duration_min` and `prerequisites` (release, example project, hardware profile `minimum` per D-014).
- **Workflow source:** JOB.ACC.01 in `seeds/journeys.md`; commands from the targeted release's reference pages.
- **Determinism:** D1: expected outputs come from the release binary on one platform and are compared after normalisation.
- **Crate:** none (content); checks run from `fid-outreach`.

## Test fixtures

The tutorial's example project and journal, contributed through ECO's example-project process (ECO.DATA.EX) under CC-BY-4.0 or the source dataset's licence; the release binaries of the targeted release.

## Build prompt

> Write `content/tutorials/inspect/tutorial.md`, a Markdown tutorial for the Inspect app that follows acceptance job JOB.ACC.01 (scan FAI from CAD and PMI) in your own words. Front matter: the content manifest plus `app: inspect`, `topics`, `duration_min`, `prerequisites` (state the hardware profile the scan needs). Sections: import; alignment; surface comparison; sections; GD&T evaluation; reporting. CLI steps are `sh step` blocks with `text expected` blocks taken from the targeted release; GUI steps have screenshots made with `capture.take_screenshot`. Use a CAD and scan pair that is public or CC-licensed. Done when `fid-outreach content-check` and `fid-outreach tut-check --run` exit 0 on the tutorial, and its journal replays `IDENTICAL` on the targeted release. Write in your own words; use only screenshots made with our `capture.*` commands; no "certified", "compliant" or comparative claims.

## Open questions

- Inspect is a P1b app; the tutorial is written against its first alpha.

## Children

_Leaf: none._
