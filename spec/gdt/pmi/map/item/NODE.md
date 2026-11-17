---
id: GDT.PMI.MAP.ITEM
title: PMI entity to proposal item with evaluability status
level: 4
parent: GDT.PMI.MAP
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PMI.MAP-001, REQ-GDT.PMI.MAP-002]
covers: [GOM.GDT.16, SA.GDT.01]
depends_on: [GDT.CHAR.MODEL.QIFMAP, GDT.RES.RESOLVE, GDT.PMI.LINK]
uses_artifacts: [IO.CAD.PMI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PMI.MAP.ITEM: PMI entity to proposal item with evaluability status

## Purpose

A pure function from one imported PMI entity to one proposal item. Semantic PMI arrives already mapped to QIF MBD characteristics by `IO.CAD.PMI` (or directly from a QIF MBD file), together with an import report that also lists graphic-only and unsupported entities. This leaf turns each mapped QIF characteristic into a `Characteristic` through the existing QIF mapping, attaches its `PmiLink`, checks that its faces exist, asks the resolver whether it resolves, and marks the item `evaluable` or `not_evaluable{reason}`. Report entries that never became characteristics still yield an item, so nothing disappears from the reviewer's list.

## Scope

**In:** `map_entity(entry: PmiReportEntry, qif: Option<&QifCharacteristic>, cad: &CadFaceIndex, governing: &GoverningSpec, dmt) -> ProposalItem`; `from_qif` from `GDT.CHAR.MODEL.QIFMAP`; `PmiLink` from `GDT.PMI.LINK`; face-existence check; resolver call; reasons `graphic_only`, `unsupported_in_import`, `missing_geometry{face}`, `unresolved{key}`, `unsupported_in_dmt{row}`, `unsupported_characteristic_type`.
**Out:** AP242 to QIF MBD mapping and the import report (owned by `IO.CAD.PMI`, read as its published report format); governing-spec proposal and the command (owned by `GDT.PMI.MAP.CMD`); adoption (owned by `GDT.PMI.REVIEW`).

## Requirements

- **REQ-GDT.PMI.MAP.ITEM-001** For each import-report entry the function shall return exactly one `ProposalItem` carrying the source PMI entity ID.
- **REQ-GDT.PMI.MAP.ITEM-002** A mapped entry whose faces all exist and whose characteristic resolves to a `supported` or `partial` row shall yield status `evaluable` with the mapped characteristic and its `PmiLink`.
- **REQ-GDT.PMI.MAP.ITEM-003** An entry that is graphic-only, unsupported at import, references a missing face, maps to an unknown characteristic type, or resolves `UNRESOLVED` or `UNSUPPORTED` shall yield status `not_evaluable` with the first applicable reason in that order, and no evaluable characteristic.
- **REQ-GDT.PMI.MAP.ITEM-004** The function shall be pure: identical inputs give byte-identical canonical JSON output.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pmi/report_mixed.json` (a synthetic import report with one mapped flatness, one mapped position at MMC, one graphic-only note, one unsupported entity, one tolerance on a deleted face) with matching QIF characteristics and a synthetic DMT supporting flatness only, when each entry is mapped, then the items are: flatness `evaluable`; position `not_evaluable{unresolved}`; note `not_evaluable{graphic_only}`; unsupported `not_evaluable{unsupported_in_import}`; deleted face `not_evaluable{missing_geometry}`.
- **AC-2** Given AC-1, every item carries its source PMI entity ID and evaluable items carry a `PmiLink` with face IDs.
- **AC-3** (determinism) Running AC-1 twice yields byte-identical output.

## Design

- **Registry command(s):** none (pure function used by `GDT.PMI.MAP.CMD`).
- **Data structures:** `ProposalItem { pmi_entity_id, status: Evaluable{characteristic, link} | NotEvaluable{reason, link?}, resolution? }`; `PmiReportEntry` as a read-only view of the published report format (schema version checked).
- **Algorithm:** classify by report status, then map, then face check, then resolve; first failing check gives the reason.
- **Errors:** `unsupported_report_schema{version}` (whole call); all per-entity problems are item statuses, not errors.
- **Determinism:** D0.
- **Performance budget:** 5 000 entities < 500 ms [unverified].
- **Crate:** `fid-gdt` (module `pmi::map`).

## Test fixtures

Synthetic import reports and QIF MBD snippets (CC0); real AP242 models are exercised by `GDT.PMI.MAP.CORPUS`.

## Build prompt

> In crate `fid-gdt`, module `pmi::map`, implement the pure function `map_entity(entry, qif, cad_faces, governing, dmt) -> ProposalItem`. Read the import-report entry (a view over the PMI import report format published by IO, checking its schema version), map the QIF MBD characteristic with `model::qif::from_qif`, attach the `PmiLink`, check every face ID exists, call the resolver, and return exactly one item per entry: `Evaluable{characteristic, link}` or `NotEvaluable{reason}` with reasons in the order `graphic_only`, `unsupported_in_import`, `unsupported_characteristic_type`, `missing_geometry{face}`, `unresolved{key}`, `unsupported_in_dmt{row}`. Output canonical JSON must be byte-identical for identical input. Tests per acceptance criteria. Constraints: CONSTITUTION C6 (PMI text is untrusted data), §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`.

## Open questions

- None.

## Children

_Leaf: none._
