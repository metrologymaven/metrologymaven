---
id: UX.REPORT
title: Report visual language
level: 2
parent: UX
kind: subsystem
status: open
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
updated: 2026-11-16
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

## Open questions

- Whether the original form layouts (if D-015 decides layouts must be original) are designed here or in RPT.FORMS.LAYOUT; this node currently excludes them.

## Children

_None yet: status `open`._
