---
id: IO.REG.PLUGIN.DRAW
title: Bounded drawing reader for PDF pages and raster images
level: 4
parent: IO.REG.PLUGIN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.REG.PLUGIN-002]
covers: []
depends_on: [IO.REG.PLUGIN.ABI, IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-io-draw]
updated: 2026-11-22
---

# IO.REG.PLUGIN.DRAW: Bounded drawing reader for PDF pages and raster images

## Purpose

The IO-owned reader that `RPT.BALLOON.SHEET` needs to bring 2D drawings into a project: it reads a PDF's page structure (count, page boxes, rotation) and decodes PNG and TIFF raster images (dimensions, stated resolution, pixels), and returns them as drawing-page records. It is a first-party reader built against the plugin contract (bytes in, records out, no file, network or document access), so it can later move into the WASM sandbox unchanged. PDF content is never executed.

## Scope

**In:**
- PDF: header and cross-reference parsing (including object and xref streams), page tree walk, MediaBox and CropBox, `/Rotate`, `/UserUnit`, page count; Flate decoding of streams needed for structure only
- Rejection of encrypted PDFs; actions, JavaScript, embedded files and forms ignored and counted in the report
- PNG (all standard colour types and bit depths, `pHYs` resolution) and TIFF (baseline plus LZW, Deflate, PackBits, CCITT G3/G4 [unverified: decoder availability], resolution tags); multi-page TIFF as several pages
- Page geometry in integer 0.01 mm units, origin top-left (the sheet convention of `RPT.BALLOON.SHEET`); raster pixels as RGBA8 or Gray8
- Limits: 200 pages, 500 MiB input (from `RPT.BALLOON.SHEET`), 400 Mpx per raster page [unverified]

**Out:**
- Drawing revisions, sheets, balloons and the `report.add_drawing_balloon@1` command (`RPT.BALLOON.SHEET`)
- Rendering PDF page content to pixels for display (not owned yet; see open questions)
- Text extraction from PDF content streams (later; see open questions)

## Requirements

- **REQ-IO.REG.PLUGIN.DRAW-001** The reader shall receive only the source bytes and options through the contract read entry point and shall use no file, network, clock or document access.
- **REQ-IO.REG.PLUGIN.DRAW-002** For a PDF, the reader shall return one page record per page in page-tree order, with width and height in integer 0.01 mm taken from the CropBox (or MediaBox if no CropBox), after applying `/Rotate` and `/UserUnit`.
- **REQ-IO.REG.PLUGIN.DRAW-003** For a PNG or TIFF, the reader shall return one page record per image with pixel dimensions, decoded pixels and the stated resolution, or `resolution: none` when the file states none.
- **REQ-IO.REG.PLUGIN.DRAW-004** If a PDF is encrypted, then the reader shall fail with `pdf_encrypted`.
- **REQ-IO.REG.PLUGIN.DRAW-005** The reader shall never execute PDF actions or JavaScript and shall list their presence, and the presence of embedded files and forms, as warnings in the import report.
- **REQ-IO.REG.PLUGIN.DRAW-006** If the input has more than 200 pages, is larger than 500 MiB, or a raster page exceeds 400 Mpx, then the reader shall fail with `limit_exceeded` naming the limit.
- **REQ-IO.REG.PLUGIN.DRAW-007** On malformed input the reader shall return a typed error with byte offset and shall not panic or loop without bound, verified by a fuzz target over PDF, PNG and TIFF seeds.

## Acceptance criteria

- **AC-1** Given `fixtures/io/reg/plugin/draw/a3_landscape.pdf` (one page, 420 × 297 mm MediaBox), then one page record with width 42000 and height 29700.
- **AC-2** Given `rotated.pdf` (A4 portrait MediaBox, `/Rotate 90`), then width 29700 and height 21000.
- **AC-3** Given `three_pages.pdf` with a CropBox on page 2, then three records in order and page 2 sized from the CropBox.
- **AC-4** Given `scan_300dpi.png` (3508 × 2480 px, `pHYs` 300 dpi) and `nodpi.png`, then the first has resolution 300 dpi and the second `resolution: none`.
- **AC-5** Given `g4_two_pages.tif` (CCITT G4), then two pages decoded to Gray8 with pixel checksums equal to `g4_two_pages.sha256`.
- **AC-6** (error path) Given `encrypted.pdf`, then `pdf_encrypted`; given `js_action.pdf`, then success with warning `pdf_javascript_ignored`.
- **AC-7** (error path) Given `pages201.pdf`, then `limit_exceeded` (pages).
- **AC-8** Given the fuzz target and hostile seeds from `IO.CORP`, when run for the CI budget, then no panic or timeout.

## Design

- **Registry commands:** none (contract reader registered with `IO.REG.DETECT` as formats `pdf-drawing` and `raster-drawing`, origin `first_party`).
- **Data structures:** `Record::DrawingPage { index, width_cmm: i64, height_cmm: i64, rotation, box_used: Crop|Media, raster: Option<Raster { w, h, format, dpi: Option<(u32,u32)>, pixels_blob }> }`.
- **Algorithm:** PDF: locate `startxref`, parse xref tables and streams with object-count and nesting limits, walk the page tree iteratively with a visited set (cycle → `malformed`), inherit boxes per page-tree rules, convert PDF units (1/72 inch × UserUnit) to 0.01 mm with one rounding (half-even). PNG and TIFF: pure-Rust decoders with explicit size checks before allocation [unverified: decoder crates and D-001 allowlist]. Pixels go to a CORE blob through the staging sink.
- **Errors:** `pdf_encrypted`, `limit_exceeded`, `malformed{at}`, `truncated_file{at}`, `format_unknown`.
- **Determinism:** D0.
- **Performance budget:** 50-page PDF structure in < 1 s; 300 dpi A0 TIFF G4 decoded in < 2 s [unverified until measured].
- **Crate:** `fid-io-draw`.

## Test fixtures

Generated PDFs, PNGs and TIFFs under `fixtures/io/reg/plugin/draw/`, produced by a committed generator script (CC-BY-4.0); hostile seeds from `IO.CORP`.

## Build prompt

> Create crate `fid-io-draw`: a first-party reader built on the `fid-io-plugin` contract (bytes and options in, `DrawingPage` records out; no file, network, clock or document access), registered as formats `pdf-drawing` and `raster-drawing`. PDF: bounded xref and object-stream parsing, iterative page-tree walk with cycle detection, CropBox else MediaBox, `/Rotate`, `/UserUnit`, size in integer 0.01 mm (origin top-left); reject encryption (`pdf_encrypted`); never execute actions or JavaScript, report them and embedded files and forms as warnings. PNG and TIFF (baseline, LZW, Deflate, PackBits, CCITT G3/G4; multi-page): dimensions, stated resolution or `none`, pixels to a blob via the staging sink. Limits: 200 pages, 500 MiB, 400 Mpx per raster page (`limit_exceeded`). Add a fuzz target. CONSTITUTION §8: no panics, no unwrap, no `unsafe`. Tests AC-1..AC-8.

## Open questions

- **Cross-domain:** rendering PDF page content to pixels for on-screen ballooning has no owner. A pure-Rust PDF renderer is a large, high-risk component; it could run as a sandboxed plugin (`IO.REG.PLUGIN.WASM`), or RPT/VIZ could accept vector PDFs only through a renderer they own. Until decided, vector PDF sheets have correct geometry but no display raster; raised for `RPT.BALLOON.SHEET` and VIZ.
- Text extraction (REQ-RPT.BALLOON.SHEET-004 mentions extracted drawing text) is not in this leaf; it would be a later child once a use needs it.

## Children

_Leaf: none._
