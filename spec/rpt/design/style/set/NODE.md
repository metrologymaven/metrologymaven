---
id: RPT.DESIGN.STYLE.SET
title: Style-set model, default style set and style commands
level: 4
parent: RPT.DESIGN.STYLE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.STYLE-001, REQ-RPT.DESIGN.STYLE-002, REQ-RPT.DESIGN.STYLE-003, REQ-RPT.DESIGN.STYLE-005]
covers: []
depends_on: [RPT.MODEL.TMPL, RPT.DESIGN.STYLE.FONT]
uses_artifacts: [UX.REPORT, UX.TOKENS]
side_effect: [W2, W2, W2, R0]
determinism: D0
cli: [fid report set-style, fid report save-style, fid report apply-style, fid report show-style]
mcp: [report_set_style, report_save_style, report_apply_style, report_show_style]
journal: [report.set_style@1, report.save_style@1, report.apply_style@1, report.show_style@1]
updated: 2026-11-17
---

# RPT.DESIGN.STYLE.SET: Style-set model, default style set and style commands

## Purpose

A style set fixes how a report looks: text roles with font family and size, colours, table styles, outcome presentation and a logo. This leaf defines the `StyleSet` model, the commands that edit it, save it with a template version and apply it to a report, and the built-in default style set. The default is not designed here: it is adopted from the `UX.REPORT` default report style set, whose values resolve through `UX.TOKENS`, so RPT carries no literal design values of its own.

## Scope

**In:**
- `StyleSet { id, version, name, text_roles, colours, table_styles, outcome_styles, logo: Option<BlobRef> }`, all colours as token references or sRGB values, all sizes in 0.01 pt integers
- `report.set_style@1` (edit fields of the report's working style set), `report.save_style@1` (store it with a template version), `report.apply_style@1` (apply a saved style set to a report), `report.show_style@1` (R0, effective style)
- Import of the `UX.REPORT` default style set at build time as the immutable built-in `default` style set, with its source digest
- Outcome styles that always pair a colour role with a text or symbol for each outcome

**Out** (and which node owns it instead):
- Font references, bundled fonts and `report.font_missing` (`RPT.DESIGN.STYLE.FONT`)
- Per-block overrides (`RPT.DESIGN.STYLE.OVR`)
- The design values of the default style set (`UX.REPORT`, `UX.TOKENS`)
- Template storage and versioning (`RPT.MODEL.TMPL`)

## Requirements

- **REQ-RPT.DESIGN.STYLE.SET-001** When `report.save_style@1` runs, the system shall store the style set with the given template version, and every report generated from that template version shall use it unless the report applies another style set.
- **REQ-RPT.DESIGN.STYLE.SET-002** Each set, save and apply shall be one journaled W2 command, and replay shall reproduce the same effective style.
- **REQ-RPT.DESIGN.STYLE.SET-003** Style commands shall change only style data and never a block payload, binding, decision or formatter choice.
- **REQ-RPT.DESIGN.STYLE.SET-004** The built-in `default` style set shall be generated from the `UX.REPORT` default report style set and record that artifact's version and digest; RPT source code shall contain no literal colour or font-size values for it.
- **REQ-RPT.DESIGN.STYLE.SET-005** Every outcome style shall carry both a colour role and a non-empty text or symbol for each of CONFORMS, NONCONFORMS, INDETERMINATE and NOT_DECIDABLE, and if an edit would remove the text or symbol, then the command shall fail with error `report.style_invalid`.
- **REQ-RPT.DESIGN.STYLE.SET-006** If a font size is below 6 pt or above 72 pt, or a colour is not a token reference or a 6-digit sRGB value, then the command shall fail with error `report.style_invalid` naming the field.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/one_page_report` from template T v1, when `fid report set-style --report R --role body --size 9.5pt --json` and `fid report save-style --template T --json` run, then template T v2 stores the style set and a new report from T v2 shows body size 950 in `show-style`.
- **AC-2** Given a report with no explicit style, `show-style` returns the `default` set with the `UX.REPORT` artifact version and digest.
- **AC-3** (error path) When an edit sets the NONCONFORMS symbol to empty, then it fails with `report.style_invalid` and journals nothing; a size of 5pt also fails.
- **AC-4** Given the payload and binding digest before AC-1, then it is unchanged after AC-1.
- **AC-5** A source lint run in the test suite finds no hex colour or point-size literal in the default style module.
- **AC-6** (replay) Given the journal after AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `report.set_style@1(report, set: Vec<StyleFieldPatch>) -> StyleSet` (W2); `report.save_style@1(report, template: TemplateId, name: Untrusted<Text>) -> TemplateRef` (W2, through the template save path); `report.apply_style@1(report, style: StyleRef) -> StyleSet` (W2); `report.show_style@1(report) -> EffectiveStyle` (R0). All `--json`.
- **Data structures:** `StyleFieldPatch` typed field paths (no free-form JSON patch); sizes `u32` in 0.01 pt; colours `TokenRef | Srgb(u8, u8, u8)`; font references from `RPT.DESIGN.STYLE.FONT`.
- **Build step:** a build script reads the published `UX.REPORT` style-set file (schema version pinned) and generates the `default` style set; if the artifact is not yet published, a placeholder neutral set marked `provisional` is generated and `show-style` reports it.
- **Errors:** `report.style_invalid { field }`, `report.not_found`, font errors from FONT.
- **Determinism:** D0; integers only.
- **Performance budget:** under 2 ms per command [unverified until measured].
- **Crate:** `fid-report` (module `style::set`).

## Test fixtures

- `fixtures/rpt/design/one_page_report` (shared), a template T v1 built by a test builder, and a synthetic `UX.REPORT` style-set file for the build-step test; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `style::set`, implement the `StyleSet` model (text roles, sizes in 0.01 pt integers, colours as token references or sRGB, table styles, outcome styles, logo blob) and `report.set_style@1`, `report.save_style@1`, `report.apply_style@1` (W2, one journal event each) and `report.show_style@1` (R0), all `--json`; CLI `fid report set-style`, `save-style`, `apply-style`, `show-style`; MCP names derived. Saving stores the style set with a template version through the template module; reports from that version use it unless another is applied. Outcome styles must keep a colour role plus a non-empty text or symbol for all four outcomes; sizes 6–72 pt (`report.style_invalid { field }`). Generate the built-in `default` set at build time from the UX report style-set file (record its version and digest; no literal colours or sizes in RPT source; provisional placeholder if the file is absent). Never change payloads, bindings, decisions or formatter choices. Tests AC-1 to AC-6. CONSTITUTION §8 applies. Spec: `spec/rpt/design/style/set/NODE.md`.

## Open questions

- **Cross-domain:** `UX.REPORT` (open) must publish its default report style set as a versioned file with a schema; until then the default is a provisional placeholder.

## Children

_Leaf: none._
