---
id: RPT.OUTPUT.PDF.TEXT
title: Bundled fonts, text shaping, metrics and font subsetting
level: 4
parent: RPT.OUTPUT.PDF
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-report-pdf]
satisfies: [REQ-RPT.OUTPUT.PDF-001, REQ-RPT.OUTPUT.PDF-002]
covers: []
depends_on: [CORE.STO.CAS]
uses_artifacts: [I18N.SCRIPT, RPT.DESIGN.STYLE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.OUTPUT.PDF.TEXT: Bundled fonts, text shaping, metrics and font subsetting

## Purpose

Everything the PDF renderer needs to put text on a page reproducibly: a fixed, bundled font set (never system fonts), deterministic shaping of a string into positioned glyphs with integer metrics for layout, line breaking candidates, and subsetting of exactly the glyphs used into embeddable font programs. It is separate from layout and drawing because font handling decides whether two machines produce the same bytes.

## Scope

**In:** the bundled font set (a sans and a monospace family with regular and bold, plus CJK fonts read from the font files published by `I18N.SCRIPT` when present) and its manifest of font digests; font selection by style name (from a `RPT.DESIGN.STYLE` style set when present, else the default style); shaping to glyph IDs and advances in integer layout units (1 µm); UAX 14 line-break opportunities [unverified library support]; fallback across the font set for missing glyphs; deterministic subsetting with a stable subset tag derived from the glyph set.

**Out:** page breaking and box placement (`RPT.OUTPUT.PDF.LAYOUT`); drawing operators (`RPT.OUTPUT.PDF.DRAW`); PDF object serialisation (`RPT.OUTPUT.PDF.WRITE`); CJK input and UI fonts (`I18N.SCRIPT`).

## Requirements

- **REQ-RPT.OUTPUT.PDF.TEXT-001** The renderer shall use only fonts from the bundled font manifest (and font files listed in the I18N script-font manifest), each verified by SHA-256 before use, and shall never load a system font.
- **REQ-RPT.OUTPUT.PDF.TEXT-002** Shaping the same string with the same font and size shall give the same glyph IDs and integer advances on x86-64, ARM64 and wasm32.
- **REQ-RPT.OUTPUT.PDF.TEXT-003** When a character has no glyph in the selected font, the shaper shall try the manifest's fallback order, and if no font has it shall use the font's `.notdef` glyph and report warning `report.glyph_missing` with the code point.
- **REQ-RPT.OUTPUT.PDF.TEXT-004** For each font used in a document, the subsetter shall produce one embedded subset containing exactly the used glyphs plus `.notdef`, with a subset tag derived from the SHA-256 of the sorted glyph ID list, so the same glyph set gives byte-identical subset bytes.
- **REQ-RPT.OUTPUT.PDF.TEXT-005** Every font in the bundled set shall carry a licence on the D-001 SPDX allowlist (for example OFL-1.1 [unverified allowlist]) and be listed in `ATTRIBUTION.md`.
- **REQ-RPT.OUTPUT.PDF.TEXT-006** The shaper shall not alter the characters of a `ValueText` display string: shaping followed by reading the PDF text layer shall return the same string.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/output/text/strings.json` (Latin, digits with separators, German, Japanese, a missing-glyph code point), when shaped on x86-64, ARM64 and wasm32, then glyph IDs and advances are identical across targets and the missing glyph gives `report.glyph_missing`.
- **AC-2** Given a document using 120 distinct glyphs of the sans regular font, when subset twice, then the subset bytes are identical and contain exactly 121 glyphs.
- **AC-3** Given a font file whose digest does not match the manifest, when loaded, then it fails with `report.font_invalid` and is not used.
- **AC-4** Given the string `-0,050` (a German-locale value), when shaped and embedded with a ToUnicode map, then extracting text gives `-0,050` unchanged.
- **AC-5** Given the bundled manifest, when the licence check runs, then every font's SPDX ID is on the allowlist and appears in `ATTRIBUTION.md`.

## Design

- **Data structures:** `FontManifest { fonts: Vec<FontEntry { id, family, weight, style, sha256, spdx }>, fallback: Vec<FontId> }`; `ShapedRun { font: FontId, size_um: i64, glyphs: Vec<(GlyphId, advance_um: i64, cluster: u32)> }`; `Subset { font: FontId, tag: [u8; 6], bytes: Vec<u8>, to_unicode: CMap }`.
- **Algorithm:** fixed-point scaling of font units to µm with integer rounding half-even; shaping through a pure-Rust shaper with no system dependencies (candidate: rustybuzz, MIT [unverified]); subset with glyph renumbering in ascending original ID order.
- **Errors:** `report.font_invalid`, `report.glyph_missing` (warning).
- **Determinism:** D0; integer metrics, no platform font APIs.
- **Performance budget:** shape 100 000 characters in under 100 ms [unverified until measured].
- **Crate:** `fid-report-pdf` (module `text`).

## Test fixtures

`fixtures/rpt/output/text/strings.json` (in-project); bundled fonts under an OFL-type licence [unverified until the allowlist is set].

## Build prompt

> In crate `fid-report-pdf`, add module `text`: a bundled font manifest (fonts verified by SHA-256; never system fonts; CJK fonts from the I18N script-font manifest when present), deterministic shaping to glyph IDs and integer advances in µm (pure-Rust shaper; identical on x86-64, ARM64, wasm32), fallback order with `.notdef` and warning `report.glyph_missing`, and deterministic subsetting (exact used glyphs plus `.notdef`, subset tag from SHA-256 of the sorted glyph list, ToUnicode map). Never alter the characters of formatter output. Every font licence must be on the D-001 SPDX allowlist and listed in `ATTRIBUTION.md`. Acceptance: AC-1..AC-5 in `spec/rpt/output/pdf/text/NODE.md`. Constraints: CONSTITUTION §8 (bounded font parsing of hostile input, no panic, no `unsafe`), clean room.

## Open questions

- Which font families to bundle depends on the D-001 SPDX allowlist (`ECO.RULES`) and on CJK coverage owned by `I18N.SCRIPT` [unverified].

## Children

_Leaf: none._
