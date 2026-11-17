---
id: RPT.BALLOON.SHEET.IMPORT
title: Drawing revision import and sheet coordinates
level: 4
parent: RPT.BALLOON.SHEET
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.SHEET-001, REQ-RPT.BALLOON.SHEET-002, REQ-RPT.BALLOON.SHEET-003, REQ-RPT.BALLOON.SHEET-004, REQ-RPT.BALLOON.SHEET-005]
covers: []
depends_on: [IO.REG.PLUGIN.DRAW, IO.REG.ROOTS, CORE.STO.CAS, CORE.PRJ.CONT]
side_effect: [W2, R0, W2]
determinism: D0
cli: [fid report add-drawing-balloon, fid report list-drawing-balloon, fid report set-size-drawing-balloon]
mcp: [report_add_drawing_balloon, report_list_drawing_balloon, report_set_size_drawing_balloon]
journal: [report.add_drawing_balloon@1, report.list_drawing_balloon@1, report.set_size_drawing_balloon@1]
updated: 2026-11-17
---

# RPT.BALLOON.SHEET.IMPORT: Drawing revision import and sheet coordinates

## Purpose

Brings a 2D drawing file into the project as a drawing revision made of sheets, ready for ballooning. The file is read by IO's bounded drawing reader (`IO.REG.PLUGIN.DRAW`, PDF pages and PNG/TIFF images); this leaf turns its page records into `DrawingRevision` and `DrawingSheet` entities with stable IDs, stores the source bytes and page rasters by content digest, and fixes each sheet's coordinate system (origin top-left, integer 0.01 mm). It also lets the user state the size of a raster sheet that carries no resolution, and offers the `SheetInfo` trait that the balloon register uses.

## Scope

**In:**
- `report.add_drawing_balloon@1` (W2): import one file from an approved import root into a new drawing revision linked to a part revision
- `report.list_drawing_balloon@1` (R0): drawing revisions and their sheets with bounds and status
- `report.set_size_drawing_balloon@1` (W2): state the physical size or resolution of a raster sheet that has none
- Sheet coordinate system and the `SheetInfo` trait (`bounds`, `revision`, `image`, `ready`)
- Drawing metadata (drawing number, revision label, title) as untrusted text

**Out** (and which node owns it instead):
- Parsing and decoding PDF, PNG and TIFF (`IO.REG.PLUGIN.DRAW`)
- Approved import roots and handle resolution (`IO.REG.ROOTS`)
- Carrying balloons to a new revision (`RPT.BALLOON.SHEET.CARRY`)
- Balloons (`RPT.BALLOON.REG`); sheet display, zoom and pan (VIZ)

## Requirements

- **REQ-RPT.BALLOON.SHEET.IMPORT-001** When `report.add_drawing_balloon@1` imports a file, the system shall store the source bytes by SHA-256 digest, create one drawing revision with a UUIDv7 ID, and create one sheet per reader page record, in page order, each with a UUIDv7 sheet ID.
- **REQ-RPT.BALLOON.SHEET.IMPORT-002** Each sheet shall hold its bounds as integer width and height in 0.01 mm with the origin at the top-left corner, taken from the reader's page geometry for PDF pages and from pixel count and stated resolution for raster pages.
- **REQ-RPT.BALLOON.SHEET.IMPORT-003** If a raster page states no resolution, then the sheet shall be created with status `needs_size`, and `SheetInfo::ready` shall return false for it until `report.set_size_drawing_balloon@1` sets a width in 0.01 mm or a resolution in dots per inch.
- **REQ-RPT.BALLOON.SHEET.IMPORT-004** Drawing number, revision label and title shall be typed as untrusted text of at most 256 bytes each and never interpreted (CONSTITUTION C6).
- **REQ-RPT.BALLOON.SHEET.IMPORT-005** If the reader fails (including `limit_exceeded` for more than 200 pages or 500 MiB, `pdf_encrypted` or `malformed`), then the command shall fail with error `report.drawing_unreadable` carrying the reader's error code and offset, and journal nothing.
- **REQ-RPT.BALLOON.SHEET.IMPORT-006** For an agent or script principal, the command shall accept the file only as an approved import-root handle (`IO.REG.ROOTS`), never a raw path.
- **REQ-RPT.BALLOON.SHEET.IMPORT-007** Reader warnings (ignored PDF actions, JavaScript, embedded files, forms) shall be stored with the drawing revision and returned by the import command.
- **REQ-RPT.BALLOON.SHEET.IMPORT-008** If the same part revision already has a drawing revision with the same drawing number and revision label, then the import shall fail with error `report.drawing_revision_exists` and journal nothing.

## Acceptance criteria

- **AC-1** Given `fixtures/io/reg/plugin/draw/three_pages.pdf` in an approved import root and a part revision P1/A, when `fid report add-drawing-balloon --part-rev P1/A --file root://drawings/three_pages.pdf --number D-100 --rev B --json` runs, then one drawing revision with three sheets in page order is created, page 2 bounded by its CropBox, the source digest is stored, and one event is journaled.
- **AC-2** Given `fixtures/io/reg/plugin/draw/nodpi.png`, when imported, then its sheet has status `needs_size` and `SheetInfo::ready` is false; when `set-size-drawing-balloon --sheet <s> --dpi 300` runs, then the bounds equal pixel count × 2540 / 300 rounded half-even and status is `ready`.
- **AC-3** (error path) Given `encrypted.pdf` or `pages201.pdf`, then the import fails with `report.drawing_unreadable` carrying `pdf_encrypted` or `limit_exceeded`, and the journal length is unchanged.
- **AC-4** (error path) Given an agent principal passing a raw path, then the command is refused by root resolution and journals nothing; given the same drawing number and revision label twice, then `report.drawing_revision_exists`.
- **AC-5** Given `js_action.pdf`, then the import succeeds and returns warning `pdf_javascript_ignored`, which `list-drawing-balloon` also shows.
- **AC-6** (replay) Given the journal after AC-2, when replayed in exact mode, then the outcome is `IDENTICAL` and the sheet IDs are unchanged.

## Design

- **Registry commands:**
  - `report.add_drawing_balloon@1(part_rev: RevisionId, file: FileHandle, drawing_number: Untrusted<Text>, revision_label: Untrusted<Text>, title: Option<Untrusted<Text>>) -> { drawing_rev: DrawingRevision, warnings: Vec<ReaderWarning> }`: W2, `--json`, `--dry-run`.
  - `report.list_drawing_balloon@1(part_rev: Option<RevisionId>) -> Vec<DrawingRevision>`: R0, ordered by drawing number, then revision label, then ID.
  - `report.set_size_drawing_balloon@1(sheet: SheetId, size: Width{cmm: i64} | Dpi{x: u32, y: u32}) -> DrawingSheet`: W2.
- **Data structures:** `DrawingRevision { id, part_rev, drawing_number, revision_label, title, source_digest: Sha256, sheets: Vec<SheetId>, warnings, carried_from: BTreeMap<BalloonId, BalloonId> }` (the carry map is filled by `RPT.BALLOON.SHEET.CARRY`); `DrawingSheet { id, drawing_rev, index: u32, width_cmm: i64, height_cmm: i64, status: Ready | NeedsSize, image: Option<BlobRef>, px: Option<(u32, u32)> }`.
- **Trait offered:** `SheetInfo { fn bounds(&self, s: SheetId) -> Option<(i64, i64)>; fn revision(&self, s: SheetId) -> Option<DrawingRevId>; fn image(&self, s: SheetId) -> Option<BlobRef>; fn ready(&self, s: SheetId) -> bool; }`.
- **Algorithm:** resolve the handle → read bytes with the reader (pure, bounded) → store source and raster blobs in the CAS → build entities → commit one event. Raster size from dpi: `px × 2540 / dpi`, half-even; from a stated width, height keeps the pixel aspect ratio.
- **Errors:** `report.drawing_unreadable { code, offset }`, `report.drawing_revision_exists`, `report.text_invalid`, `report.not_found`, `report.size_invalid`.
- **Determinism:** D0; the reader is D0 and all geometry is integer.
- **Performance budget:** import of a 50-page PDF in under 2 s including blob writes [unverified until measured].
- **Crate:** `fid-report` (module `balloon::sheet`).

## Test fixtures

- The generated drawing fixtures of `IO.REG.PLUGIN.DRAW` under `fixtures/io/reg/plugin/draw/` (CC-BY-4.0), plus a part revision created by a test builder.

## Build prompt

> In crate `fid-report`, module `balloon::sheet`, implement drawing-revision import for ballooning: `report.add_drawing_balloon@1` (W2), `report.list_drawing_balloon@1` (R0) and `report.set_size_drawing_balloon@1` (W2), all `--json`; CLI `fid report add-drawing-balloon`, `list-drawing-balloon`, `set-size-drawing-balloon`; MCP names derived.
>
> Import: resolve the file through approved import-root handles (raw paths refused for agent and script principals); read it with the IO drawing reader crate `fid-io-draw` (PDF page geometry, PNG/TIFF rasters; it enforces 200 pages, 500 MiB, 400 Mpx); on reader error fail with `report.drawing_unreadable { code, offset }` and journal nothing. Store the source bytes and page rasters in the content-addressed blob store; create a `DrawingRevision` (UUIDv7, linked to a part revision, drawing number and revision label unique per part revision, metadata as untrusted text ≤ 256 bytes) and one `DrawingSheet` per page in page order (UUIDv7; width and height in integer 0.01 mm, origin top-left). Raster pages with no resolution get status `needs_size` until a width or dpi is set (size = px × 2540 / dpi, half-even). Keep reader warnings with the revision. Offer the `SheetInfo` trait (`bounds`, `revision`, `image`, `ready`) used by the balloon register and block.
>
> **Acceptance tests:** AC-1 to AC-6 using the IO drawing fixtures, including exact replay giving `IDENTICAL`.
>
> **Constraints:** CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`); C6; no floating point; `BTreeMap` only. Do not parse PDF or images yourself. Spec: `spec/rpt/balloon/sheet/import/NODE.md`.

## Open questions

- **Cross-domain:** no node owns rendering vector PDF page content to pixels for on-screen ballooning (raised in `IO.REG.PLUGIN.DRAW`); vector PDF sheets have exact geometry but `image` is `None` until a renderer exists.
- Command names `*_drawing_balloon` stay inside the parent's `*_balloon` pattern; `report.import_drawing` would read better.

## Children

_Leaf: none._
