---
id: IO.RES.REPORT
title: Report-file export envelope
level: 3
parent: IO.RES
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.RES-012]
covers: [GOM.EXP.03]
depends_on: [IO.REG.ENV, IO.REG.WRITE]
side_effect: W1
determinism: D0
cli: [fid export results-report]
mcp: [export_results_report]
journal: [export.results_report@1]
updated: 2026-11-16
---

# IO.RES.REPORT: Report-file export envelope

## Purpose

Writes a report that RPT has already rendered (PDF or another output kind) into an approved export root. IO owns only the file write and its journaling; RPT owns content, layout and rendering. This keeps report export inside the same root, overwrite and atomic-write rules as every other export.

## Scope

**In:**
- `export.results_report@1` (W1, `fs_effect: write_root`, CONSTITUTION §3.1)
- Taking a rendered-report blob reference (digest, media type) and writing its bytes unchanged
- Journaling the target handle, blob digest, media type and written digest

**Out:**
- Rendering, layout and forms (RPT.OUTPUT); this leaf never inspects report content

## Requirements

- **REQ-IO.RES.REPORT-001** When a report export is requested, the system shall write the bytes of the RPT-rendered blob unchanged to the target in an approved export root.
- **REQ-IO.RES.REPORT-002** If the referenced blob does not exist or is not of a report media type, then the export shall fail with `report_not_found` or `media_type_not_report`.
- **REQ-IO.RES.REPORT-003** The journal event shall record the source blob SHA-256, media type and the written file's SHA-256, which shall be equal to the blob digest.
- **REQ-IO.RES.REPORT-004** The command shall not lay out, re-render or modify report content.

## Acceptance criteria

- **AC-1** Given blob `fixtures/io/res/report/sample.pdf` stored in the test blob store and export root `exp`, when `fid export results-report --report <digest> --target root://exp/r.pdf --json` runs, then `r.pdf` exists, its SHA-256 equals the blob digest and the journal holds one `export.results_report@1` event with both digests.
- **AC-2** (error path) Given an unknown digest, then the command fails `report_not_found` and writes nothing.
- **AC-3** (error path) Given a blob of media type `model/stl`, then fails `media_type_not_report`.
- **AC-4** (error path) Given an existing `r.pdf` and no `--overwrite`, then fails `target_exists`.
- **AC-5** (error path) Given an agent principal and a raw path target, then fails `raw_path_forbidden`.
- **AC-6** (replay) Given the journal of AC-1, when replayed exactly into an empty root, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `export.results_report@1`, W1 with `fs_effect: write_root`; params `report: BlobDigest`, `target: RootHandle`, `overwrite: bool = false`, `dry_run: bool = false`; result `ExportReport { target, sha256, bytes, media_type }`.
- **Data structures:** none new.
- **Algorithm:** look up blob in CORE.STO; check media type against an allowlist (`application/pdf`, `text/html`, `application/vnd.openxmlformats-officedocument.*` [unverified: RPT output kinds]); stream bytes through `AtomicWriter`; compare digests.
- **Errors:** `report_not_found`, `media_type_not_report`, `target_exists`, `raw_path_forbidden`, `path_outside_root`.
- **Determinism:** D0.
- **Performance budget:** bounded by disk throughput.
- **Crate:** `fid-io-res`.

## Test fixtures

A small generated PDF under `fixtures/io/res/report/` (CC-BY-4.0) loaded into a test blob store.

## Build prompt

> In crate `fid-io-res`, implement registry command `export.results_report@1` (W1, `fs_effect: write_root`; CLI `fid export results-report`; MCP `export_results_report`): given a blob digest of a rendered report and a `root://` target handle, check the blob exists (`report_not_found`) and has a report media type (`media_type_not_report`), then stream its bytes unchanged through the `fid-io-reg` `AtomicWriter` (overwrite rule `target_exists`, root rules `raw_path_forbidden`/`path_outside_root`), and journal blob digest, media type and written digest (must match). Never re-render or modify content; rendering belongs to RPT. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- GOM.EXP.03 (PDF export) may move to RPT if maintainers prefer (carried from IO and IO.RES).

## Children

_Leaf: none._
