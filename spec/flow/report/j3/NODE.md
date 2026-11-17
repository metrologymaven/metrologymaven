---
id: FLOW.REPORT.J3
title: J3 drawing-only first-article inspection from CMM, arm and hand-gauge data
level: 3
parent: FLOW.REPORT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.REPORT-001, REQ-FLOW.REPORT-005, REQ-FLOW.REPORT-006, REQ-FLOW.REPORT-010, REQ-FLOW.REPORT-011]
covers: [JOB.ACC.03]
depends_on: [IO.RES.CSV, RPT.BALLOON.SHEET, RPT.BALLOON.REG, RPT.BALLOON.CHAR, RPT.BALLOON.ENTRY, RPT.BALLOON.GAUGE, RPT.FORMS.HDR, RPT.FORMS.ROWS, RPT.FORMS.FINAL, RPT.OUTPUT.PDF, RPT.OUTPUT.SHEET]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.REPORT.J3: J3 drawing-only first-article inspection from CMM, arm and hand-gauge data

## Purpose

Owns acceptance job `JOB.ACC.03`: with no CAD, a quality engineer balloons a 3-sheet vector PDF drawing with 80 characteristics, imports CMM CSV results for 50, arm CSV results for 15, enters 15 hand-gauge readings, and produces AS9102 Forms 1 to 3 and a customer XLSX; then repeats for the next serial.

## Scope

**In:**
- Synthetic data: project-authored 3-sheet vector PDF drawing (80 characteristics), CMM CSV (50), arm CSV (15), hand-gauge readings list (15) for 2 serials, and a requirement truth table
- Journey map `JOB.ACC.03`
- Packages `j3-unassisted` (gating), `j3-next` (gating) and `j3-assisted` (non-gating)

**Out:**
- Keyboard-only variant (owned by `FLOW.REPORT.KBD`)
- Ballooning, readings, gauges, forms and exports (owned by `RPT.BALLOON`, `RPT.FORMS`, `RPT.OUTPUT`, `IO.RES.CSV`)

## Requirements

- **REQ-FLOW.REPORT.J3-001** The J3 data generator shall produce, from a seed, the 3-sheet drawing, both CSV files mapped to 50 and 15 distinct balloons, a 15-entry hand-gauge reading list, and a truth table of all 80 requirements.
- **REQ-FLOW.REPORT.J3-002** The J3 journey map shall list drawing import, ballooning (`report.add_char_balloon@1`), gauge registration, CSV imports, manual reading entry (`report.enter_reading@1`, human-only), header data, form check, finalisation (human-only) and PDF and XLSX export.
- **REQ-FLOW.REPORT.J3-003** The `j3-unassisted` package shall declare a `requirements_equal` check that every characteristic's nominal, tolerance and unit equal the truth table, giving zero transcription errors.
- **REQ-FLOW.REPORT.J3-004** The package shall declare a `sheet_cells_equal` check that every value cell of the exported XLSX equals its ResultRecord value as formatted.
- **REQ-FLOW.REPORT.J3-005** The packages shall declare time targets of under 45 minutes for `j3-unassisted` (including ballooning) and under 5 minutes for `j3-next` [unverified].
- **REQ-FLOW.REPORT.J3-006** `j3-unassisted` and `j3-next` shall be gating; `j3-assisted` (agent proposes balloons from the PDF, human adopts) shall be non-gating.

## Acceptance criteria

- **AC-1** Given seed 3, when `cargo xtask accept-data report --job j3` runs twice, then outputs are byte-identical.
- **AC-2** Given the J3 packages, when `fid accept run-suite --job JOB.ACC.03 --json` runs, then all checks pass.
- **AC-3** (error path) Given a copy whose journal sets one tolerance to 0.02 instead of 0.2, then `requirements_equal` fails naming the balloon.
- **AC-4** (error path) Given an XLSX exporter fault injected in a test build that rounds one cell differently, then `sheet_cells_equal` fails.

## Design

- **Registry command(s):** none; generator `cargo xtask accept-data report --job j3`; content under `acceptance/jobs/j3/`.
- **Data structures:** packages per `acceptance-job@1`; checks `replay_outcome`, `requirements_equal`, `sheet_cells_equal`, `result_count` (80), `file_digest`.
- **Algorithm:** drawing generated from project-authored geometry with dimensions and feature control frames drawn as vector text; CSVs in the column map used by `IO.RES.CSV`.
- **Errors:** generator `DATA-IO`.
- **Determinism:** D0; times observed (N).
- **Performance budget:** none beyond the task targets.
- **Crate:** `fid-accept-data` (generator); content otherwise.

## Test fixtures

Generated data, CC0, recorded in `datasets.toml`; drawings are original works.

## Build prompt

> Add `--job j3` to `cargo xtask accept-data report` in crate `fid-accept-data`: a project-authored 3-sheet vector PDF drawing with 80 characteristics, CMM CSV (50), arm CSV (15), hand-gauge list (15) for 2 serials and `truth.json`. Create `acceptance/jobs/j3/` with `map.json` (`journey-map@1`, `JOB.ACC.03`) and packages `j3-unassisted` (gating, 45 min), `j3-next` (gating, 5 min), `j3-assisted` (tracked; agent proposes balloons, human adopts); checks replay_outcome, requirements_equal, sheet_cells_equal, result_count = 80, file_digest. Verify AC-1..AC-4 of spec node FLOW.REPORT.J3.

## Open questions

- AS9102 form layout follows D-015 (accepted, option C; counsel review before release).

## Children

_Leaf: none._
