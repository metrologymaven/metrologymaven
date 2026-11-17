---
id: RPT.FORMS.PPAP
title: PPAP dimensional results data
level: 3
parent: RPT.FORMS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.FORMS-002]
covers: []
depends_on: [RPT.BALLOON.REG, RPT.MODEL.FMT, CORE.PRJ]
cli: [fid report create-ppap, fid report generate-ppap]
mcp: [report_create_ppap, report_generate_ppap]
journal: [report.create_ppap@1]
updated: 2026-11-16
---

# RPT.FORMS.PPAP: PPAP dimensional results data

## Purpose

The data behind a PPAP dimensional results report: a PPAP record with its header (part, revision, organisation, submission context) and rows for every dimension and note on the design record, with specification, results for each sample part and an OK / not-OK outcome. It builds the draft as a scratch result; how it looks on paper belongs to `RPT.FORMS.LAYOUT`.

## Scope

**In:**
- `PpapRecord` with header fields in our own words, citing the AIAG PPAP manual by edition only [unverified edition]
- Rows per ballooned characteristic with results from several sample parts (multi-part run)
- Per-row outcome derived from the journaled decisions
- `report.generate_ppap@1`: the draft as a `ResolvedReport` (R1)

**Out** (and which node owns it instead):
- Other PPAP elements (control plan, PFMEA, MSA studies): not in scope of RPT
- Finalisation (`RPT.FORMS.FINAL`)
- Visual layout (`RPT.FORMS.LAYOUT`)

## Requirements

- **REQ-RPT.FORMS.PPAP-001** When `report.create_ppap@1` runs for a part revision, the system shall create a PPAP record with a UUIDv7 ID and journal one event.
- **REQ-RPT.FORMS.PPAP-002** The dimensional results shall contain one row per ballooned characteristic in ascending balloon-number order, with one result column per sample part in part-sequence order.
- **REQ-RPT.FORMS.PPAP-003** A row's outcome shall be OK only if every sample part's decision is `CONFORMS`; any other outcome shall be shown as not OK together with the outcomes that caused it.
- **REQ-RPT.FORMS.PPAP-004** `report.generate_ppap@1` shall be side-effect class R1, journal nothing, and produce identical content digests for the same journal head and PPAP record.
- **REQ-RPT.FORMS.PPAP-005** The field list and labels shall be written in the project's own words, citing the AIAG PPAP manual by edition and section only.

## Interfaces

- **Offers:** `report.create_ppap@1` (W2), `report.generate_ppap@1` (R1); types `PpapRecord`, `PpapDimRow`.
- **Consumes:** balloon register (`RPT.BALLOON.REG`), formatter layer (`RPT.MODEL.FMT`), project container and multi-part runs (`CORE.PRJ`).

## Decomposition hints

REC (record and header), ROW (multi-sample rows and outcome).

## Open questions

- Number of sample parts expected by default (often six for many customers) [unverified]; make it a header setting.

## Children

_None yet: status `open`._
