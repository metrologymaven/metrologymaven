---
id: RPT.DESIGN.PAGE
title: Page setup
level: 3
parent: RPT.DESIGN
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-report]
built_pr: 631
satisfies: [REQ-RPT.DESIGN-001, REQ-RPT.DESIGN-003]
covers: []
depends_on: [RPT.MODEL.DOC]
side_effect: W2
determinism: D0
cli: [fid report set-page-layout, fid report show-page-layout]
mcp: [report_set_page_layout, report_show_page_layout]
journal: [report.set_page_layout@1]
updated: 2026-11-16
---

# RPT.DESIGN.PAGE: Page setup

## Purpose

Sets the page format of a report or template: page size, orientation, margins and the heights of the header and footer areas, with an optional override per page (for example one landscape page in a portrait report). It defines the printable body area that block placement (`RPT.DESIGN.PLACE`) and the overflow check use. Lengths are stored as exact integers so layout is bit-identical everywhere.

## Scope

**In:** page-size presets (ISO A4, ISO A3, US Letter) and custom sizes; portrait and landscape; four margins; header and footer area heights; per-page override of size and orientation; the derived body rectangle; the set and show commands.

**Out:** block position and size (`RPT.DESIGN.PLACE`); fonts and colours (`RPT.DESIGN.STYLE`); header and footer content blocks (`RPT.MODEL.BLOCKS`); rendering (`RPT.OUTPUT`).

## Requirements

- **REQ-RPT.DESIGN.PAGE-001** The system shall offer the page-size presets ISO A4 (210 × 297 mm), ISO A3 (297 × 420 mm) and US Letter (215.9 × 279.4 mm), each in portrait and landscape.
- **REQ-RPT.DESIGN.PAGE-002** The system shall accept a custom page size with width and height each between 50 mm and 2000 mm.
- **REQ-RPT.DESIGN.PAGE-003** The system shall store every page length as an integer number of 0.01 mm units, converting inputs given in mm or inches exactly.
- **REQ-RPT.DESIGN.PAGE-004** If an input length is not an exact multiple of 0.01 mm, then the command shall fail with error `report.layout_invalid` naming the field and journal nothing.
- **REQ-RPT.DESIGN.PAGE-005** If margins plus header and footer heights would leave a body area narrower or shorter than 20 mm, then the command shall fail with error `report.layout_invalid` and journal nothing.
- **REQ-RPT.DESIGN.PAGE-006** When `report.set_page_layout@1` runs with a page ID, the system shall store an override for that page only; without a page ID it shall set the report default, and pages without an override shall use the default.
- **REQ-RPT.DESIGN.PAGE-007** Every page-layout change shall be one journaled W2 command, and replaying the journal shall reproduce the same layout.
- **REQ-RPT.DESIGN.PAGE-008** `report.show_page_layout@1` shall return, for each page, the effective page size, orientation, margins and body rectangle in 0.01 mm units.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/one_page_report`, when `fid report set-page-layout --report <id> --size A4 --orientation landscape --margins 15mm,15mm,15mm,15mm --header 12mm --footer 10mm --json` runs, then `show-page-layout` reports page 29700 × 21000 units and a body rectangle of 26700 × 15800 units at origin (1500, 2700).
- **AC-2** Given the same fixture, when `--size custom --width 8.5in --height 11in` runs, then the stored size is exactly 21590 × 27940 units.
- **AC-3** (error path) When `--margins 10.005mm,10mm,10mm,10mm` runs, then it fails with `report.layout_invalid` naming `margin_top` and the journal length is unchanged.
- **AC-4** (error path) When an A4 portrait layout has margins of 100 mm on left and right, then it fails with `report.layout_invalid` (body narrower than 20 mm).
- **AC-5** Given fixture `fixtures/rpt/design/three_page_report`, when page 2 gets a landscape override, then `show-page-layout` gives portrait for pages 1 and 3 and landscape for page 2.
- **AC-6** (replay) Given the journal from AC-5, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:**
  - `report.set_page_layout@1(report: ReportId, page: Option<PageId>, size: PagePreset | Custom{width: Length, height: Length}, orientation: Portrait|Landscape, margins: String (one string `top,right,bottom,left`; the CLI appends array flags one at a time, so a four-element array cannot be passed), header: Length, footer: Length) -> PageLayout` (W2)
  - `report.show_page_layout@1(report: ReportId) -> Vec<EffectivePageLayout>` (R0)
- **Data structures:** `Length` is an `i64` count of 0.01 mm; `PageLayout { size_w, size_h, orientation, margins, header, footer }`; the report stores `default_layout` and `overrides: BTreeMap<PageId, PageLayout>`.
- **Algorithm:** parse decimal length strings with a unit suffix (`mm` or `in`) using integer arithmetic only (1 in = 2540 units exactly); landscape swaps width and height; body = page − margins, with the header area at the top and the footer area at the bottom inside the margins.
- **Errors:** `report.layout_invalid { field, reason }`, `report.not_found`, and the crate-wide `report.bad_params` for badly shaped parameters (for example a `margins` string that is not four comma-separated lengths).
- **Defaults and omitted parameters:** a report's default page layout is A4 portrait with 20 mm margins and no header or footer. Parameters left out of a `report.set_page_layout@1` call keep the value already in effect; only `size` is required.
- **Determinism:** D0; integer arithmetic only, no floating point.
- **Performance budget:** trivial (< 1 ms per command) [unverified until measured].
- **Crate:** `fid-report` (module `layout::page`).

## Test fixtures

- `fixtures/rpt/design/one_page_report` and `fixtures/rpt/design/three_page_report`: reports built with the `RPT.MODEL.DOC` commands by a test builder; authored in-project, CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `layout::page`, implement page setup for reports.
>
> **Commands:** `report.set_page_layout@1` (W2, journaled, one event) and `report.show_page_layout@1` (R0), registered in the command registry with `--json` output; CLI `fid report set-page-layout` and `fid report show-page-layout`; MCP names are derived by the registry.
>
> **Model:** lengths are `i64` counts of 0.01 mm. Presets: A4 21000 × 29700, A3 29700 × 42000, Letter 21590 × 27940 (portrait; landscape swaps). Custom sizes 5000 to 200000 units per side. Parse inputs like `15mm`, `0.5in` with integer arithmetic only (no floats); reject anything not an exact multiple of 0.01 mm with `report.layout_invalid { field }`. Reject layouts whose body area is under 2000 units in either direction. A report has a default layout and optional per-page overrides (`BTreeMap<PageId, PageLayout>`; no std HashMap). The body rectangle is page minus margins, minus the header area at the top and the footer area at the bottom.
>
> **Acceptance tests:** AC-1 to AC-6 above (fixtures `fixtures/rpt/design/one_page_report`, `three_page_report`), including exact replay giving `IDENTICAL`.
>
> **Constraints:** no panic, no `unsafe`, no `unwrap`/`expect` (CONSTITUTION §8); failed commands journal nothing; layout commands must not change any bound value, decision or formatter choice. Do not implement block placement, styles or rendering. Spec: `spec/rpt/design/page/NODE.md`.

## Open questions

- Should page layout live on the template only, or also on each report? Assumed both: a report copies its template's layout and may override it.
- **For `RPT.MODEL.TMPL`:** a template shall carry a page layout (this node no longer states that obligation, because a template is not defined here). `RPT.MODEL.TMPL` should add the requirement and say how a report copies it.

## Change log

- 2026-11-25 (maintainer decision, recording what #631 built): added the crate-wide `report.bad_params` error; `margins` is passed as one string `top,right,bottom,left` (the CLI appends array flags one at a time), replacing `[Length; 4]`; the default layout is A4 portrait, 20 mm margins, no header or footer; omitted parameters keep the value in effect and only `size` is required; the obligation that a template carries a page layout moved to an open question for `RPT.MODEL.TMPL`. No REQ ID was renumbered.

## Children

_Leaf: none._
