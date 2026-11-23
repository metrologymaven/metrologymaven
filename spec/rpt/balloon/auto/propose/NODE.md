---
id: RPT.BALLOON.AUTO.PROPOSE
title: Batched balloon and characteristic proposal command
level: 4
parent: RPT.BALLOON.AUTO
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.AUTO-003, REQ-RPT.BALLOON.AUTO-004]
covers: []
depends_on: [RPT.BALLOON.AUTO.TEXT, RPT.BALLOON.AUTO.PARSE, RPT.BALLOON.CHAR, CORE.JRN.BR, CORE.BUS.PIPE]
side_effect: W1
determinism: D0
cli: [fid report propose-balloons]
mcp: [report_propose_balloons]
journal: [report.propose_balloons@1]
built_in: [fid-report]
updated: 2026-11-22
---

# RPT.BALLOON.AUTO.PROPOSE: Batched balloon and characteristic proposal command

## Purpose

Turns a 2D PDF or raster drawing into a ready-to-review, numbered set of proposed balloons and characteristics in one command (`report.propose_balloons@1`, W1 on a draft/agent branch). It runs deterministic vector-text extraction and callout parsing automatically, accepts optional vision-OCR candidate boxes for scanned raster sheets (e.g., from a local or MCP vision model), computes collision-free balloon label positions and drawing zone labels, and stages a batched H2 proposal queue so a human reviews cropped PDF snippets in the Proposal Inbox and adopts all confirmed balloons in one action via `report.add_char_balloon@1`.

## Scope

**In:**
- Command `report.propose_balloons@1` (W1, writes only to a draft or agent branch; CLI `fid report propose-balloons`, MCP `report_propose_balloons`)
- Automatic pipeline: `auto_text` → `auto_parse` → optional merge of caller-supplied OCR `TextRun` candidates (validated as `UntrustedText` inside sheet bounds) → deterministic zone lookup (e.g., `B4` from configurable border grid) → non-overlapping balloon label offset placement → sequential provisional numbering starting after existing balloons
- Explicit dialect (`ASME` or `ISO`) and default drawing unit (`mm` or `in`) parameters; if omitted, flags proposals as requiring confirmation at H2 before adoption
- Batch adoption hand-off producing one atomic sequence of `report.add_char_balloon@1` (`W2`) items on human H2 approval, carrying provenance (`source_sheet`, `source_bbox`, `raw_text`, `origin: vector_parse | ocr_candidate`)

**Out** (and which node owns it instead):
- Running an LLM or vision model inside the command engine (external/MCP or local adapter supplies optional OCR candidate boxes as untrusted input)
- Proposal Inbox UI rendering (`VIZ.INBOX`)
- Atomic working-document commit (`RPT.BALLOON.CHAR`, `CORE.JRN.BR`)

## Requirements

- **REQ-RPT.BALLOON.AUTO.PROPOSE-001** `report.propose_balloons@1` shall write proposed balloons and characteristics only to a draft or agent branch (W1) and shall never mutate the working document directly.
- **REQ-RPT.BALLOON.AUTO.PROPOSE-002** Every proposed item shall record its provisional balloon number, sheet ID, anchor and non-overlapping label `SheetPoint`, source `SheetRect`, raw `UntrustedText`, parsed `DrawingCharInput`, origin (`vector_parse` or `ocr_candidate`), and qualitative confidence word (`high`, `medium`, `low`) with reasons.
- **REQ-RPT.BALLOON.AUTO.PROPOSE-003** When a sheet is `raster_only` and no OCR candidates are provided, the command shall return zero proposals with diagnostic `report.raster_sheet_needs_ocr` rather than failing with a crash or fabricating callouts.
- **REQ-RPT.BALLOON.AUTO.PROPOSE-004** When caller-supplied OCR candidates are passed, every candidate shall pass through `auto_parse` and sheet-bounds validation, rejecting out-of-bounds boxes with `report.balloon_off_sheet` and typing all strings as untrusted text (C6).
- **REQ-RPT.BALLOON.AUTO.PROPOSE-005** Human adoption of an approved proposal batch (H2) shall invoke `report.add_char_balloon@1` (`W2`) per accepted item with exact authored decimal strings and preserve provenance links back to the sheet bounding box.
- **REQ-RPT.BALLOON.AUTO.PROPOSE-006** Running `report.propose_balloons@1` twice on the same drawing revision, register state and options shall produce byte-identical proposals and branch digest (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/auto/vector_bracket.pdf` imported into a plan, when `fid report propose-balloons --plan <p> --drawing <d> --dialect ASME --default-unit mm --json` runs, then a draft/agent branch is created containing all 15 numbered proposals with non-overlapping label offsets and exact authored decimal strings, while the working document remains unchanged.
- **AC-2** Given AC-1, when a human principal adopts 14 accepted proposals (and edits 1) in a single H2 batch, then the working document gains 15 characteristics and balloons 1–15 via `report.add_char_balloon@1`, and an agent principal attempting adoption is refused by `CORE.BUS`.
- **AC-3** Given `scanned_only.pdf` with zero OCR candidates, then the result returns `proposals: []` and diagnostic `report.raster_sheet_needs_ocr`; given valid OCR candidate JSON for 5 callouts, then 5 `ocr_candidate` proposals are staged on the draft branch.
- **AC-4** (replay) Given the journal after AC-1 and AC-2, exact replay yields `IDENTICAL`.

## Design

- **Registry command:** `report.propose_balloons@1(plan: PlanId, drawing_rev: DrawingRevId, sheets?: Vec<SheetId>, dialect?: Dialect, default_unit?: Unit, zone_grid?: ZoneGridSpec, ocr_candidates?: Vec<OcrTextRun>, branch?: BranchName) -> BalloonProposalBatch` (W1, `--json`, `--dry-run`).
- **Data structures:** `BalloonProposalBatch { branch, drawing_rev, proposals: Vec<BalloonProposal>, skipped_runs: u32, diagnostics: Vec<Diagnostic> }`; `BalloonProposal { provisional_number, sheet, anchor: SheetPoint, label: SheetPoint, source_bbox: SheetRect, zone: Option<UntrustedText>, raw_text: UntrustedText, char_input: Option<DrawingCharInput>, origin: VectorParse | OcrCandidate, confidence: ConfidenceWord, reasons: Vec<ReasonCode>, needs_confirmation: bool }`.
- **Algorithm:** load sheets; run `auto_text` + `auto_parse`; merge validated `ocr_candidates` (deduplicating against vector boxes by IoU > 0.5); place balloon labels along an 8-direction collision-free spiral respecting `UX.REPORT.BALLOON` clearances; assign sequential numbers after current max in `RPT.BALLOON.REG`; record `BalloonProposalBatch` on the W1 draft/agent branch.
- **Errors:** `report.not_found`, `report.balloon_off_sheet`, `report.limit_exceeded`, `report.text_invalid`.
- **Determinism:** D0.
- **Performance budget:** 50-callout drawing proposed in < 200 ms excluding optional external OCR [unverified].
- **Crate:** `fid-report` (module `balloon::auto_propose`).

## Test fixtures

`fixtures/rpt/balloon/auto/vector_bracket.pdf`, `scanned_only.pdf`, and `ocr_candidates_5.json` (project-authored, CC-BY-4.0).

## Build prompt

> In crate `fid-report`, add module `balloon::auto_propose` implementing `report.propose_balloons@1` (W1 draft/agent branch, `--json`, `--dry-run`, CLI `fid report propose-balloons`, MCP `report_propose_balloons`). Run `auto_text` and `auto_parse` across the drawing revision's sheets, optionally merge validated untrusted `ocr_candidates` (for scanned PDFs; emit `report.raster_sheet_needs_ocr` if raster-only with none), compute drawing zones and collision-free balloon label offsets, and write a `BalloonProposalBatch` to a draft/agent branch without mutating the working document. Every proposal carries sheet `SheetRect` provenance, raw `UntrustedText`, exact authored decimal strings, origin tag, and confidence word. Wire batch H2 adoption to emit `report.add_char_balloon@1` (`W2`) for accepted items (human principal only). Tests AC-1..AC-4. Rules: CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`), C5 (human adopts proposals), C6 (untrusted text).

## Open questions

- None blocking.

## Children

_Leaf: none._
