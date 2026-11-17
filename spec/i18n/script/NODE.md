---
id: I18N.SCRIPT
title: Scripts, fonts, IME composition and right-to-left readiness
level: 2
parent: I18N
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-I18N-006]
covers: []
depends_on: [VIZ.SHELL.THEME, VIZ.SHELL.DOCK, RPT.OUTPUT.PDF, ECO.RULES.SPDX]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# I18N.SCRIPT: Scripts, fonts, IME composition and right-to-left readiness

## Purpose

Makes Chinese, Japanese and Korean text display correctly and lets users type it, and keeps the UI ready for right-to-left scripts. Owns the bundled font set and per-locale fallback chains (Han glyph variants differ between Japanese, Simplified Chinese, Traditional Chinese and Korean), IME composition in every text input, bidirectional text handling and layout mirroring readiness. These concerns sit on the UI toolkit (D-005, provisional) and the PDF renderer, so they are one subsystem with one set of script fixtures.

## Scope

**In:**
- Bundled fonts covering Latin, Greek, Cyrillic, CJK (SC, TC, JP, KR glyph variants) and the GD&T symbol set, under licences on the SPDX allowlist (e.g. OFL-1.1) [unverified: final font choice]
- Per-locale font fallback chains, used by both UI and PDF so a report looks like its preview
- IME composition: preedit display, candidate window placed at the caret, commit and cancel, in every text field
- Text input for CJK in names, comments, tags and free-text report blocks
- Bidirectional text per the Unicode Bidirectional Algorithm (UAX #9) for mixed-direction strings; numbers, units and IDs isolated left-to-right
- RTL readiness: layout mirroring switch tested with a pseudo-RTL locale; no RTL language ships until a decision adds one
- Line breaking for CJK (UAX #14) in UI labels and report text

**Out** (and which node owns it instead):
- Text shaping and font embedding in PDFs (`RPT.OUTPUT.PDF`; this node supplies fonts and fallback chains)
- Themes, scale and the UI lookup hook (`VIZ.SHELL.THEME`)
- Numeric input parsing and full-width digit normalisation (`I18N.LOCALE`)
- Message text (`I18N.CATALOG`)

## Requirements

- **REQ-I18N.SCRIPT-001** The product shall bundle fonts that render every character in every shipped language's catalog and glossary, and a CI check shall fail on any catalog character without a glyph in that language's fallback chain.
- **REQ-I18N.SCRIPT-002** When the active language is Japanese, Simplified Chinese, Traditional Chinese or Korean, Han characters shall render from that language's glyph variant font.
- **REQ-I18N.SCRIPT-003** Every bundled font shall carry an SPDX licence on the allowlist, checked by `ECO.RULES.SPDX`.
- **REQ-I18N.SCRIPT-004** While an IME composition is in progress in a text field, the field shall display the preedit text and place the candidate window within 2 lines of the caret, and shall not treat composition keystrokes as shortcuts.
- **REQ-I18N.SCRIPT-005** When a composition is committed, the field shall contain exactly the committed text, verified with scripted IME sessions for Japanese, Chinese (Pinyin) and Korean on each tier-1 OS [unverified: automation available per OS].
- **REQ-I18N.SCRIPT-006** Mixed-direction strings shall be laid out per UAX #9, and numbers, units, element IDs and characteristic IDs shall be rendered as left-to-right isolates.
- **REQ-I18N.SCRIPT-007** When the pseudo-RTL locale is active, every panel shall mirror its layout with no clipped or overlapping text, checked by golden images.
- **REQ-I18N.SCRIPT-008** The same text in the same font chain shall break into the same lines in the UI preview and the PDF output.

## Interfaces

- **Offers:** font bundle and fallback-chain data, IME and bidi behaviour contract for text widgets, script fixtures (CJK, mixed bidi, pseudo-RTL).
- **Consumes:** UI toolkit text input and rendering (`VIZ.SHELL.THEME`, `VIZ.SHELL.DOCK`); PDF text shaping (`RPT.OUTPUT.PDF`); licence allowlist (`ECO.RULES.SPDX`).

## Decomposition hints

FONTS (bundle, licences, coverage check), FALLBACK (per-locale chains shared with PDF), IME (composition contract and scripted tests), BIDI (UAX #9, isolates, pseudo-RTL mirroring), BREAK (line breaking).

## Open questions

- D-005 (egui + winit + wgpu) is provisional and its spikes do not test IME composition or CJK font fallback; IME support in that stack is reported as incomplete on some platforms [unverified]. An IME spike should be added before D-005 is confirmed.
- CJK font size: a full CJK set adds tens of megabytes to the installer [unverified]; per-language optional font packs may be needed (ECO.REL).

## Children

_None yet: status `open`._
