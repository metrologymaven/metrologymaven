---
id: UX.REPORT
title: Report visual language
level: 2
parent: UX
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UX-001, REQ-UX-002, REQ-UX-006]
covers: []
depends_on: [UX.TOKENS, UX.ICONS, UX.COMP]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.REPORT: Report visual language

## Purpose

How printed and exported reports look: the default style set, page grids for common paper sizes, print typography, table styles, chart styles, balloon style, outcome presentation that survives greyscale printing, and colour-map snapshots that use the viewer's palettes. RPT implements reports and the designer; this node decides the visual language and hands RPT a default style set. It is separate because print has its own constraints (greyscale, paper sizes, point sizes) that the screen tokens do not cover.

## Scope

**In:**
- Default report style set expressed in tokens: fonts per text role, sizes, table styles, outcome colours
- Page grids and margins for A4 and US Letter, portrait and landscape
- Chart styles: control chart, run chart, histogram, capability chart, deviation colour-map snapshot and its legend
- Balloon style: shape, numbering, leader lines
- Greyscale-safe outcome presentation
- Reference report mockups on synthetic data

**Out** (and which node owns it instead):
- Report model, templates and blocks (`RPT.MODEL`)
- Style-set data model and per-block overrides (`RPT.DESIGN.STYLE`)
- Fixed AS9102 and PPAP form layouts (`RPT.FORMS.LAYOUT`, D-015)
- PDF rendering and font embedding (`RPT.OUTPUT.PDF`)
- Computing chart values, limits and statistics (`RPT.STATS`)
- Palette values (`UX.TOKENS`)

## Requirements

- **REQ-UX.REPORT-001** The design system shall define a default report style set in tokens that RPT can adopt as its starter style set without literal values.
- **REQ-UX.REPORT-002** Every outcome (CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE) shall remain distinguishable in a greyscale print through text and glyph, without relying on colour.
- **REQ-UX.REPORT-003** Deviation colour-map snapshots and legends in reports shall use palette IDs from the `UX.TOKENS` catalogue, the same as the viewer.
- **REQ-UX.REPORT-004** Each chart style shall define axes, labels, limit-line and specification-line styles, and point markers, and shall state that every plotted value and limit comes from RPT, not from the chart.
- **REQ-UX.REPORT-005** The page grids shall cover A4 and US Letter in portrait and landscape, and body text shall be no smaller than 8 pt and table text no smaller than 7 pt [unverified].
- **REQ-UX.REPORT-006** The balloon style shall keep balloon numbers legible at 8 pt on a printed drawing sheet and shall not overlap leader lines with other balloons in the reference mockups.
- **REQ-UX.REPORT-007** Each reference report mockup shall carry a design-review record including the trade-dress distinctness check, and no report style shall reproduce an incumbent's report layout.

## Interfaces

- **Offers:** the default report style set, page grids, chart and balloon styles, reference report mockups.
- **Consumes:** `UX.TOKENS`, `UX.ICONS` (GD&T symbols, status glyphs), `UX.COMP` (table and legend anatomy). Interface notes (no `depends_on`): `RPT.DESIGN.STYLE` should adopt the default style set; `RPT.BALLOON` should follow the balloon style; `RPT.STATS` charts should follow the chart styles; `I18N` supplies localised font needs.

## Decomposition hints

STYLE (default style set and print typography), PAGE (page grids), CHART (chart styles), BALLOON (balloon style), MOCK (reference report mockups).

## Split rationale

The dividing principle is **one print concern per child, with mockups last**: STYLE (the default style set RPT.DESIGN.STYLE adopts, print typography and greyscale-safe outcomes), PAGE (page grids), CHART (chart styles and colour-map snapshot legend), BALLOON (balloon style), MOCK (reference mockups that apply all of them and carry the trade-dress review). This follows the decomposition hints. Each style child is a token-referencing file plus a check in `fid-design`; MOCK is the visual proof.

Alternatives considered:
- **BALLOON inside STYLE.** Rejected: balloons carry their own clearance rules that RPT.BALLOON's placement must honour, and their own review evidence (no leader overlap).
- **Designing the fixed AS9102 and PPAP forms here.** Rejected: D-015 (option C) keeps original form layouts in RPT.FORMS.LAYOUT; they can still pass the UX design review.

## Open questions

- Boundary with RPT (agreed): UX.REPORT supplies RPT.DESIGN.STYLE's default styles as `report/style.json`; RPT owns the StyleSet model and commands and should declare `uses_artifacts: [UX.REPORT.STYLE]`. RPT.BALLOON and RPT.STATS should likewise read UX.REPORT.BALLOON and UX.REPORT.CHART (not editable from here).
- Form layouts under D-015: kept out of this node (see Split rationale).
- Font licences for print: shared with UX.TOKENS.TYPE and RPT.DESIGN.STYLE-006; the allowlist is ECO.RULES' to publish [unverified].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [UX.REPORT.STYLE](style/NODE.md) | Default report style set and print typography (leaf) | 001 002 005 | none |
| [UX.REPORT.PAGE](page/NODE.md) | Report page grids and margins (leaf) | 005 | none |
| [UX.REPORT.CHART](chart/NODE.md) | Report chart styles and colour-map snapshot legend (leaf) | 003 004 | none |
| [UX.REPORT.BALLOON](balloon/NODE.md) | Balloon style (leaf) | 006 | none |
| [UX.REPORT.MOCK](mock/NODE.md) | Reference report mockups (leaf) | 006 007 | none |
