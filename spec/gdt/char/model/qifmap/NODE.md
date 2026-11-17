---
id: GDT.CHAR.MODEL.QIFMAP
title: Mapping between QIF model types and characteristic types
level: 4
parent: GDT.CHAR.MODEL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.CHAR.MODEL-001, REQ-GDT.CHAR.MODEL-003]
covers: []
depends_on: [GDT.CHAR.MODEL.TYPES, IO.QIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.CHAR.MODEL.QIFMAP: Mapping between QIF model types and characteristic types

## Purpose

Two pure functions: `from_qif` turns the generated QIF 3.0 characteristic elements (as parsed by `IO.QIF`) into `Characteristic` values, and `to_qif` turns them back. IO owns XML and JSON; this leaf owns only the field-by-field mapping, so that import and export are lossless and a QIF characteristic type the model does not know fails loudly instead of being dropped.

## Scope

**In:** `from_qif(&QifCharacteristicSet) -> Result<Vec<Characteristic>, MapError>`; `to_qif(&[Characteristic]) -> QifCharacteristicSet`; definition, nominal, item and measurement linkage by QIF id; FCF fields; QIF ids, QPIds and the whole `Attributes` bag (including any `fx:qpid`) carried verbatim, never minted or interpreted here; extension payloads carried in QIF `Attributes` through IO's extension bag; the mapping table as a generated document.
**Out:** XML and JSON parsing and writing (owned by `IO.QIF`); the types (owned by `GDT.CHAR.MODEL.TYPES`); corpus round-trip runs (owned by `GDT.CHAR.MODEL.RTRIP`); AP242 to QIF mapping (owned by `IO.CAD.PMI`).

## Requirements

- **REQ-GDT.CHAR.MODEL.QIFMAP-001** `from_qif` shall map every QIF 3.0 characteristic definition, nominal, item and measurement element to the matching `Characteristic` fields, preserving every QIF id and QPId.
- **REQ-GDT.CHAR.MODEL.QIFMAP-002** If `from_qif` meets a QIF characteristic type with no `CharacteristicKind` variant, then it shall fail with `unsupported_characteristic_type{qif_type, id}` for the whole import and shall not drop the element.
- **REQ-GDT.CHAR.MODEL.QIFMAP-003** `to_qif(from_qif(x))` shall equal `x` under the `qif-norm@1` equality for every characteristic element of `x`.
- **REQ-GDT.CHAR.MODEL.QIFMAP-004** An `Extension` characteristic shall be written to and read from the QIF `Attributes` hook with its owner, kind, schema version and payload unchanged.
- **REQ-GDT.CHAR.MODEL.QIFMAP-005** If a QIF item references a definition or nominal id that is not present, then `from_qif` shall fail with `dangling_reference{id}`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/model/qif_fcf_sampler.qif` (one characteristic of each P1a-relevant QIF type plus a composite position FCF), when `from_qif` then `to_qif` run, then the output equals the input under `qif-norm@1`.
- **AC-2** (error path) Given a QIF document with a characteristic type whose variant is removed in a test build, then import fails with `unsupported_characteristic_type` naming the type and id.
- **AC-3** Given a comparison-point `Extension` written by `to_qif`, when read by `from_qif`, then owner, kind, version and payload bytes are unchanged.
- **AC-4** (error path) Given an item whose nominal id is missing, then `dangling_reference{id}`.
- **AC-5** (determinism) Mapping AC-1 twice yields byte-identical canonical JSON of the characteristics.

## Design

- **Registry command(s):** none; called by `IO.QIF` import and export and by `GDT.CHAR.MODEL.CMDS`.
- **Data structures:** `MapError { UnsupportedCharacteristicType{qif_type, id}, DanglingReference{id}, InvalidFcf{reason} }`.
- **Algorithm:** index QIF elements by id in a `BTreeMap`; walk definitions, nominals, items, measurements in document order; build types through `GDT.CHAR.MODEL.TYPES` constructors; ids and attributes copied verbatim (identity minting and `fx:qpid` reading stay in `GDT.CHAR.IDENT`, which calls this module, not the reverse).
- **Errors:** as above.
- **Determinism:** D0.
- **Performance budget:** 10 000 characteristics < 200 ms [unverified].
- **Crate:** `fid-gdt` (module `model::qif`).

## Test fixtures

Synthetic QIF 3.0 files written for this leaf (CC0); corpus files are exercised by `GDT.CHAR.MODEL.RTRIP`.

## Build prompt

> In crate `fid-gdt`, module `model::qif`, implement `from_qif` and `to_qif` between the generated QIF 3.0 characteristic types (`fid-qif-model`, from IO) and the `Characteristic` types in `model`. Preserve every QIF id, QPId and the whole `Attributes` bag verbatim (do not mint or interpret identity here); fail the whole import with `unsupported_characteristic_type{qif_type, id}` for unknown types and `dangling_reference{id}` for missing references; carry `Extension` characteristics through the QIF `Attributes` hook unchanged. Prove `to_qif(from_qif(x)) == x` under the `qif-norm@1` comparison from IO for the fixture `fixtures/gdt/model/qif_fcf_sampler.qif`. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`; input hostile), no std `HashMap`/`HashSet`, D0.

## Open questions

- None.

## Children

_Leaf: none._
