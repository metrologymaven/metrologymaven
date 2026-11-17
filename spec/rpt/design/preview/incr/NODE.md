---
id: RPT.DESIGN.PREVIEW.INCR
title: Incremental preview regeneration
level: 4
parent: RPT.DESIGN.PREVIEW
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.PREVIEW-001, REQ-RPT.DESIGN.PREVIEW-002, REQ-RPT.DESIGN.PREVIEW-005]
covers: []
depends_on: [RPT.MODEL.BIND, RPT.DESIGN.PLACE.FLOW, RPT.DESIGN.STYLE.SET, RPT.OUTPUT.PDF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.DESIGN.PREVIEW.INCR: Incremental preview regeneration

## Purpose

Keeps the designer's preview current within 500 ms of an edit on a 20-page report. After each layout or style command, it works out which pages can have changed, regenerates only those through the same generation (`RPT.MODEL.BIND`), pagination (`RPT.DESIGN.PLACE.FLOW`) and page layout engine (`RPT.OUTPUT.PDF`) that export uses, and hands the resulting page display lists to the rasteriser. If regeneration fails, the last good pages stay on screen with the error.

## Scope

**In:** a `PreviewSession` per open report holding the last resolved report, page plan and per-page display-list digests; dirty-page analysis from the edited command (frame and break edits dirty their page and following pages of the same flow; style edits dirty pages using the changed fields; binding edits regenerate fully); last-good fallback; pinning to a journal head.

**Out:** generation and binding (`RPT.MODEL.BIND`); pagination (`RPT.DESIGN.PLACE.FLOW`); the page layout engine and display lists (`RPT.OUTPUT.PDF`); pixels (`RPT.DESIGN.PREVIEW.RASTER`).

## Requirements

- **REQ-RPT.DESIGN.PREVIEW.INCR-001** After a layout or style edit on a 20-page report, the session shall produce updated display lists for all affected pages within 500 ms on reference hardware [unverified].
- **REQ-RPT.DESIGN.PREVIEW.INCR-002** The display lists produced incrementally shall be identical (by digest) to those produced by a full regeneration and to those the PDF export lays out for the same head.
- **REQ-RPT.DESIGN.PREVIEW.INCR-003** If regeneration fails, then the session shall keep the last good display lists, mark the preview stale, and expose the error code.
- **REQ-RPT.DESIGN.PREVIEW.INCR-004** The session shall journal nothing and change no document state (side-effect class R1).
- **REQ-RPT.DESIGN.PREVIEW.INCR-005** When pinned to a journal head, the session shall ignore later edits until unpinned.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/twenty_page_report`, when a block on page 7 is moved, then only pages 7 onward within its flow are regenerated (counted by a test hook) and the full-regeneration digests match.
- **AC-2** For each of 50 random edits (seed 7), incremental and full display-list digests are equal, and equal to the export layout digests.
- **AC-3** (error path) Given an edit that makes binding fail, then the previous display lists remain, `stale` is set and the error code is exposed.
- **AC-4** (benchmark) The 95th-percentile update time over the 50 edits is under 500 ms on the reference machine [unverified until measured].
- **AC-5** The journal length is unchanged by any preview activity.

## Design

- **Registry commands:** none (library; the headless command is `RPT.DESIGN.PREVIEW.HEADLESS`).
- **API:** `PreviewSession::open(report, head: Live | Pinned(Head))`; `on_command(&CommandEvent) -> Vec<PageId>`; `pages() -> &[PageDisplayList]`; `status() -> Fresh | Stale { error }`.
- **Algorithm:** classify each event; mark dirty pages; regenerate via bind → paginate → layout for dirty pages; compare per-page digests.
- **Determinism:** D0 for display lists; timing is observed.
- **Performance budget:** as REQ-001.
- **Crate:** `fid-report` (module `preview::incr`).

## Test fixtures

- `fixtures/rpt/design/twenty_page_report`: built by a test builder with tables, text and a drawing block; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `preview::incr`, implement `PreviewSession`: keep the last resolved report, page plan and per-page display-list digests; on each layout, style or binding command classify the dirty pages, regenerate them with the report generator, the paginator and the PDF page layout engine (the same path export uses), and keep the last good pages with a `stale` status and the error code when regeneration fails. Support pinning to a journal head. Journal nothing. Tests AC-1 to AC-5 (fixture `fixtures/rpt/design/twenty_page_report`, 50 seeded edits). CONSTITUTION §8 applies. Spec: `spec/rpt/design/preview/incr/NODE.md`.

## Open questions

- **Cross-domain:** depends on `RPT.OUTPUT.PDF` (open) exposing its page layout engine as display lists; the parent's open question on sharing it is answered by this contract.

## Children

_Leaf: none._
