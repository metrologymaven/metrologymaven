---
id: RPT.MODEL.BIND
title: Binding resolution and report generation
level: 3
parent: RPT.MODEL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL-002, REQ-RPT.MODEL-003, REQ-RPT.MODEL-004, REQ-RPT.MODEL-013]
covers: [GOM.RPT.07]
depends_on: [RPT.MODEL.TMPL, RPT.MODEL.FMT, CORE.JRN, GDT.CHAR]
cli: [fid report generate-report, fid report bind-block, fid report save-report]
mcp: [report_generate_report, report_bind_block, report_save_report]
journal: [report.bind_block@1, report.save_report@1]
updated: 2026-11-17
---

# RPT.MODEL.BIND: Binding resolution and report generation

## Purpose

The engine that turns a report or template plus a journal head into a `ResolvedReport`: it resolves every binding and slot to journaled ResultRecords, saved views and statistics series by stable ID, expands repetition rules, calls the formatter layer for every value, and computes the content digest. It is the heart of the reproducibility promise (REQ-RPT-001), so it is a unit of its own.

## Scope

**In:**
- Binding a block to stable IDs (`report.bind_block@1`)
- Generation against an explicit journal head (R1 scratch result)
- Expansion of slots and repetition rules into rows and pages
- Multi-stage and multi-run binding within one project, side by side
- `ResolvedReport` type with canonical JSON and SHA-256 content digest; provenance header (journal head hash, template ID and version, kernel version, formatter IDs)
- Saving a resolved report into the project (W2)

**Out** (and which node owns it instead):
- Formatting numbers and labels (`RPT.MODEL.FMT`)
- Template format and slot language (`RPT.MODEL.TMPL`)
- Computing statistics series (`RPT.STATS`)
- Rendering to files (`RPT.OUTPUT`)

## Requirements

- **REQ-RPT.MODEL.BIND-001** A bound block shall store only stable IDs of ResultRecords, saved views and statistics series, never copies of their values.
- **REQ-RPT.MODEL.BIND-002** When generation runs, the system shall resolve every binding against the journal head given as input and produce a `ResolvedReport` with an RFC 8785 canonical JSON SHA-256 content digest.
- **REQ-RPT.MODEL.BIND-003** When a report is generated twice from the same journal head, template version and formatter versions, the two resolved reports shall have identical content digests.
- **REQ-RPT.MODEL.BIND-004** If a binding resolves to a ResultRecord that is not in the journal up to the given head, then generation shall fail with error `report.unjournaled_record` naming the binding and journal nothing.
- **REQ-RPT.MODEL.BIND-005** If a binding names a ResultRecord of another project, then generation shall fail with error `report.cross_project_binding`; cross-project data enters reports only as statistics series.
- **REQ-RPT.MODEL.BIND-006** When a report binds results from several stages or runs of one project, the system shall show them side by side in stage order, then run time order.
- **REQ-RPT.MODEL.BIND-007** A resolved report shall record the journal head hash, template ID and version, kernel version and every formatter ID and version it used.
- **REQ-RPT.MODEL.BIND-008** Generation into scratch shall be side-effect class R1 and journal nothing; storing a resolved report in the project shall be the separate W2 command `report.save_report@1`.
- **REQ-RPT.MODEL.BIND-009** Generation of a 500-characteristic, three-stage report shall complete in under 2 s on reference hardware [unverified].
- **REQ-RPT.MODEL.BIND-010** When generation resolves a `value_binding@1`, the system shall read the bound record and its fid-kunc uncertainty statement from the project store, recompute both SHA-256 digests, and fail with error `report.binding_digest_mismatch` naming the binding if either differs from `record_digest` or `statement_digest`; values and labels shall come only from the stored record and statement, never from the block or the caller.

## Interfaces

- **Offers:** `report.generate_report@1` (R1), `report.bind_block@1` (W2), `report.save_report@1` (W2); type `ResolvedReport` consumed by `RPT.OUTPUT`, `RPT.DESIGN` preview and `RPT.FORMS`.
- **Consumes:** templates and slots (`RPT.MODEL.TMPL`), formatter layer (`RPT.MODEL.FMT`), journal reads (`CORE.JRN`), ResultRecords (`GDT.CHAR`), chart series (`RPT.STATS`), saved views (`VIZ.VIEW`).

## Decomposition hints

RESOLVE (binding lookup against a journal head), EXPAND (repetition and multi-stage expansion), DIGEST (resolved document, provenance header and digest), SAVE (save command).

## Open questions

- Should a resolved report embed snapshot images or reference them by digest only? Assumed by digest, with `RPT.OUTPUT` fetching bytes.
- **Applied at split (2026-11-17):** snapshots are referenced by digest only (`RPT.MODEL.BIND.GEN` REQ-006). Block kinds plug in through the `BlockResolver` trait of `RPT.MODEL.BIND.RESOLVE`; form data plugs in through the `FieldSource` trait of `RPT.MODEL.BIND.RESOLVE`. Saved views and chart series are resolved by their block kinds, so this module needs no code edge to VIZ (L3) or RPT.STATS.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.MODEL.BIND.RESOLVE | Block binding and resolution of stable IDs against a journal head (leaf) | 001 004 005 010 | GOM.RPT.07 |
| RPT.MODEL.BIND.EXPAND | Slot, repetition and multi-stage expansion (leaf) | 006 | GOM.RPT.07 |
| RPT.MODEL.BIND.GEN | Report generation, resolved document, provenance and digest (leaf) | 002 003 007 008 009 | none |
