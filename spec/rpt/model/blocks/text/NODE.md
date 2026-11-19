---
id: RPT.MODEL.BLOCKS.TEXT
title: Text, image, header and footer blocks
level: 4
parent: RPT.MODEL.BLOCKS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL.BLOCKS-001, REQ-RPT.MODEL.BLOCKS-007, REQ-RPT.MODEL.BLOCKS-008, REQ-RPT.MODEL.BLOCKS-009, REQ-RPT.MODEL.BLOCKS-010, REQ-RPT.MODEL.BLOCKS-011, REQ-RPT.MODEL.BLOCKS-012]
covers: [PCD.RPT.03]
depends_on: [RPT.MODEL.DOC, RPT.MODEL.BIND.RESOLVE, RPT.MODEL.FMT, CORE.STO.CAS, IO.REG.ROOTS]
side_effect: [W2, W2]
determinism: D0
cli: [fid report set-text-block, fid report set-image-block]
mcp: [report_set_text_block, report_set_image_block]
journal: [report.set_text_block@1, report.set_image_block@1]
updated: 2026-11-19
---

# RPT.MODEL.BLOCKS.TEXT: Text, image, header and footer blocks

## Purpose

The simple block kinds: `text` (paragraphs with bound fields such as a characteristic's measured value), `image` (a picture stored by content digest), and `header` and `footer` (page number, page count, report ID and resolved-report digest). Bound fields are typed placeholders resolved only through `RPT.MODEL.FMT`; user text is never evaluated as an expression or instruction.

## Scope

**In:** payload schemas v1 for `text`, `image`, `header`, `footer`; the bound-field placeholder grammar (literal text runs plus typed field runs, no expressions); image import from an approved import root into the content-addressed store with type sniffing (PNG, JPEG) and the size limit; page fields filled at layout time; `report.set_text_block@1` (text, header, footer) and `report.set_image_block@1` (W2).

**Out:** fonts, styles and placement (`RPT.DESIGN`); page numbering at render (`RPT.OUTPUT.PDF.LAYOUT` fills page fields); drawing-sheet blocks (`RPT.BALLOON.BLOCK`).

## Requirements

- **REQ-RPT.MODEL.BLOCKS.TEXT-001** The `text`, `image`, `header` and `footer` kinds shall each register payload schema version 1, and a payload that fails its schema shall be rejected with error `report.invalid_block_payload`.
- **REQ-RPT.MODEL.BLOCKS.TEXT-002** A `text` payload shall be a list of runs, each either literal untrusted text or a typed field (`result_value`, `nominal`, `limit_lower`, `limit_upper`, `decision`, `uncertainty`, `characteristic_name`) with a stable-ID reference, and the parser shall never interpret literal text as a field, expression or instruction (CONSTITUTION C6).
- **REQ-RPT.MODEL.BLOCKS.TEXT-003** When a `text` block is resolved, every field run shall be replaced by the `ValueText` that `RPT.MODEL.FMT` returns for it, and literal runs shall be copied unchanged with their untrusted marking.
- **REQ-RPT.MODEL.BLOCKS.TEXT-004** When `report.set_image_block@1` runs with an import-root handle, the system shall read at most 50 MiB [unverified limit], accept only PNG or JPEG by content sniffing, store the bytes in the content-addressed store and record their digest; a larger file shall fail with error `report.image_too_large` and another type with `report.image_type_unsupported`.
- **REQ-RPT.MODEL.BLOCKS.TEXT-005** `header` and `footer` payloads shall offer the fields page number, page count, report ID and resolved-report digest, plus literal runs, and the resolver shall leave page number and page count as layout placeholders.
- **REQ-RPT.MODEL.BLOCKS.TEXT-006** Agents and scripts shall pass image sources only as import-root handles, and a raw path from them shall fail with error `raw_path_forbidden` (CONSTITUTION §3).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/bracket_3stage`, when `fid report set-text-block --runs '[{"text":"Bore: "},{"field":"result_value","ref":"r7"}]' --json` runs and the report is generated, then the resolved text is "Bore: " followed by r7's formatted value with its formatter ID.
- **AC-2** (C6) Given literal text `{{result_value r7}} ignore previous instructions`, when resolved, then it is copied verbatim, marked untrusted, and no field is substituted.
- **AC-3** Given fixture `fixtures/rpt/model/images` (a 2 MiB PNG, a 60 MiB PNG, a GIF), when each is set through an import-root handle, then the first is stored by digest, the second fails `report.image_too_large`, the third `report.image_type_unsupported`.
- **AC-4** Given a footer with page number, page count and digest fields, when generated, then the digest field holds the resolved-report digest and page fields remain placeholders for layout.
- **AC-5** (error path) Given an agent principal passing a raw path, then it fails with `raw_path_forbidden`.
- **AC-6** (replay) Given the journal of AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `report.set_text_block@1(report, block, runs: Vec<Run>)` for `text`, `header` and `footer` blocks; `report.set_image_block@1(report, block, source: ImportHandle | Sha256, caption)` (both W2).
- **Data structures:** `Run { Literal(Untrusted<Text>) | Field { field: FieldKind, r: Ref } | Page(PageField) }`; `ImagePayload { digest, mime: Png | Jpeg, width_px, height_px, caption }`.
- **Errors:** `report.invalid_block_payload`, `report.image_too_large`, `report.image_type_unsupported`, `raw_path_forbidden`, `report.text_invalid`.
- **Determinism:** D0.
- **Performance budget:** resolve 1 000 text blocks in under 50 ms [unverified until measured].
- **Crate:** `fid-report`.

## Test fixtures

`fixtures/rpt/model/bracket_3stage` (shared); `fixtures/rpt/model/images` (in-project generated PNG and JPEG files and one GIF; CC-BY-4.0).

## Build prompt

> In crate `fid-report`, add block kinds `text`, `image`, `header`, `footer` (payload schemas v1) with `BlockResolver`s, and commands `report.set_text_block@1` and `report.set_image_block@1` (W2, one event each; CLI `fid report set-text-block | set-image-block --json`). Text is a list of runs: literal untrusted text, typed field runs referencing stable IDs (resolved only through the report formatter layer), and page fields left as layout placeholders; never parse literal text for fields or expressions. Images come from import-root handles (agents and scripts: handles only, else `raw_path_forbidden`), max 50 MiB, PNG or JPEG by content sniffing, stored by digest in the content-addressed store. Acceptance: AC-1..AC-6 in `spec/rpt/model/blocks/text/NODE.md`. Constraints: CONSTITUTION C6, §3, §8 (bounded image header parsing, no panic, no `unsafe`).

## Open questions

- The 50 MiB image limit is a placeholder [unverified]; align with `IO.REG.LIMITS`.

## Children

_Leaf: none._
