---
id: RPT.OUTPUT.PDF.WRITE
title: Deterministic PDF writer and the PDF export command
level: 4
parent: RPT.OUTPUT.PDF
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT.PDF-002, REQ-RPT.OUTPUT.PDF-003, REQ-RPT.OUTPUT.PDF-005, REQ-RPT.OUTPUT.PDF-006, REQ-RPT.OUTPUT.PDF-007]
covers: [GOM.RPT.06, PW.RPT.04, SA.RPT.03]
depends_on: [RPT.OUTPUT.PDF.LAYOUT, RPT.OUTPUT.PDF.DRAW, RPT.OUTPUT.PDF.TEXT, RPT.OUTPUT.META, RPT.MODEL.BIND.GEN]
side_effect: W1
fs_effect: write_root
determinism: D0
cli: [fid report export-pdf]
mcp: [report_export_pdf]
journal: [report.export_pdf@1]
updated: 2026-11-17
---

# RPT.OUTPUT.PDF.WRITE: Deterministic PDF writer and the PDF export command

## Purpose

Serialises laid-out, painted pages into PDF bytes that are identical every time for the same input and renderer version, and offers the user command `report.export_pdf@1`, which takes a resolved report, renders it and writes it into an approved local export root through the export sink. The document ID comes from the resolved-report digest, and the `ExportMeta` record is embedded in the document metadata.

## Scope

**In:** the PDF object writer (fixed object numbering, fixed key order, no timestamps, `/ID` from the resolved-report digest, cross-reference table, Flate with pinned settings); embedding fonts from TEXT and images from DRAW; XMP or Info metadata carrying `ExportMeta`; `report.export_pdf@1` (W1, `fs_effect: write_root`) with dry run; the end-to-end performance budget.

**Out:** layout, painting and fonts (siblings); metadata record and the export-root write (`RPT.OUTPUT.META`); PDF/A conformance (see Open questions); signing (`TRUST`); sending files off the machine (`egress.send@1`).

## Requirements

- **REQ-RPT.OUTPUT.PDF.WRITE-001** When the same resolved report is rendered twice with the same renderer version, the writer shall produce byte-identical PDF files.
- **REQ-RPT.OUTPUT.PDF.WRITE-002** The PDF shall contain no creation or modification time, host name, user name or random value, and its `/ID` entries shall both be the first 16 bytes of the resolved-report digest.
- **REQ-RPT.OUTPUT.PDF.WRITE-003** The writer shall embed the canonical `ExportMeta` record from `RPT.OUTPUT.META` in the document metadata, with renderer ID `pdf` and the renderer version.
- **REQ-RPT.OUTPUT.PDF.WRITE-004** `report.export_pdf@1` shall be side-effect class W1 with `fs_effect: write_root`, write only through the export sink, and journal one event holding the resolved-report digest and the sink's `ExportReceipt`.
- **REQ-RPT.OUTPUT.PDF.WRITE-005** If the painter check reports `report.unsupported_block`, then the command shall fail with that error naming the block ID, write no file and journal nothing.
- **REQ-RPT.OUTPUT.PDF.WRITE-006** With `--dry-run`, the command shall render fully and return the byte count and SHA-256 it would write, without writing or journaling.
- **REQ-RPT.OUTPUT.PDF.WRITE-007** Rendering and writing a 50-page report with 20 images shall complete in under 10 s on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/golden/bracket_3stage.resolved.json` and a temporary export root, when `fid report export-pdf --resolved <d> --root <h> --name bracket.pdf --json` runs twice with overwrite, then both files are byte-identical, their SHA-256 equals the golden `fixtures/rpt/output/pdf/bracket_3stage.pdf.sha256`, and two `report.export_pdf@1` events with receipts are journaled.
- **AC-2** Given AC-1's file, when parsed by an independent PDF reader (for example qpdf `--check` [unverified tool availability]), then it is valid, all fonts are embedded subsets, and the metadata holds the `ExportMeta` JSON.
- **AC-3** Given AC-1's file, when searched for date strings, host or user names, then none is found, and `/ID` equals the first 16 bytes of the resolved digest.
- **AC-4** (error path) Given a resolved report with a `chart` block and no chart painter, when exported, then it fails with `report.unsupported_block`, no file exists and the journal length is unchanged.
- **AC-5** Given `--dry-run`, then the returned SHA-256 equals AC-1's and nothing is written or journaled.
- **AC-6** (cross-platform) Given AC-1 on x86-64, ARM64 and wasm32, then the PDF bytes are identical.
- **AC-7** (performance) Given fixture `fixtures/rpt/output/pdf/perf_50p_20img`, when exported on reference hardware, then it completes in under 10 s [unverified until measured].

## Design

- **Registry command:** `report.export_pdf@1(resolved: Sha256 | ReportId, root: ExportRootHandle, name: ExportName, overwrite: bool, dry_run: bool) -> ExportResult` (W1, `fs_effect: write_root`).
- **Algorithm:** load the resolved report -> painter registry check -> layout -> paint -> write objects in fixed order (catalog, pages, fonts in font-ID order, images in digest order, content streams in page order) -> xref -> trailer with derived `/ID` -> hand bytes to `ExportSink::write`.
- **Errors:** `report.unsupported_block`, errors from the sink (`raw_path_forbidden`, `report.export_root_not_local`, `report.export_name_invalid`, `report.export_exists`), `report.resolved_not_found`.
- **Determinism:** D0; renderer version recorded.
- **Performance budget:** REQ-007 [unverified until measured].
- **Crate:** `fid-report-pdf` (module `write`, command registration).

## Test fixtures

Golden resolved reports from `RPT.MODEL.BIND.GEN`; `fixtures/rpt/output/pdf/perf_50p_20img` (generated with seed 13); golden PDF digests. CC-BY-4.0.

## Build prompt

> In crate `fid-report-pdf`, add module `write` and register `report.export_pdf@1` (W1, `fs_effect: write_root`; CLI `fid report export-pdf --json` with `--dry-run`; MCP generated). Serialise PDF deterministically: fixed object order and key order, no timestamps or host/user data, `/ID` = first 16 bytes of the resolved-report digest (both entries), pinned Flate settings, embedded font subsets and images from the sibling modules, and the canonical `ExportMeta` (renderer `pdf` + version) in the metadata. Check painters first (`report.unsupported_block`, no file, nothing journaled). Write only through the `RPT.OUTPUT.META` export sink and journal one event with the resolved digest and receipt; dry run returns size and SHA-256 only. Acceptance: AC-1..AC-7 in `spec/rpt/output/pdf/write/NODE.md`. Constraints: CONSTITUTION §3.1 (local export is W1 `write_root`; leaving the machine goes only through `egress.send@1`), C7 (no network), §8.

## Open questions

- PDF/A (ISO 19005) conformance for archiving is likely wanted by regulated users [unverified]; it would add an output intent and XMP rules and is not required yet.
- The PDF library choice must pass the D-001 licence allowlist and AC-6; writing the object layer in-house is acceptable if no library is deterministic.

## Children

_Leaf: none._
