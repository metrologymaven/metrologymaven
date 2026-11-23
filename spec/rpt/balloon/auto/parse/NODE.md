---
id: RPT.BALLOON.AUTO.PARSE
title: Deterministic dimension and GD&T callout parser
level: 4
parent: RPT.BALLOON.AUTO
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.AUTO-002]
covers: []
depends_on: [RPT.BALLOON.AUTO.TEXT, GDT.CHAR.MODEL.TYPES, KRN.NUM.KDEC]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-report]
updated: 2026-11-22
---

# RPT.BALLOON.AUTO.PARSE: Deterministic dimension and GD&T callout parser

## Purpose

Groups nearby PDF text runs (`TextRun`) into spatial callout clusters and deterministically parses engineering drawing requirements—linear, diameter, radius, angle, symmetric/bilateral/limit tolerances, basic dimensions, count prefixes (`4X`), thread/chamfer notes, and GD&T feature control frames—preserving exact authored decimal strings without binary floating-point rounding and keeping full bounding-box provenance back to the sheet.

## Scope

**In:**
- Pure, zero-I/O spatial clustering of `TextRun`s into multi-line callouts and compartmentalised feature control frames (handling stacked `+0.018 / -0.000` tolerances and CAD symbol-font codepoints mapped cleanly to semantic GD&T symbol kinds)
- Deterministic grammar parser for:
  - Linear, diameter (`Ø`), spherical diameter (`SØ`), radius (`R`, `SR`), angle (`°`), chamfer (`C` / `×45°`), and repeat counts (`4X`)
  - Symmetric (`±0.015`), bilateral (`+0.018 / -0.000`), and limit (`11.985 - 12.015`) tolerances
  - Basic dimensions (`[25.00]`) and reference dimensions (`(10.0)`)
  - Feature control frames (`⌖`, `⏥`, `○`, `⌭`, `⟂`, `∥`, `∠`, `⌓`, `⌒`, `↗`, `⌰`, `◎`, `⌯`, material modifiers `Ⓜ`/`Ⓛ`, primary/secondary/tertiary datum labels `A|B|C`)
- Title-block unit and default-tolerance-block detection (hinted as untrusted metadata requiring human confirmation)
- Exact authored decimal preservation (`KRN.NUM.KDEC` string validation, never `f64` round-trip)
- Qualitative confidence word (`high`, `medium`, `low`) with structured reason codes

**Out** (and which node owns it instead):
- PDF decoding (`RPT.BALLOON.AUTO.TEXT`)
- Draft-branch proposal creation and OCR candidate ingestion (`RPT.BALLOON.AUTO.PROPOSE`)
- Characteristic creation and GDT rule evaluation (`RPT.BALLOON.CHAR`, `GDT`)

## Requirements

- **REQ-RPT.BALLOON.AUTO.PARSE-001** The parser shall deterministically cluster spatially adjacent text runs and parse linear, diameter, radius, angle, tolerance (±, bilateral, limit) and GD&T feature control frame callouts into typed `ParsedCallout` records carrying exact authored decimal strings.
- **REQ-RPT.BALLOON.AUTO.PARSE-002** Every `ParsedCallout` shall retain the merged integer 0.01 mm `SheetRect`, the constituent `TextRun` IDs, the raw `UntrustedText` source string and a qualitative confidence word (`high`, `medium`, `low`) with reasons.
- **REQ-RPT.BALLOON.AUTO.PARSE-003** Authored numeric strings (e.g., `"12.000"`, `"+0.018"`, `"0.000"`) shall be preserved verbatim with their trailing zeros and never converted through binary floating point during parsing.
- **REQ-RPT.BALLOON.AUTO.PARSE-004** Title-block or general-note text that is not a valid requirement callout shall be excluded from characteristic candidates (or tagged `note_only`) rather than misparsed as a dimension.
- **REQ-RPT.BALLOON.AUTO.PARSE-005** Given identical `DrawingTextMap` input, the parser shall produce byte-identical output across platforms (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/auto/callouts_corpus.json` (100 synthetic engineering callouts covering all supported dimension, tolerance and GD&T FCF forms), when parsed, then precision and recall on exact authored nominal/upper/lower/type fields are at least 95 % and every decimal string preserves exact trailing zeros.
- **AC-2** Given stacked bilateral tolerance runs (`"12.000"`, `"+0.018"`, `"-0.000"` at adjacent sheet coordinates), then they merge into one `ParsedCallout` with nominal `"12.000"`, upper `"+0.018"`, lower `"-0.000"` and a single union `SheetRect`.
- **AC-3** Given an ambiguous or partially occluded callout, then it emits `confidence: low` with a typed reason code (e.g., `missing_unit`, `ambiguous_tolerance`) and never fabricates a tolerance.
- **AC-4** Given title-block metadata (`"DRAWN BY"`, `"SCALE 1:1"`, `"REV C"`), then none is emitted as a characteristic candidate.

## Design

- **Registry command(s):** none (pure library called by `RPT.BALLOON.AUTO.PROPOSE`).
- **Data structures:** `ParsedCallout { kind, repeat_count: u32, nominal: Option<DecimalString>, upper: Option<DecimalString>, lower: Option<DecimalString>, unit_hint: Option<Unit>, datum_refs: Vec<UntrustedText>, modifiers: Vec<Modifier>, raw_text: UntrustedText, bbox: SheetRect, confidence: ConfidenceWord, reasons: Vec<ReasonCode> }`.
- **Algorithm:** deterministic spatial union-find over `SheetRect`s within font-height-scaled proximity thresholds; rule-based symbol normalisation (Unicode + common CAD vector-font glyphs); recursive-descent callout grammar over tokens; deterministic sort by reading order (zone / top-to-bottom, left-to-right).
- **Errors:** typed parse diagnostics attached per candidate; never panics on arbitrary Unicode/malformed strings.
- **Determinism:** D0 (pure integer geometry and string grammar, `BTreeMap` only).
- **Performance budget:** 500 text runs clustered and parsed in < 10 ms [unverified].
- **Crate:** `fid-report` (module `balloon::auto_parse`).

## Test fixtures

`fixtures/rpt/balloon/auto/callouts_corpus.json`: 100 project-authored synthetic callouts and title-block distractors (CC-BY-4.0).

## Build prompt

> In crate `fid-report`, add pure module `balloon::auto_parse`: cluster spatially adjacent `TextRun`s (integer 0.01 mm `SheetRect` proximity scaled by font height, handling stacked bilateral tolerances and FCF compartments) and parse engineering callouts (linear, diameter, radius, angle, repeat counts `NX`, symmetric `±`, bilateral `+a/-b`, limit `lo-hi`, basic/reference dims, and GD&T FCFs with modifiers and datum refs) into `ParsedCallout` records. Preserve all decimal strings verbatim (never binary float), retain merged `SheetRect` and raw `UntrustedText`, exclude title-block noise, and emit a qualitative confidence word (`high`/`medium`/`low`) with reason codes. Pure D0, `BTreeMap` only, no I/O. Tests AC-1..AC-4 with `callouts_corpus.json`. Rules: CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`), C6.

## Open questions

- None blocking.

## Children

_Leaf: none._
