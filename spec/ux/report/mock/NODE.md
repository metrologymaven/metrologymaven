---
id: UX.REPORT.MOCK
title: Reference report mockups
level: 3
parent: UX.REPORT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.REPORT-006, REQ-UX.REPORT-007]
covers: []
depends_on: [UX.REPORT.STYLE, UX.REPORT.PAGE, UX.REPORT.CHART, UX.REPORT.BALLOON, UX.SYSTEM.REVIEW]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.REPORT.MOCK: Reference report mockups

## Purpose

Reference mockups that show the report visual language applied: an inspection summary, a characteristic table report, a ballooned drawing sheet, an SPC report and a colour-map snapshot page, on synthetic data, in A4 and Letter. Each carries a design-review record with the trade-dress distinctness check. They are the visual target RPT renders against.

## Scope

**In:** `report/mockups/<id>/` with PDF and PNG exports, the synthetic data used, and a design-review record per mockup.
**Out:** fixed AS9102 and PPAP form layouts (RPT.FORMS.LAYOUT under D-015); report templates (RPT.MODEL); usability testing of reports (`UX.RESEARCH.USAB`).

## Requirements

- **REQ-UX.REPORT.MOCK-001** The set shall contain at least the five mockups named in Purpose, each in A4 portrait and Letter portrait, and the colour-map page also in landscape.
- **REQ-UX.REPORT.MOCK-002** Each mockup shall use only synthetic or public data, only package styles and tokens, and shall include at least one example of each outcome where the report shows outcomes.
- **REQ-UX.REPORT.MOCK-003** Each mockup shall have a `design-review@1` record with a result for every trade-dress rule and an originality declaration, and shall pass `fid-design review-check`.
- **REQ-UX.REPORT.MOCK-004** Each mockup shall include a greyscale export that a reviewer uses to confirm REQ-UX.REPORT-002.

## Acceptance criteria

- **AC-1** Given the mockup folders, when `fid-design sources-check` and `fid-design review-check` run, then both exit 0.
- **AC-2** Given the greyscale exports, then the review record shows all four outcomes distinguishable (review item).
- **AC-3** Given the ballooned drawing sheet mockup, then the review record shows no balloon overlapping a leader line (review item for REQ-UX.REPORT-006).

## Design

- **Deliverable:** mockups drawn in the recommended design tool from package styles, exported to PDF and PNG (colour and greyscale), registered in `sources.json`.
- **Data:** synthetic part with invented characteristic IDs and values; drawing sheet drawn for the project.
- **Determinism:** D0 (checked files).
- **Crate:** none (package content); checked by `fid-design`.

## Test fixtures

The mockups themselves and their synthetic data; CC-BY-4.0.

## Build prompt

> Using the package's report style, page grids, chart and balloon styles, design five reference report mockups on synthetic data: inspection summary, characteristic table report, ballooned drawing sheet (an original drawing), SPC report, colour-map snapshot page; A4 and Letter portrait, colour map also landscape; show each outcome where outcomes appear. Export PDF and PNG in colour and greyscale into `report/mockups/<id>/`, register sources in `sources.json`, add originality declarations and a `design-review@1` record with every trade-dress rule result. Do not reproduce any competitor report layout. Run `fid-design sources-check` and `fid-design review-check` until both exit 0.

## Open questions

- D-015 (accepted, option C) keeps fixed form layouts original and in RPT.FORMS.LAYOUT; they are excluded here, but RPT.FORMS.LAYOUT may submit its layouts for a design review through UX.SYSTEM.REVIEW.

## Children

_Leaf: none._
