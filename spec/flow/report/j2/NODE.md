---
id: FLOW.REPORT.J2
title: J2 transcription AS9102 from an existing CMM report
level: 3
parent: FLOW.REPORT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.REPORT-001, REQ-FLOW.REPORT-002, REQ-FLOW.REPORT-003, REQ-FLOW.REPORT-004, REQ-FLOW.REPORT-010, REQ-FLOW.REPORT-011]
covers: [JOB.ACC.02]
depends_on: [IO.DMIS.OUT, IO.RES.TRANS, RPT.BALLOON.SHEET, RPT.BALLOON.REG, RPT.BALLOON.CHAR, RPT.FORMS.HDR, RPT.FORMS.ROWS, RPT.FORMS.FINAL, RPT.OUTPUT.PDF]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.REPORT.J2: J2 transcription AS9102 from an existing CMM report

## Purpose

Owns acceptance job `JOB.ACC.02`, the first paid wedge: a quality engineer imports a 120-characteristic DMIS output file from another vendor's CMM software, maps it to an already-ballooned drawing, resolves 5 seeded requirement mismatches found by lint and exports an AS9102 Form 3, then repeats for the next serial. It owns the synthetic data, journey map, packages, checks and targets.

## Scope

**In:**
- Synthetic data: a project-authored 2-sheet drawing (original work) with 120 balloons, a DMIS output file for 2 serials written in the DMIS 5.3 output syntax, 5 seeded mismatches (nominal, tolerance or unit differences between drawing and source)
- Journey map `JOB.ACC.02`
- Packages `j2-unassisted` (gating), `j2-next` (gating, second serial) and `j2-assisted` (non-gating)

**Out:**
- Keyboard-only variant (owned by `FLOW.REPORT.KBD`)
- Transcription records, balloons and forms (owned by `IO.RES.TRANS`, `RPT.BALLOON`, `RPT.FORMS`)

## Requirements

- **REQ-FLOW.REPORT.J2-001** The J2 data generator shall produce, from a seed, a DMIS output file with 120 characteristic results for each of 2 serials, the matching ballooned drawing as vector PDF, and a truth file listing the 5 seeded mismatches by balloon number.
- **REQ-FLOW.REPORT.J2-002** The J2 journey map shall list import drawing, place balloons (pre-done in the dataset project), import DMIS results (`import.results_dmis@1`), map results to balloons, check the form (`report.check_fai@1`), resolve each finding, finalise (`report.finalize_fai@1`, human-only) and export PDF, each with its owning node.
- **REQ-FLOW.REPORT.J2-003** The `j2-unassisted` package shall declare a `lexical_identical` check that every transcribed value string equals the source file's string byte for byte, and a check that every Form 3 line carries an evaluation basis.
- **REQ-FLOW.REPORT.J2-004** The package shall declare a `lint_findings_equal` check that the form check reports exactly the 5 seeded mismatches, by balloon number.
- **REQ-FLOW.REPORT.J2-005** The package shall declare a check that no value was entered by a manual-reading command, and time targets of under 5 minutes for `j2-unassisted` and under 1 minute for `j2-next` [unverified].
- **REQ-FLOW.REPORT.J2-006** `j2-unassisted` and `j2-next` shall be gating; `j2-assisted` (agent proposes the result-to-balloon mapping, human adopts) shall be non-gating.

## Acceptance criteria

- **AC-1** Given seed 2, when `cargo xtask accept-data report --job j2` runs twice, then outputs are byte-identical and the truth file lists 5 mismatches.
- **AC-2** Given the J2 packages, when `fid accept run-suite --job JOB.ACC.02 --json` runs, then all checks pass and replay is `IDENTICAL`.
- **AC-3** (error path) Given a copy whose journal alters one transcribed digit, then `lexical_identical` fails naming the balloon.
- **AC-4** (error path) Given a build whose lint misses one seeded mismatch, then `lint_findings_equal` fails.

## Design

- **Registry command(s):** none; generator `cargo xtask accept-data report --job j2 --seed <u64>`; content under `acceptance/jobs/j2/`.
- **Data structures:** packages per `acceptance-job@1`; checks `replay_outcome`, `lexical_identical`, `basis_present`, `lint_findings_equal`, `no_manual_entry`, `file_digest`.
- **Algorithm:** generator writes DMIS output statements for 120 characteristics with realistic vendor-neutral header text (no vendor strings copied); drawing generated as vector PDF from project-authored geometry.
- **Errors:** generator `DATA-IO`; runtime failures are runner findings.
- **Determinism:** D0 for data and values; times observed (N).
- **Performance budget:** machine time for `j2-next` under 20 s [unverified].
- **Crate:** `fid-accept-data` (generator); content otherwise.

## Test fixtures

Generated data, CC0, recorded in `datasets.toml`; drawings are original works of the project.

## Build prompt

> Add `cargo xtask accept-data report --job j2 --seed <u64>` to crate `fid-accept-data`: write a DMIS output file (120 characteristics, 2 serials, neutral header, no copied vendor text), a project-authored 2-sheet vector PDF drawing with 120 balloons, and `truth.json` with 5 seeded mismatches. Then create `acceptance/jobs/j2/` with `map.json` (`journey-map@1`, `JOB.ACC.02`) and packages `j2-unassisted` (gating, 5 min), `j2-next` (gating, 1 min), `j2-assisted` (tracked; agent proposes mapping, human adopts); checks replay_outcome, lexical_identical, basis_present, lint_findings_equal (5), no_manual_entry, file_digest. Register datasets in `datasets.toml`. Verify AC-1..AC-4 of spec node FLOW.REPORT.J2.

## Open questions

- No node yet owns a lint that compares drawing requirements with transcribed source nominals and tolerances; the map assumes `report.check_fai@1` reports them. This needs an owner in RPT (likely `RPT.BALLOON.CHAR` or `RPT.FORMS.FINAL`).
- Form layout follows D-015 (accepted, option C: general designer, no reproduced form layout; counsel review before release).

## Children

_Leaf: none._
