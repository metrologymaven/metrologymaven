---
id: RPT.DESIGN.STYLE
title: Style sets
level: 3
parent: RPT.DESIGN
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN-001, REQ-RPT.DESIGN-005, REQ-RPT.DESIGN-007]
covers: []
depends_on: [RPT.MODEL.TMPL]
cli: [fid report set-style, fid report save-style, fid report apply-style, fid report show-style]
mcp: [report_set_style, report_save_style, report_apply_style, report_show_style]
journal: [report.set_style@1, report.save_style@1, report.apply_style@1]
updated: 2026-11-16
---

# RPT.DESIGN.STYLE: Style sets

## Purpose

Named, versioned style sets that fix how a report looks: fonts, sizes, colours, table styles (borders, header shading, colour of conformance outcomes) and the logo. A style set is saved with a template and applied to every report generated from it, so a company's look is consistent. Styles never change content.

## Scope

**In:**
- Style-set model: font families and sizes per text role, colours, table styles, outcome colours, logo image by digest
- Saving a style set with a template and applying it to reports
- Per-block style overrides
- Font references by family name and file digest (the font files themselves are embedded by `RPT.OUTPUT.PDF`)

**Out** (and which node owns it instead):
- Page geometry (`RPT.DESIGN.PAGE`) and block frames (`RPT.DESIGN.PLACE`)
- Font embedding and text shaping (`RPT.OUTPUT.PDF`)
- Template storage (`RPT.MODEL.TMPL`)

## Requirements

- **REQ-RPT.DESIGN.STYLE-001** A style set shall be stored with a template version, and every report generated from that template version shall use it unless the report overrides it.
- **REQ-RPT.DESIGN.STYLE-002** Each style-set change shall be one journaled W2 command, and replay shall reproduce the same style.
- **REQ-RPT.DESIGN.STYLE-003** Style commands shall not change any bound value, decision, binding or formatter choice.
- **REQ-RPT.DESIGN.STYLE-004** A style set shall reference fonts by family name and file digest, and if a referenced font file is missing, then the system shall report error `report.font_missing` naming the family.
- **REQ-RPT.DESIGN.STYLE-005** The default style set shall show outcome colours together with a text or symbol for each outcome, so that conformance is never conveyed by colour alone.
- **REQ-RPT.DESIGN.STYLE-006** The project shall ship only fonts whose licences are on the D-001 allowlist [unverified which fonts qualify].

## Interfaces

- **Offers:** `report.set_style@1`, `report.save_style@1`, `report.apply_style@1` (W2), `report.show_style@1` (R0); type `StyleSet`.
- **Consumes:** templates (`RPT.MODEL.TMPL`); font files from settings or project storage.

## Decomposition hints

SET (style-set model and commands), FONT (font references and bundled font list), OVR (per-block overrides).

## Open questions

- Which open-licence font families to bundle as defaults (clean room, CONSTITUTION §8; licence per D-001).

## Children

_None yet: status `open`._
