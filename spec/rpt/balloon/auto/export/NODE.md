---
id: RPT.BALLOON.AUTO.EXPORT
title: Quick ballooned-PDF and AS9102 Form 3 starter pack export
level: 4
parent: RPT.BALLOON.AUTO
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.AUTO-005]
covers: []
depends_on: [RPT.BALLOON.REG, RPT.BALLOON.BLOCK, RPT.FORMS.ROWS, IO.REG.ROOTS, IO.RES.CSV.OUT]
uses_artifacts: [UX.REPORT.BALLOON]
side_effect: W1
determinism: D0
cli: [fid report export-balloon-pack]
mcp: [report_export_balloon_pack]
journal: [report.export_balloon_pack@1]
updated: 2026-11-22
---

# RPT.BALLOON.AUTO.EXPORT: Quick ballooned-PDF and AS9102 Form 3 starter pack export

## Purpose

Gives job shops immediate Day-1 output from 2D drawing ballooning in a single command (`report.export_balloon_pack@1`): writes a **vector-overlaid ballooned PDF** (source PDF sheets with crisp vector balloons and leader lines drawn at exact 0.01 mm coordinates per `UX.REPORT.BALLOON`) alongside an **AS9102 Form 3 starter CSV** ordered by balloon number (balloon #, zone, requirement with exact authored decimals, tolerance, dialect, status/result if evaluated), into an approved local export root.

## Scope

**In:**
- Command `report.export_balloon_pack@1` (W1, `fs_effect: write_root`, CLI `fid report export-balloon-pack`, MCP `report_export_balloon_pack`)
- Pure-Rust vector balloon overlay onto PDF sheets (preserving original PDF page geometry and adding clean vector balloon circles/shapes, leader lines, arrowheads and numbers in ascending balloon order; never baking balloons into lossy rasters)
- Starter AS9102 Form 3 CSV export via `IO.RES.CSV.OUT` with verbatim authored requirement strings, zones, and any existing kernel-formatted results (or blank/not-evaluated status when no readings exist yet)
- Deterministic output bytes (D0: fixed PDF metadata/timestamps, sorted rows, SHA-256 digests returned in the command result and journaled)
- Export-root confinement through `IO.REG.ROOTS` (fail closed on path traversal or unapproved roots)

**Out** (and which node owns it instead):
- Final W3 signed AS9102 forms (`RPT.FORMS.FINAL`)
- Multi-page custom report layouts (`RPT.OUTPUT.PDF`, `RPT.DESIGN`)
- Data egress off the machine (`egress.send@1`, X1)

## Requirements

- **REQ-RPT.BALLOON.AUTO.EXPORT-001** When `report.export_balloon_pack@1` runs on a drawing revision, the system shall write a vector-overlaid ballooned PDF and a Form 3 starter CSV in ascending balloon-number order into the approved local export root and return their SHA-256 digests.
- **REQ-RPT.BALLOON.AUTO.EXPORT-002** Balloon overlays shall be written as vector PDF graphics at the exact stored 0.01 mm sheet coordinates styled from `UX.REPORT.BALLOON`, with non-colour cues when outcome marking is enabled.
- **REQ-RPT.BALLOON.AUTO.EXPORT-003** Form 3 starter rows shall emit authored nominal and tolerance strings verbatim (and kernel-formatted result strings where results exist), quoting untrusted text safely against CSV formula injection (`=`, `+`, `-`, `@` prefixes in free-text note/zone fields).
- **REQ-RPT.BALLOON.AUTO.EXPORT-004** If the destination path escapes the approved local export root, then the command shall fail closed with `io.root_violation` and write nothing.
- **REQ-RPT.BALLOON.AUTO.EXPORT-005** Running `report.export_balloon_pack@1` twice on the same journal head shall produce byte-identical PDF and CSV files (D0).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/auto/vector_bracket.pdf` with 15 adopted balloons, when `fid report export-balloon-pack --drawing <d> --out <export_root>/pack --json` runs, then `<export_root>/pack/ballooned.pdf` and `<export_root>/pack/form3-starter.csv` are written, the CSV has 15 rows numbered 1..15 with exact authored requirements, and both SHA-256 digests match the journal entry.
- **AC-2** Given AC-1 run twice on the same head, then both PDF and CSV files are byte-identical.
- **AC-3** (error path) Given `--out ../outside`, then the command fails with `io.root_violation` and writes no files.
- **AC-4** Given a balloon zone containing `=CMD|'/C calc'!A0`, then the CSV neutralises the untrusted string and never emits an unescaped spreadsheet formula.

## Design

- **Registry command:** `report.export_balloon_pack@1(drawing_rev: DrawingRevId, out_dir: ExportRootPath, outcome_marking?: bool, include_unmeasured?: bool) -> BalloonPackResult` (W1, `fs_effect: write_root`, `--json`, `--dry-run`).
- **Data structures:** `BalloonPackResult { pdf_path, pdf_digest: Sha256, csv_path, csv_digest: Sha256, balloon_count: u32 }`.
- **Algorithm:** resolve balloons from `RPT.BALLOON.REG` and block geometry from `RPT.BALLOON.BLOCK`; append a deterministic vector PDF overlay content stream per page using `UX.REPORT.BALLOON` dimensions; generate Form 3 starter rows from `RPT.FORMS.ROWS` (allowing unmeasured draft rows when `include_unmeasured` is true, clearly labelled `not_evaluated`) and emit CSV via `IO.RES.CSV.OUT` with formula-injection neutralisation; write atomically inside approved `IO.REG.ROOTS`.
- **Errors:** `io.root_violation`, `report.not_found`, `report.no_balloons`.
- **Determinism:** D0.
- **Performance budget:** 100-balloon drawing pack exported in < 250 ms [unverified].
- **Crate:** `fid-report` (module `balloon::auto_export`).

## Test fixtures

`fixtures/rpt/balloon/auto/vector_bracket.pdf` with 15 adopted balloons and one untrusted formula-injection zone string (CC-BY-4.0).

## Build prompt

> In crate `fid-report`, add module `balloon::auto_export` implementing `report.export_balloon_pack@1` (W1, `fs_effect: write_root`, `--json`, `--dry-run`, CLI `fid report export-balloon-pack`, MCP `report_export_balloon_pack`). Overlay crisp vector balloons and leader lines onto the source PDF sheets at exact stored 0.01 mm coordinates styled from `UX.REPORT.BALLOON` (with non-colour cues when outcome marking is on), and export an ascending-balloon-number AS9102 Form 3 starter CSV (verbatim authored requirements, kernel-formatted results or `not_evaluated`, neutralising CSV formula injection on untrusted text) into an approved local export root (`IO.REG.ROOTS`). Output deterministic D0 bytes (fixed PDF metadata/timestamps) and return SHA-256 digests. Tests AC-1..AC-4. Rules: CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`), C6, clean-room balloon style.

## Open questions

- None blocking.

## Children

_Leaf: none._
