---
id: RPT.FORMS.ROWS
title: Characteristic accountability rows (form 3)
level: 3
parent: RPT.FORMS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-003, REQ-RPT.FORMS-004]
covers: []
depends_on: [RPT.BALLOON.REG, RPT.MODEL.FMT, RPT.FORMS.HDR]
cli: [fid report generate-fai]
mcp: [report_generate_fai]
journal: []
updated: 2026-11-16
---

# RPT.FORMS.ROWS: Characteristic accountability rows (form 3)

## Purpose

Builds the form 3 table of an FAI: one row per ballooned characteristic, in balloon-number order, showing the requirement, the result, its evaluation basis and the tool or software that produced it. It also assembles the whole draft FAI (header from `RPT.FORMS.HDR` plus these rows) as a scratch result. It reads the balloon register and journaled ResultRecords; it never creates results.

## Scope

**In:**
- Row builder from the balloon register of one drawing revision and the ResultRecords linked to each characteristic
- Requirement text from the characteristic (nominal, tolerances, callout) through `RPT.MODEL.FMT`
- Result, decision, basis and producing tool (gauge ID, CMM program, scan inspection, or transcription source)
- Partial-FAI filtering to the characteristics in scope
- `report.generate_fai@1`: the draft FAI as a `ResolvedReport` (R1)

**Out** (and which node owns it instead):
- Balloon numbering (`RPT.BALLOON.REG`)
- Header and forms 1–2 data (`RPT.FORMS.HDR`)
- Completeness check and finalisation (`RPT.FORMS.FINAL`)
- Visual layout (`RPT.FORMS.LAYOUT`)

## Requirements

- **REQ-RPT.FORMS.ROWS-001** The form 3 table shall contain exactly one row per ballooned characteristic of the drawing revision in scope, ordered by ascending balloon number.
- **REQ-RPT.FORMS.ROWS-002** Each row shall show the balloon number, the characteristic's requirement, the formatted result, the decision outcome, the evaluation basis and the producing tool or software.
- **REQ-RPT.FORMS.ROWS-003** When a characteristic has several journaled results, the row shall use the latest result of the run selected for the FAI and shall name that run.
- **REQ-RPT.FORMS.ROWS-004** When a ballooned characteristic has no journaled result, its row shall show the fixed text "no result" and shall not be omitted.
- **REQ-RPT.FORMS.ROWS-005** When the FAI is partial, only characteristics in the header's scope list shall get rows.
- **REQ-RPT.FORMS.ROWS-006** `report.generate_fai@1` shall be side-effect class R1, journal nothing, and produce identical content digests for the same journal head and FAI record.

## Interfaces

- **Offers:** `report.generate_fai@1` (R1); type `FaiForm3Row`; a draft FAI `ResolvedReport`.
- **Consumes:** balloon register (`RPT.BALLOON.REG`), formatter layer (`RPT.MODEL.FMT`), header (`RPT.FORMS.HDR`), ResultRecords (`GDT.CHAR`).

## Decomposition hints

ROW (row builder), DRAFT (draft FAI assembly and command).

## Open questions

- Which result to use when a characteristic was measured several times in the selected run (latest, or all as sub-rows)? Assumed latest.

## Children

_None yet: status `open`._
